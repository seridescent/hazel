use anyhow::Context;
use hazel::{Repo, deploy::DeploymentManager, git};
use octocrab::{Octocrab, models::AppId};
use secrecy::ExposeSecret;
use std::{env, path::PathBuf};
use tracing::info;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .init();

    let (data_dir, _) = tokio::try_join!(initialize_data_dir(), initialize_app_client())?;

    let mut manager = initialize_deployment_manager()?;

    // PROTOTYPE CODE BELOW

    let repo = Repo::new("seridescent", "hazel-test-repo");

    let app_client = octocrab::instance();

    let installation = app_client
        .apps()
        .get_repository_installation(&repo.owner, &repo.name)
        .await?;

    // TODO: handle installation token expiry
    //  installation tokens expire after an hour. fetch_url is built fresh each prototype run,
    //  but for a long-running service we need to refresh the token periodically.
    //  there is a function implementing this logic, but it's not public for some reason.
    //  meanwhile, there isn't an ergonomic way (AFAICT) to set the required auth headers and
    //  construct the request myself. one would expect the installation client to do this, but it doesn't.
    let (installation_client, installation_token) =
        app_client.installation_and_token(installation.id).await?;

    let repo_dir = data_dir.join("repos").join(repo.to_string());
    let bare_repo = git::ensure_bare_repo(&repo_dir).await?;

    let fetch_url = format!(
        "https://x-access-token:{}@github.com/{}.git",
        installation_token.expose_secret(),
        repo,
    );

    let open_pulls = installation_client
        .pulls(&repo.owner, &repo.name)
        .list()
        .state(octocrab::params::State::Open)
        .send()
        .await?;

    // Create checkouts and start deployments for each PR
    for pull in &open_pulls.items {
        let sha = &pull.head.sha;
        info!(pr = pull.number, sha = %sha, "processing PR");

        let checkout_dir = data_dir.join("checkouts").join(sha);
        git::extract_commit(&bare_repo, &fetch_url, sha, &checkout_dir).await?;

        let run_dir = data_dir.join("deploys").join(sha);
        manager.start(sha, &checkout_dir, &run_dir).await?;
    }

    info!(count = manager.deployments.len(), "all deployments started");

    // Keep the process alive while deployments run
    // TODO: proper signal handling, webhook server, etc.
    tokio::signal::ctrl_c().await?;

    info!("shutting down");
    manager.kill_all().await;

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

async fn initialize_app_client() -> anyhow::Result<()> {
    let app_id: u64 = env::var("GITHUB_APP_ID")
        .context("GITHUB_APP_ID not set")?
        .parse()
        .context("GITHUB_APP_ID must be a number")?;
    let key_path = env::var("GITHUB_APP_KEY_PATH").context("GITHUB_APP_KEY_PATH not set")?;
    let key = tokio::fs::read_to_string(&key_path)
        .await
        .with_context(|| format!("failed to read key from {key_path}"))?;
    let key = jsonwebtoken::EncodingKey::from_rsa_pem(key.as_bytes())?;

    octocrab::initialise(
        Octocrab::builder()
            .app(AppId(app_id), key)
            .build()
            .context("app client failed to build")?,
    );

    Ok(())
}

fn initialize_deployment_manager() -> anyhow::Result<DeploymentManager> {
    let port_min: u16 = env::var("HAZEL_PORT_MIN")
        .context("HAZEL_PORT_MIN not set")?
        .parse()
        .context("HAZEL_PORT_MIN must be a number")?;
    let port_max: u16 = env::var("HAZEL_PORT_MAX")
        .context("HAZEL_PORT_MAX not set")?
        .parse()
        .context("HAZEL_PORT_MAX must be a number")?;

    Ok(DeploymentManager::new(port_min, port_max))
}
