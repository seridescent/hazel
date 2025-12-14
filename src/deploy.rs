use anyhow::{Context, bail};
use std::path::Path;
use tokio::process::{Child, Command};
use tracing::info;

use crate::Sha;

pub struct Deployment {
    pub sha: Sha,
    pub port: u16,
    pub process: Child,
    pub serve: Child,
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

    // TODO: provide a host env var (e.g. a <device-name>.tailnet-id.ts.net)
    // so services can allow-list it during staging.

    // TODO: this can fail for lack of permission to do something like cp over an existing file
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

    // TODO: tailscale serve and the actual server process are independent,
    // so they can be started together

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

    let serve = Command::new("tailscale")
        .args([
            "serve",
            &format!("--http={}", tailscale_proxy_port),
            &format!("--set-path=/{sha}"),
            &format!("localhost:{port}"),
        ])
        .current_dir(run_dir)
        .spawn()
        .context("failed to serve via tailscale")?;

    info!(sha = %sha, serve_pid = ?serve.id(), "tailscale serve started");

    Ok(Deployment {
        sha: sha.clone(),
        port,
        process,
        serve,
    })
}
