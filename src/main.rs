use anyhow::{Context, bail};
use hazel::{
    Repo, Sha, git,
    installation::Installation,
    port_allocator::PortAllocator,
    production::{ProductionDeployment, build_production, kill_production, run_production},
    staging::{StagingDeployment, deploy_staging, kill_staging},
};
use octocrab::{Octocrab, models::AppId};
use serde::Deserialize;
use std::{collections::HashMap, env, path::PathBuf, time::Duration};
use tokio::task::JoinSet;
use tracing::{debug, info, warn};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .init();

    let data_dir = initialize_data_dir().await?;
    let app_client = initialize_app_client().await?;
    let mut port_allocator = initialize_port_allocator()?;
    let poll_interval = initialize_poll_interval()?;
    let tailscale_hostname = get_tailscale_hostname().await?;

    // intentionally just watching one repository because YAGNI,
    // but it wouldn't be hard to generalize this to just query for
    // repositories where the app is installed.
    let repo = initialize_watched_repo()?;
    let installation = initialize_installation(&app_client, repo.clone()).await?;

    // Production config - all optional, only enabled if HAZEL_PRODUCTION_ENABLE=true
    let production_enabled = env::var("HAZEL_PRODUCTION_ENABLE").ok().as_deref() == Some("true");
    let production_branch = env::var("HAZEL_PRODUCTION_BRANCH").unwrap_or_else(|_| "main".into());
    let production_port: Option<u16> = env::var("HAZEL_PRODUCTION_PORT")
        .ok()
        .and_then(|p| p.parse().ok());
    let production_run_dir: Option<PathBuf> =
        env::var("HAZEL_PRODUCTION_RUN_DIR").ok().map(PathBuf::from);

    let repo_dir = data_dir.join("repos").join(repo.to_string());
    let bare_repo = git::ensure_bare_repo(&repo_dir).await?;

    let mut staging_deployments: HashMap<Sha, StagingDeployment> = HashMap::new();
    let mut production_deployment: Option<ProductionDeployment> = None;

    info!(
        repo = %repo,
        poll_interval_secs = poll_interval.as_secs(),
        "hazel started"
    );

    // choosing to do the silly thing and poll because i don't feel like
    // setting up a webhook receiver.
    loop {
        let targets = match installation.fetch_deploy_targets(&app_client).await {
            Ok(t) => t,
            Err(e) => {
                warn!(error = ?e, "failed to fetch deploy targets");
                tokio::select! {
                    _ = tokio::time::sleep(poll_interval) => continue,
                    _ = tokio::signal::ctrl_c() => break,
                }
            }
        };

        let new_targets: Vec<_> = targets
            .iter()
            .filter(|t| !staging_deployments.contains_key(&t.sha))
            .collect();

        if !new_targets.is_empty() {
            info!(count = new_targets.len(), "deploying new targets");

            let mut set: JoinSet<anyhow::Result<(StagingDeployment, u64)>> = JoinSet::new();
            for target in new_targets {
                let port = port_allocator.allocate()?;
                let bare_repo = bare_repo.clone();
                let data_dir = data_dir.clone();
                let sha = target.sha.clone();
                let fetch_url = target.fetch_url.clone();
                let pr_number = target.pr_number;

                let tailscale_hostname = tailscale_hostname.clone();
                set.spawn(async move {
                    let checkout_dir = data_dir.join("checkouts").join(sha.as_str());
                    git::extract_commit(
                        &bare_repo,
                        fetch_url.as_str(),
                        sha.as_str(),
                        &checkout_dir,
                    )
                    .await?;

                    let run_dir = data_dir.join("staging").join(sha.as_str());
                    let deployment =
                        deploy_staging(&sha, &checkout_dir, &run_dir, port, &tailscale_hostname)
                            .await?;
                    Ok((deployment, pr_number))
                });
            }

            while let Some(result) = set.join_next().await {
                match result {
                    Ok(Ok((deployment, pr_number))) => {
                        info!(sha = %deployment.sha, port = deployment.port, "staging deployment succeeded");

                        let preview_url =
                            format!("http://{}:{}/", tailscale_hostname, deployment.port);
                        if let Err(e) = installation
                            .upsert_deploy_comment(pr_number, &preview_url)
                            .await
                        {
                            warn!(error = ?e, pr = pr_number, "failed to post deploy comment");
                        }

                        staging_deployments.insert(deployment.sha.clone(), deployment);
                    }
                    Ok(Err(e)) => {
                        warn!(error = ?e, "staging deployment failed");
                    }
                    Err(e) => {
                        warn!(error = ?e, "task panicked");
                    }
                }
            }
        }

        let target_shas: std::collections::HashSet<_> = targets.iter().map(|t| &t.sha).collect();
        for sha in staging_deployments
            .keys()
            .filter(|sha| !target_shas.contains(sha))
            .cloned()
            .collect::<Vec<_>>()
        {
            if let Some(mut deployment) = staging_deployments.remove(&sha) {
                kill_staging(&mut deployment).await;
                port_allocator.release(deployment.port);
            }
        }

        // Production deployment
        if production_enabled {
            let production_port =
                production_port.expect("HAZEL_PRODUCTION_PORT required when production enabled");
            let production_run_dir = production_run_dir
                .as_ref()
                .expect("HAZEL_PRODUCTION_RUN_DIR required when production enabled");

            match installation.fetch_branch_sha(&production_branch).await {
                Ok(branch_sha) => {
                    let needs_deploy = production_deployment
                        .as_ref()
                        .map(|d| d.sha != branch_sha)
                        .unwrap_or(true);

                    if needs_deploy {
                        info!(
                            branch = %production_branch,
                            sha = %branch_sha,
                            "production branch updated, deploying"
                        );

                        // Get fetch URL with token
                        let token = installation.ensure_token(&app_client).await?;
                        let fetch_url = format!(
                            "https://x-access-token:{}@github.com/{}.git",
                            secrecy::ExposeSecret::expose_secret(&token),
                            installation.repo
                        );

                        // Extract commit to checkout dir
                        let checkout_dir = data_dir.join("checkouts").join(branch_sha.as_str());
                        if let Err(e) = git::extract_commit(
                            &bare_repo,
                            &fetch_url,
                            branch_sha.as_str(),
                            &checkout_dir,
                        )
                        .await
                        {
                            warn!(error = ?e, sha = %branch_sha, "failed to extract production commit");
                        } else {
                            match build_production(&checkout_dir).await {
                                Ok(_) => {
                                    // Kill old deployment now that new is built
                                    if let Some(mut old) = production_deployment.take() {
                                        kill_production(&mut old).await;
                                    }

                                    match run_production(
                                        &branch_sha,
                                        &checkout_dir,
                                        production_run_dir,
                                        production_port,
                                        &tailscale_hostname,
                                    )
                                    .await
                                    {
                                        Ok(deployment) => {
                                            info!(
                                                sha = %deployment.sha,
                                                port = production_port,
                                                "production deployment succeeded"
                                            );
                                            production_deployment = Some(deployment);
                                        }
                                        Err(e) => {
                                            warn!(error = ?e, sha = %branch_sha, "production deployment failed");
                                        }
                                    }
                                }
                                Err(e) => {
                                    warn!(error = ?e, sha = %branch_sha, "production build failed");
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    warn!(error = ?e, branch = %production_branch, "failed to fetch branch sha");
                }
            }
        }

        info!(
            staging = staging_deployments.len(),
            production = production_deployment.is_some(),
            targets = targets.len(),
            "poll complete"
        );

        tokio::select! {
            _ = tokio::time::sleep(poll_interval) => {}
            _ = tokio::signal::ctrl_c() => break,
        }
    }

    info!("shutting down");

    // Kill staging deployments
    for (_, mut deployment) in staging_deployments {
        kill_staging(&mut deployment).await;
    }

    // Kill production deployment
    if let Some(mut deployment) = production_deployment {
        kill_production(&mut deployment).await;
    }

    // Clean up checkouts and staging dirs (keep repos for git cache)
    if let Err(e) = tokio::fs::remove_dir_all(data_dir.join("checkouts")).await {
        warn!(error = ?e, "failed to clean checkouts");
    }
    if let Err(e) = tokio::fs::remove_dir_all(data_dir.join("staging")).await {
        warn!(error = ?e, "failed to clean staging");
    }

    info!("cleanup complete");
    Ok(())
}

async fn initialize_data_dir() -> anyhow::Result<PathBuf> {
    let data_dir = PathBuf::from(env::var("HAZEL_DATA_DIR").context("HAZEL_DATA_DIR not set")?);

    // Clean up stale checkouts/staging from previous runs (ignore errors if they don't exist)
    let _ = tokio::fs::remove_dir_all(data_dir.join("checkouts")).await;
    let _ = tokio::fs::remove_dir_all(data_dir.join("staging")).await;

    tokio::try_join!(
        tokio::fs::create_dir_all(data_dir.join("repos")),
        tokio::fs::create_dir_all(data_dir.join("checkouts")),
        tokio::fs::create_dir_all(data_dir.join("staging")),
    )
    .context("failed to create directories")?;

    let data_dir = tokio::fs::canonicalize(&data_dir)
        .await
        .with_context(|| format!("failed to canonicalize {data_dir:?}"))?;

    Ok(data_dir)
}

async fn initialize_app_client() -> anyhow::Result<Octocrab> {
    let app_id: u64 = env::var("GITHUB_APP_ID")
        .context("GITHUB_APP_ID not set")?
        .parse()
        .context("GITHUB_APP_ID must be a number")?;
    let key_path = env::var("GITHUB_APP_KEY_PATH").context("GITHUB_APP_KEY_PATH not set")?;
    let key = tokio::fs::read_to_string(&key_path)
        .await
        .with_context(|| format!("failed to read key from {key_path}"))?;
    let key = jsonwebtoken::EncodingKey::from_rsa_pem(key.as_bytes())?;

    Octocrab::builder()
        .app(AppId(app_id), key)
        .build()
        .context("app client failed to build")
}

async fn initialize_installation(
    app_client: &Octocrab,
    repo: Repo,
) -> anyhow::Result<Installation> {
    let installation_info = app_client
        .apps()
        .get_repository_installation(&repo.owner, &repo.name)
        .await
        .context("failed to get installation")?;

    let installation_client = app_client.installation(installation_info.id)?;
    let access_tokens_url = installation_info
        .access_tokens_url
        .context("installation missing access_tokens_url")?
        .to_string();

    Ok(Installation::new(
        installation_client,
        access_tokens_url,
        repo,
    ))
}

fn initialize_port_allocator() -> anyhow::Result<PortAllocator> {
    let port_min: u16 = env::var("HAZEL_PORT_MIN")
        .context("HAZEL_PORT_MIN not set")?
        .parse()
        .context("HAZEL_PORT_MIN must be a number")?;
    let port_max: u16 = env::var("HAZEL_PORT_MAX")
        .context("HAZEL_PORT_MAX not set")?
        .parse()
        .context("HAZEL_PORT_MAX must be a number")?;

    Ok(PortAllocator::new(port_min, port_max))
}

fn initialize_poll_interval() -> anyhow::Result<Duration> {
    let secs: u64 = env::var("HAZEL_POLL_INTERVAL_SECS")
        .context("HAZEL_POLL_INTERVAL_SECS not set")?
        .parse()
        .context("HAZEL_POLL_INTERVAL_SECS must be a number")?;

    Ok(Duration::from_secs(secs))
}

fn initialize_watched_repo() -> anyhow::Result<Repo> {
    let owner = env::var("HAZEL_WATCHED_REPO_OWNER").context("HAZEL_WATCHED_REPO_OWNER not set")?;
    let name = env::var("HAZEL_WATCHED_REPO_NAME").context("HAZEL_WATCHED_REPO_NAME not set")?;

    Ok(Repo::new(owner, name))
}

async fn get_tailscale_hostname() -> anyhow::Result<String> {
    #[derive(Deserialize)]
    struct TailscaleStatus {
        #[serde(rename = "Self")]
        self_node: SelfNode,
    }

    #[derive(Deserialize)]
    struct SelfNode {
        #[serde(rename = "DNSName")]
        dns_name: String,
    }

    let output = tokio::process::Command::new("tailscale")
        .args(["status", "--self", "--json"])
        .output()
        .await
        .context("failed to run tailscale status")?;

    if !output.status.success() {
        bail!(
            "tailscale status failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let status: TailscaleStatus =
        serde_json::from_slice(&output.stdout).context("failed to parse tailscale status")?;

    // DNSName looks like "hostname.tail1234.ts.net." - extract just the hostname
    let hostname = status
        .self_node
        .dns_name
        .trim_end_matches('.')
        .split('.')
        .next()
        .context("invalid DNSName format")?
        .to_string();

    Ok(hostname)
}
