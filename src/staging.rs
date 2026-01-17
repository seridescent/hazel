use anyhow::{bail, Context};
use std::path::Path;
use tokio::process::{Child, Command};
use tracing::{info, warn};

use crate::Sha;

pub struct StagingDeployment {
    pub sha: Sha,
    pub port: u16,
    pub process: Child,
}

/// Kills a staging deployment's process.
pub async fn kill_staging(deployment: &mut StagingDeployment) {
    info!(sha = %deployment.sha, port = deployment.port, "killing staging deployment");

    // if the executable produces a child process, this will not clean them all up
    // correctly. that's expected for now. executable derivations should end in `exec`
    // if they are scripts
    if let Err(e) = deployment.process.kill().await {
        warn!(sha = %deployment.sha, error = ?e, "failed to kill staging process");
    }

    if let Err(e) = deployment.process.wait().await {
        warn!(sha = %deployment.sha, error = ?e, "failed to wait for staging process");
    }
}

/// Deploys a staging preview for a single SHA. Returns a StagingDeployment on success.
pub async fn deploy_staging(
    sha: &Sha,
    checkout_dir: &Path,
    run_dir: &Path,
    port: u16,
    tailscale_hostname: &str,
) -> anyhow::Result<StagingDeployment> {
    tokio::fs::create_dir_all(run_dir).await?;

    info!(sha = %sha, port = port, "starting staging deployment");

    // Direct MagicDNS URL - no reverse proxy needed
    let origin = format!("http://{}:{}", tailscale_hostname, port);

    let pre_start_status = Command::new("nix")
        .args(["run", &format!("{}#hazel-preStart", checkout_dir.display())])
        .env("HAZEL_RUN_DIR", run_dir)
        .env("HAZEL_ORIGIN", &origin)
        .current_dir(run_dir)
        .status()
        .await
        .context("failed to run preStart")?;

    if !pre_start_status.success() {
        bail!("preStart failed for {sha}");
    }

    // TODO: handle spawned process exiting with error in unhappy case?
    //  happy path: process lives until killed by us, but
    //  should probably do something about it exiting unexpectedly

    let process = Command::new("nix")
        .args([
            "run",
            &format!("{}#hazel-executable", checkout_dir.display()),
        ])
        .env("HAZEL_PORT", port.to_string())
        .env("HAZEL_RUN_DIR", run_dir)
        .env("HAZEL_ORIGIN", &origin)
        .current_dir(run_dir)
        .spawn()
        .context("failed to spawn executable")?;

    info!(sha = %sha, port = port, pid = ?process.id(), "staging deployment started");

    Ok(StagingDeployment {
        sha: sha.clone(),
        port,
        process,
    })
}
