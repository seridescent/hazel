use anyhow::{Context, anyhow};
use octocrab::{Octocrab, models::AppId};
use std::{env, path::PathBuf};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let (data_dir, _) = tokio::try_join!(init_data_dir(), initialize_app_client())?;

    // PROTOTYPE CODE BELOW, LEAVE ALONE

    let app_client = octocrab::instance();

    let installation = app_client
        .apps()
        .get_repository_installation("seridescent", "hazel-test-repo")
        .await?;

    // authenticate as installation
    let (octocrab, _installation_token) =
        app_client.installation_and_token(installation.id).await?;

    let open_pulls = octocrab
        .pulls("seridescent", "hazel-test-repo")
        .list()
        .state(octocrab::params::State::Open)
        .send()
        .await?;

    let test_pull = open_pulls
        .items
        .get(0)
        .ok_or(anyhow!("test pull missing"))?;

    println!(
        "{:?} {:?}",
        test_pull.number,
        test_pull
            .title
            .clone()
            .ok_or(anyhow!("test pull missing title"))?
    );
    Ok(())
}

async fn init_data_dir() -> anyhow::Result<PathBuf> {
    let data_dir = PathBuf::from(env::var("HAZEL_DATA_DIR").context("HAZEL_DATA_DIR not set")?);

    tokio::try_join!(
        tokio::fs::create_dir_all(data_dir.join("repos")),
        tokio::fs::create_dir_all(data_dir.join("deploys")),
    )
    .context("failed to create directories")?;

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
