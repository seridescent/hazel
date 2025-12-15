use anyhow::Context;
use hazel::{
    Repo, Sha,
    deploy::{Deployment, clear_tailscale_serve, deploy_sha, kill_deployment},
    git,
    installation::Installation,
    port_allocator::PortAllocator,
};
use octocrab::{Octocrab, models::AppId};
use std::{collections::HashMap, env, path::PathBuf, time::Duration};
use tokio::task::JoinSet;
use tracing::{info, warn};

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
    let tailscale_proxy_port = initialize_tailscale_proxy_port()?;
    let poll_interval = initialize_poll_interval()?;

    clear_tailscale_serve(tailscale_proxy_port).await?;

    // intentionally just watching one repository because YAGNI,
    // but it wouldn't be hard to generalize this to just query for
    // repositories where the app is installed.
    let repo = initialize_watched_repo()?;
    let installation = initialize_installation(&app_client, repo.clone()).await?;

    let repo_dir = data_dir.join("repos").join(repo.to_string());
    let bare_repo = git::ensure_bare_repo(&repo_dir).await?;

    let mut deployments: HashMap<Sha, Deployment> = HashMap::new();

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
            .filter(|(sha, _)| !deployments.contains_key(sha))
            .collect();

        if !new_targets.is_empty() {
            info!(count = new_targets.len(), "deploying new targets");

            let mut set = JoinSet::new();
            for (sha, fetch_url) in new_targets {
                let port = port_allocator.allocate()?;
                let bare_repo = bare_repo.clone();
                let data_dir = data_dir.clone();
                let sha = sha.clone();
                let fetch_url = fetch_url.clone();

                set.spawn(async move {
                    let checkout_dir = data_dir.join("checkouts").join(sha.as_str());
                    git::extract_commit(
                        &bare_repo,
                        fetch_url.as_str(),
                        sha.as_str(),
                        &checkout_dir,
                    )
                    .await?;

                    let run_dir = data_dir.join("deploys").join(sha.as_str());
                    deploy_sha(&sha, &checkout_dir, &run_dir, port, tailscale_proxy_port).await
                });
            }

            while let Some(result) = set.join_next().await {
                match result {
                    Ok(Ok(deployment)) => {
                        info!(sha = %deployment.sha, port = deployment.port, "deployment succeeded");
                        deployments.insert(deployment.sha.clone(), deployment);
                    }
                    Ok(Err(e)) => {
                        warn!(error = ?e, "deployment failed");
                    }
                    Err(e) => {
                        warn!(error = ?e, "task panicked");
                    }
                }
            }
        }

        let target_shas: std::collections::HashSet<_> =
            targets.iter().map(|(sha, _)| sha).collect();
        for sha in deployments
            .keys()
            .filter(|sha| !target_shas.contains(sha))
            .cloned()
            .collect::<Vec<_>>()
        {
            if let Some(mut deployment) = deployments.remove(&sha) {
                kill_deployment(tailscale_proxy_port, &mut deployment).await;
                port_allocator.release(deployment.port);
            }
        }

        info!(
            active = deployments.len(),
            targets = targets.len(),
            "poll complete"
        );

        tokio::select! {
            _ = tokio::time::sleep(poll_interval) => {}
            _ = tokio::signal::ctrl_c() => break,
        }
    }

    info!("shutting down");
    for (_, mut deployment) in deployments {
        kill_deployment(tailscale_proxy_port, &mut deployment).await;
    }

    // Clean up checkouts and deploys (keep repos for git cache)
    if let Err(e) = tokio::fs::remove_dir_all(data_dir.join("checkouts")).await {
        warn!(error = ?e, "failed to clean checkouts");
    }
    if let Err(e) = tokio::fs::remove_dir_all(data_dir.join("deploys")).await {
        warn!(error = ?e, "failed to clean deploys");
    }

    info!("cleanup complete");
    Ok(())
}

async fn initialize_data_dir() -> anyhow::Result<PathBuf> {
    let data_dir = PathBuf::from(env::var("HAZEL_DATA_DIR").context("HAZEL_DATA_DIR not set")?);

    // Clean up stale checkouts/deploys from previous runs (ignore errors if they don't exist)
    let _ = tokio::fs::remove_dir_all(data_dir.join("checkouts")).await;
    let _ = tokio::fs::remove_dir_all(data_dir.join("deploys")).await;

    tokio::try_join!(
        tokio::fs::create_dir_all(data_dir.join("repos")),
        tokio::fs::create_dir_all(data_dir.join("checkouts")),
        tokio::fs::create_dir_all(data_dir.join("deploys")),
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

fn initialize_tailscale_proxy_port() -> anyhow::Result<u16> {
    env::var("HAZEL_TAILSCALE_PROXY_PORT")
        .context("HAZEL_TAILSCALE_PROXY_PORT not set")?
        .parse()
        .context("HAZEL_TAILSCALE_PROXY_PORT must be a number")
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
