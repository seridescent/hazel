use anyhow::{Context, bail};
use std::path::{Path, PathBuf};
use tokio::process::Command;

/// Ensures the bare repo exists and is up-to-date.
/// Creates repo_dir/repo.git if it doesn't exist, otherwise fetches.
/// Returns path to repo.git.
pub async fn sync_repo(repo_dir: &Path, clone_url: &str) -> anyhow::Result<PathBuf> {
    let bare_repo = repo_dir.join("repo.git");

    if !bare_repo.exists() {
        println!("cloning bare repo to {bare_repo:?}");
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
    } else {
        println!("fetching latest into {bare_repo:?}");

        let output = Command::new("git")
            .arg("-C")
            .arg(&bare_repo)
            .args(["fetch", "--all"])
            .output()
            .await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("git fetch failed: {stderr}");
        }
    }

    Ok(bare_repo)
}

/// Ensures worktree exists at the given path and is at the correct SHA.
/// Creates worktree if it doesn't exist, otherwise checks out the SHA.
pub async fn sync_worktree(
    bare_repo: &Path,
    worktree_dir: &Path,
    head_sha: &str,
) -> anyhow::Result<()> {
    let worktrees_dir = worktree_dir
        .parent()
        .context("worktree_dir has no parent")?;

    if !worktree_dir.exists() {
        println!("creating worktree at {worktree_dir:?}");
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
        // TODO: check if already at correct SHA before checkout
        println!("checking out {head_sha} in {worktree_dir:?}");

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
