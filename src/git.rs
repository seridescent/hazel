use anyhow::{Context, bail};
use std::path::{Path, PathBuf};
use tokio::process::Command;
use tracing::{debug, info};

/// Ensures the bare repo exists.
/// Creates repo_dir/repo.git if it doesn't exist.
/// Returns path to repo.git.
pub async fn ensure_bare_repo(repo_dir: &Path) -> anyhow::Result<PathBuf> {
    let bare_repo = repo_dir.join("repo.git");

    if !bare_repo.exists() {
        info!(path = %bare_repo.display(), "initializing bare repo");
        tokio::fs::create_dir_all(&bare_repo).await?;

        let output = Command::new("git")
            .arg("-C")
            .arg(&bare_repo)
            .args(["init", "--bare"])
            .output()
            .await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("git init --bare failed: {stderr}");
        }
    }

    Ok(bare_repo)
}

/// Extracts a commit to a directory using git archive.
/// Fetches the commit from the provided URL, then extracts if the directory doesn't exist.
/// Since directories are SHA-based, an existing directory is already correct.
pub async fn extract_commit(
    bare_repo: &Path,
    fetch_url: &str,
    sha: &str,
    dest: &Path,
) -> anyhow::Result<()> {
    // Fetch the commit from URL (no origin dependency)
    let output = Command::new("git")
        .arg("-C")
        .arg(bare_repo)
        .args(["fetch", fetch_url, sha])
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("git fetch {sha} failed: {stderr}");
    }

    // SHA-based directory: if it exists, it's already correct
    if dest.exists() {
        debug!(path = %dest.display(), sha = %sha, "already extracted");
        return Ok(());
    }

    info!(path = %dest.display(), sha = %sha, "extracting commit");
    tokio::fs::create_dir_all(dest).await?;

    // git archive <sha> | tar -xf - -C <dest>
    let mut git_archive = Command::new("git")
        .arg("-C")
        .arg(bare_repo)
        .args(["archive", sha])
        .stdout(std::process::Stdio::piped())
        .spawn()
        .context("failed to spawn git archive")?;

    let git_stdout = git_archive
        .stdout
        .take()
        .context("failed to get git archive stdout")?
        .into_owned_fd()
        .context("failed to convert stdout to owned fd")?;

    let tar_output = Command::new("tar")
        .args(["-xf", "-", "-C"])
        .arg(dest)
        .stdin(git_stdout)
        .output()
        .await?;

    let git_status = git_archive.wait().await?;

    if !git_status.success() {
        bail!("git archive {sha} failed");
    }

    if !tar_output.status.success() {
        let stderr = String::from_utf8_lossy(&tar_output.stderr);
        bail!("tar extract failed: {stderr}");
    }

    Ok(())
}
