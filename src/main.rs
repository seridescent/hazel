use anyhow::{Context, anyhow};
use octocrab::{Octocrab, models::AppId};
use std::env;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let app_client = make_app_client().await?;

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

async fn make_app_client() -> anyhow::Result<Octocrab> {
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
