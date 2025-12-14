use anyhow::Context;
use hazel::{
    Repo, deploy::deploy_sha, git, installation::Installation, port_allocator::PortAllocator,
};
use octocrab::{Octocrab, models::AppId};
use std::{collections::HashMap, env, path::PathBuf};
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

    let repo = Repo::new("seridescent", "hazel-test-repo");
    let installation = initialize_installation(&app_client, repo.clone()).await?;

    let repo_dir = data_dir.join("repos").join(repo.to_string());
    let bare_repo = git::ensure_bare_repo(&repo_dir).await?;

    let targets = installation.fetch_deploy_targets(&app_client).await?;
    info!(count = targets.len(), "fetched deploy targets");

    let mut set = JoinSet::new();
    for (sha, fetch_url) in targets {
        let port = port_allocator.allocate()?;
        let bare_repo = bare_repo.clone();
        let data_dir = data_dir.clone();

        set.spawn(async move {
            let checkout_dir = data_dir.join("checkouts").join(sha.as_str());
            git::extract_commit(&bare_repo, fetch_url.as_str(), sha.as_str(), &checkout_dir)
                .await?;

            let run_dir = data_dir.join("deploys").join(sha.as_str());
            deploy_sha(&sha, &checkout_dir, &run_dir, port, tailscale_proxy_port).await
        });
    }

    let mut deployments = HashMap::new();
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

    info!(count = deployments.len(), "all deployments started");

    // TODO: proper signal handling?
    tokio::signal::ctrl_c().await?;

    info!("shutting down");
    for (sha, deployment) in &mut deployments {
        info!(sha = %sha, port = deployment.port, "killing deployment");

        if let Err(e) = deployment.serve.kill().await {
            warn!(sha = %sha, error = ?e, "failed to kill tailscale serve");
        }

        if let Err(e) = deployment.process.kill().await {
            warn!(sha = %sha, error = ?e, "failed to kill deployment");
        }
    }

    Ok(())
}

async fn initialize_data_dir() -> anyhow::Result<PathBuf> {
    let data_dir = PathBuf::from(env::var("HAZEL_DATA_DIR").context("HAZEL_DATA_DIR not set")?);

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
