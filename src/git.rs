use anyhow::{Context, bail};
use std::path::{Path, PathBuf};
use tokio::process::Command;
use tracing::{info, debug};

/// Ensures the bare repo exists.
/// Creates repo_dir/repo.git if it doesn't exist.
/// Returns path to repo.git.
pub async fn ensure_repo(repo_dir: &Path, clone_url: &str) -> anyhow::Result<PathBuf> {
    let bare_repo = repo_dir.join("repo.git");

    if !bare_repo.exists() {
        info!(path = %bare_repo.display(), "cloning bare repo");
        tokio::fs::create_dir_all(repo_dir).await?;

        let output = Command::new("git")
            .args(["clone", "--bare", clone_url])
            .arg(&bare_repo)
            .output()
            .await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("git clone failed: {stderr}");
        }
    }

    Ok(bare_repo)
}

/// Ensures worktree exists at the given path and is at the correct SHA.
/// Fetches the commit first, then creates worktree if it doesn't exist, otherwise checks out the SHA.
pub async fn sync_worktree(
    bare_repo: &Path,
    worktree_dir: &Path,
    head_sha: &str,
) -> anyhow::Result<()> {
    // Fetch the commit to ensure it's available
    let output = Command::new("git")
        .arg("-C")
        .arg(bare_repo)
        .args(["fetch", "origin", head_sha])
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("git fetch {head_sha} failed: {stderr}");
    }

    let worktrees_dir = worktree_dir
        .parent()
        .context("worktree_dir has no parent")?;

    if !worktree_dir.exists() {
        info!(path = %worktree_dir.display(), sha = %head_sha, "creating worktree");
        tokio::fs::create_dir_all(worktrees_dir).await?;

        let output = Command::new("git")
            .arg("-C")
            .arg(bare_repo)
            .args(["worktree", "add"])
            .arg(worktree_dir)
            .arg(head_sha)
            .output()
            .await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("git worktree add failed: {stderr}");
        }
    } else {
        debug!(path = %worktree_dir.display(), sha = %head_sha, "checking out");

        let output = Command::new("git")
            .arg("-C")
            .arg(worktree_dir)
            .args(["checkout", head_sha])
            .output()
            .await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("git checkout failed: {stderr}");
        }
    }

    Ok(())
}
