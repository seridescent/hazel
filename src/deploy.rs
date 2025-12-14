use anyhow::{Context, bail};
use std::path::Path;
use tokio::process::{Child, Command};
use tracing::{info, warn};

use crate::Sha;

pub struct Deployment {
    pub sha: Sha,
    pub port: u16,
    pub process: Child,
}

/// Kills a deployment's process and removes its tailscale serve route.
pub async fn kill_deployment(tailscale_proxy_port: u16, deployment: &mut Deployment) {
    info!(sha = %deployment.sha, port = deployment.port, "killing deployment");

    // if the executable produces a child process, this will not clean them all up
    // correctly. that's expected for now. executable derivations should end in `exec`
    // if they are scripts
    if let Err(e) = deployment.process.kill().await {
        warn!(sha = %deployment.sha, error = ?e, "failed to kill deployment process");
    }

    if let Err(e) = deployment.process.wait().await {
        warn!(sha = %deployment.sha, error = ?e, "failed to wait for deployment process");
    }

    let status = Command::new("tailscale")
        .args([
            "serve",
            "--yes",
            &format!("--http={}", tailscale_proxy_port),
            &format!("--set-path=/{}", deployment.sha),
            "off",
        ])
        .status()
        .await;

    match status {
        Ok(s) if s.success() => {
            info!(sha = %deployment.sha, "tailscale serve route removed");
        }
        Ok(s) => {
            warn!(sha = %deployment.sha, code = ?s.code(), "tailscale serve off failed");
        }
        Err(e) => {
            warn!(sha = %deployment.sha, error = ?e, "failed to run tailscale serve off");
        }
    }
}

/// Clears any existing tailscale serve handlers on the given port.
pub async fn clear_tailscale_serve(port: u16) -> anyhow::Result<()> {
    let output = Command::new("tailscale")
        .args(["serve", "status", "--json"])
        .output()
        .await
        .context("failed to get tailscale serve status")?;

    if !output.status.success() {
        bail!("tailscale serve status failed");
    }

    // Check if our port has any TCP handlers
    let status: serde_json::Value =
        serde_json::from_slice(&output.stdout).context("failed to parse tailscale serve status")?;

    if status
        .get("TCP")
        .and_then(|tcp| tcp.get(port.to_string()))
        .is_some()
    {
        info!(port = port, "clearing existing tailscale serve handlers");

        let off_status = Command::new("tailscale")
            .args(["serve", "--yes", &format!("--http={}", port), "off"])
            .status()
            .await
            .context("failed to run tailscale serve off")?;

        if !off_status.success() {
            bail!("tailscale serve off failed");
        }
    }

    Ok(())
}

/// Deploys a single SHA. Returns a Deployment on success.
pub async fn deploy_sha(
    sha: &Sha,
    checkout_dir: &Path,
    run_dir: &Path,
    port: u16,
    tailscale_proxy_port: u16,
) -> anyhow::Result<Deployment> {
    tokio::fs::create_dir_all(run_dir).await?;

    info!(sha = %sha, port = port, "starting deployment");

    let pre_start_status = Command::new("nix")
        .args(["run", &format!("{}#hazel-preStart", checkout_dir.display())])
        .env("HAZEL_RUN_DIR", run_dir)
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
        .current_dir(run_dir)
        .spawn()
        .context("failed to spawn executable")?;

    info!(sha = %sha, port = port, pid = ?process.id(), "deployment started");

    let serve_status = Command::new("tailscale")
        .args([
            "serve",
            "--bg",
            &format!("--http={}", tailscale_proxy_port),
            &format!("--set-path=/{sha}"),
            &format!("localhost:{port}"),
        ])
        .status()
        .await
        .context("failed to run tailscale serve")?;

    if !serve_status.success() {
        bail!("tailscale serve failed for {sha}");
    }

    info!(sha = %sha, "tailscale serve route added");

    Ok(Deployment {
        sha: sha.clone(),
        port,
        process,
    })
}
