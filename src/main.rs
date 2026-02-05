use anyhow::{Context, bail};
use hazel::{
    Repo, Sha,
    deploy::{BuildLogs, kill_process},
    env_config::{EnvConfig, load_env_config},
    git,
    installation::{DeployComment, DeployTarget, Installation},
    port_allocator::PortAllocator,
    production::{build_production, run_production},
    staging::{StagingResult, deploy_staging},
};
use octocrab::{Octocrab, models::AppId};
use serde::Deserialize;
use std::{collections::HashMap, env, path::Path};
use tokio::process::Child;
use tokio::task::JoinSet;
use tracing::{debug, info, warn};

struct StagingDeployment {
    port: u16,
    process: Child,
}

struct ProductionDeployment {
    sha: Sha,
    process: Child,
}

struct ProductionConfig {
    branch: String,
    port: u16,
    origin: String,
    run_dir: &'static Path,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .init();

    let data_dir: &'static Path = Box::leak(initialize_data_dir().await?);
    let app_client = initialize_app_client().await?;
    let mut port_allocator = initialize_port_allocator()?;
    let poll_interval = initialize_poll_interval()?;
    let tailscale_hostname: &'static str =
        Box::leak(get_tailscale_hostname().await?.into_boxed_str());
    let env_config: &'static EnvConfig = Box::leak(Box::new(load_env_config()));

    let repo = initialize_watched_repo()?;
    let installation = initialize_installation(&app_client, repo.clone()).await?;

    let production_config = initialize_production_config()?;

    // intentionally just watching one repository because YAGNI,
    // but it wouldn't be hard to generalize this to just query for
    // repositories where the app is installed.
    let repo_dir = data_dir.join("repos").join(repo.to_string());
    let bare_repo: &'static Path =
        Box::leak(git::ensure_bare_repo(&repo_dir).await?.into_boxed_path());

    let mut staging_deployments: HashMap<Sha, StagingDeployment> = HashMap::new();
    let mut production_deployment: Option<ProductionDeployment> = None;

    info!(
        repo = %repo,
        poll_interval_secs = poll_interval.as_secs(),
        "hazel started"
    );

    // choosing to do the silly thing and poll because i don't want to
    // expose a webhook receiver.
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

        // --- Staging ---
        let new_targets: Vec<_> = targets
            .iter()
            .filter(|t| !staging_deployments.contains_key(&t.sha))
            .collect();

        if !new_targets.is_empty() {
            info!(count = new_targets.len(), "deploying new targets");
            if let Err(e) = deploy_staging_targets(
                &new_targets,
                &mut port_allocator,
                bare_repo,
                data_dir,
                tailscale_hostname,
                &installation,
                &mut staging_deployments,
                env_config,
            )
            .await
            {
                warn!(error = ?e, "staging deployment spawn failed");
            }
        }

        cleanup_dead_staging(&targets, &mut staging_deployments, &mut port_allocator).await;

        // --- Production ---
        if let Some(ref config) = production_config
            && let Err(e) = poll_production(
                &installation,
                &app_client,
                bare_repo,
                data_dir,
                &mut production_deployment,
                config,
                env_config,
            )
            .await
        {
            warn!(error = ?e, "production poll failed");
        }

        debug!(
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

    for (_, mut deployment) in staging_deployments {
        kill_process(&mut deployment.process).await;
    }

    if let Some(mut deployment) = production_deployment {
        kill_process(&mut deployment.process).await;
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

async fn deploy_staging_targets(
    targets: &[&DeployTarget],
    port_allocator: &mut PortAllocator,
    bare_repo: &'static Path,
    data_dir: &'static Path,
    tailscale_hostname: &'static str,
    installation: &Installation,
    staging_deployments: &mut HashMap<Sha, StagingDeployment>,
    env_config: &'static EnvConfig,
) -> anyhow::Result<()> {
    let mut set: JoinSet<(StagingResult, u64, Sha, u16)> = JoinSet::new();

    for &target in targets {
        let port = port_allocator.allocate()?;
        let DeployTarget {
            sha,
            fetch_url,
            pr_number,
        } = target.clone();

        set.spawn(async move {
            let checkout_dir = data_dir.join("checkouts").join(sha.as_str());

            if let Err(e) =
                git::extract_commit(bare_repo, fetch_url.as_str(), sha.as_str(), &checkout_dir)
                    .await
            {
                let logs = BuildLogs {
                    pre_start_build: Some(Err(hazel::deploy::BuildOutput {
                        stdout: String::new(),
                        stderr: format!("git extract failed: {}", e),
                    })),
                    ..Default::default()
                };
                return (Err(logs), pr_number, sha, port);
            }

            let run_dir = data_dir.join("staging").join(sha.as_str());
            let result = deploy_staging(&checkout_dir, &run_dir, port, tailscale_hostname, env_config).await;
            (result, pr_number, sha, port)
        });
    }

    while let Some(result) = set.join_next().await {
        let (staging_result, pr_number, sha, port) = match result {
            Ok(r) => r,
            Err(e) => {
                warn!(error = ?e, "staging task panicked");
                continue;
            }
        };

        let comment = match staging_result {
            Ok((process, logs)) => {
                info!(sha = %sha, port, "staging deployment succeeded");
                let preview_url = format!("http://{}:{}/", tailscale_hostname, port);
                staging_deployments.insert(sha.clone(), StagingDeployment { port, process });
                DeployComment::success(preview_url, logs, sha.as_str().to_string())
            }
            Err(logs) => {
                warn!(sha = %sha, "staging deployment failed");
                port_allocator.release(port);
                DeployComment::failure(logs, sha.as_str().to_string())
            }
        };

        if let Err(e) = installation
            .upsert_deploy_comment(pr_number, &comment)
            .await
        {
            warn!(error = ?e, pr = pr_number, "failed to post deploy comment");
        }
    }

    Ok(())
}

async fn cleanup_dead_staging(
    targets: &[DeployTarget],
    staging_deployments: &mut HashMap<Sha, StagingDeployment>,
    port_allocator: &mut PortAllocator,
) {
    let target_shas: std::collections::HashSet<_> = targets.iter().map(|t| &t.sha).collect();
    for sha in staging_deployments
        .keys()
        .filter(|sha| !target_shas.contains(sha))
        .cloned()
        .collect::<Vec<_>>()
    {
        if let Some(mut deployment) = staging_deployments.remove(&sha) {
            kill_process(&mut deployment.process).await;
            port_allocator.release(deployment.port);
        }
    }
}

async fn poll_production(
    installation: &Installation,
    app_client: &Octocrab,
    bare_repo: &Path,
    data_dir: &Path,
    production_deployment: &mut Option<ProductionDeployment>,
    config: &ProductionConfig,
    env_config: &EnvConfig,
) -> anyhow::Result<()> {
    let branch_sha = installation
        .fetch_branch_sha(&config.branch)
        .await
        .context("failed to fetch branch sha")?;

    let needs_deploy = production_deployment
        .as_ref()
        .map(|d| d.sha != branch_sha)
        .unwrap_or(true);

    if !needs_deploy {
        return Ok(());
    }

    info!(
        branch = %config.branch,
        sha = %branch_sha,
        "production branch updated, deploying"
    );

    let token = installation
        .ensure_token(app_client)
        .await
        .context("failed to get installation token")?;
    let fetch_url = format!(
        "https://x-access-token:{}@github.com/{}.git",
        secrecy::ExposeSecret::expose_secret(&token),
        installation.repo
    );

    let checkout_dir = data_dir.join("checkouts").join(branch_sha.as_str());
    git::extract_commit(bare_repo, &fetch_url, branch_sha.as_str(), &checkout_dir)
        .await
        .context("failed to extract production commit")?;

    build_production(&checkout_dir)
        .await
        .context("production build failed")?;

    if let Some(mut old) = production_deployment.take() {
        kill_process(&mut old.process).await;
    }

    let process = run_production(&checkout_dir, config.run_dir, config.port, &config.origin, env_config)
        .await
        .context("production run failed")?;

    info!(
        sha = %branch_sha,
        port = config.port,
        "production deployment succeeded"
    );
    *production_deployment = Some(ProductionDeployment {
        sha: branch_sha,
        process,
    });

    Ok(())
}

async fn initialize_data_dir() -> anyhow::Result<Box<Path>> {
    let data_dir =
        std::path::PathBuf::from(env::var("HAZEL_DATA_DIR").context("HAZEL_DATA_DIR not set")?);

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

    Ok(data_dir.into_boxed_path())
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

fn initialize_poll_interval() -> anyhow::Result<std::time::Duration> {
    let secs: u64 = env::var("HAZEL_POLL_INTERVAL_SECS")
        .context("HAZEL_POLL_INTERVAL_SECS not set")?
        .parse()
        .context("HAZEL_POLL_INTERVAL_SECS must be a number")?;

    Ok(std::time::Duration::from_secs(secs))
}

fn initialize_watched_repo() -> anyhow::Result<Repo> {
    let owner = env::var("HAZEL_WATCHED_REPO_OWNER").context("HAZEL_WATCHED_REPO_OWNER not set")?;
    let name = env::var("HAZEL_WATCHED_REPO_NAME").context("HAZEL_WATCHED_REPO_NAME not set")?;

    Ok(Repo::new(owner, name))
}

fn initialize_production_config() -> anyhow::Result<Option<ProductionConfig>> {
    let enabled = env::var("HAZEL_PRODUCTION_ENABLE").ok().as_deref() == Some("true");
    if !enabled {
        return Ok(None);
    }

    let branch = env::var("HAZEL_PRODUCTION_BRANCH").unwrap_or_else(|_| "main".into());
    let port: u16 = env::var("HAZEL_PRODUCTION_PORT")
        .context("HAZEL_PRODUCTION_PORT required when production enabled")?
        .parse()
        .context("HAZEL_PRODUCTION_PORT must be a number")?;
    let origin = env::var("HAZEL_PRODUCTION_ORIGIN")
        .context("HAZEL_PRODUCTION_ORIGIN required when production enabled")?;
    let run_dir: &'static Path = Box::leak(
        std::path::PathBuf::from(
            env::var("HAZEL_PRODUCTION_RUN_DIR")
                .context("HAZEL_PRODUCTION_RUN_DIR required when production enabled")?,
        )
        .into_boxed_path(),
    );

    Ok(Some(ProductionConfig {
        branch,
        port,
        origin,
        run_dir,
    }))
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
