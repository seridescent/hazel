use anyhow::Context;
use hazel::{Repo, git};
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

    let (data_dir, _) = tokio::try_join!(init_data_dir(), initialize_app_client())?;

    // TODO: don't capture output of commands, do something more sophisticated

    // PROTOTYPE CODE BELOW

    let repo = Repo::new("seridescent", "hazel-test-repo");

    let app_client = octocrab::instance();

    let installation = app_client
        .apps()
        .get_repository_installation(&repo.owner, &repo.name)
        .await?;

    let (installation_client, installation_token) =
        app_client.installation_and_token(installation.id).await?;

    let repo_dir = data_dir.join("repos").join(repo.to_string());
    let clone_url = format!(
        "https://x-access-token:{}@github.com/{}.git",
        installation_token.expose_secret(),
        repo,
    );

    let bare_repo = git::ensure_repo(&repo_dir, &clone_url).await?;

    let open_pulls = installation_client
        .pulls(&repo.owner, &repo.name)
        .list()
        .state(octocrab::params::State::Open)
        .send()
        .await?;

    for pull in &open_pulls.items {
        let head_sha = &pull.head.sha;
        let pr_number = pull.number;

        info!(pr = pr_number, sha = %head_sha, "processing PR");

        let worktree_dir = repo_dir.join("worktrees").join(format!("pr-{pr_number}"));
        git::sync_worktree(&bare_repo, &worktree_dir, head_sha).await?;

        info!(pr = pr_number, path = %worktree_dir.display(), "PR ready");
    }

    Ok(())
}

async fn init_data_dir() -> anyhow::Result<PathBuf> {
    let data_dir = PathBuf::from(env::var("HAZEL_DATA_DIR").context("HAZEL_DATA_DIR not set")?);

    tokio::try_join!(
        tokio::fs::create_dir_all(data_dir.join("repos")),
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
