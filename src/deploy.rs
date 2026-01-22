use anyhow::{Context, bail};
use std::path::Path;
use tokio::process::{Child, Command};
use tracing::warn;

/// Builds a nix derivation and returns its store path.
pub async fn build_derivation(checkout_dir: &Path, attr: &str) -> anyhow::Result<()> {
    let flake_ref = format!("{}#{}", checkout_dir.display(), attr);

    let output = Command::new("nix")
        .args(["build", "--no-link", "--print-out-paths", &flake_ref])
        .output()
        .await
        .context("failed to run nix build")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("nix build failed for {}: {}", flake_ref, stderr);
    }

    Ok(())
}

/// Runs a deployment: executes preStart, then spawns the executable.
pub async fn run_deployment(
    checkout_dir: &Path,
    run_dir: &Path,
    port: u16,
    origin: &str,
    pre_start_attr: &str,
    executable_attr: &str,
) -> anyhow::Result<Child> {
    tokio::fs::create_dir_all(run_dir).await?;

    let env_vars = [
        ("HAZEL_PORT", port.to_string()),
        ("HAZEL_RUN_DIR", run_dir.display().to_string()),
        ("HAZEL_ORIGIN", origin.to_string()),
    ];

    // Run preStart and wait for completion
    let status = Command::new("nix")
        .args([
            "run",
            &format!("{}#{}", checkout_dir.display(), pre_start_attr),
        ])
        .current_dir(run_dir)
        .envs(env_vars.clone())
        .status()
        .await
        .context("failed to run preStart")?;

    if !status.success() {
        bail!("preStart failed");
    }

    // Spawn executable
    let child = Command::new("nix")
        .args([
            "run",
            &format!("{}#{}", checkout_dir.display(), executable_attr),
        ])
        .current_dir(run_dir)
        .envs(env_vars)
        .spawn()
        .context("failed to spawn executable")?;

    Ok(child)
}

/// Kills a process gracefully.
pub async fn kill_process(process: &mut Child) {
    if let Err(e) = process.kill().await {
        warn!(error = ?e, "failed to kill process");
    }

    if let Err(e) = process.wait().await {
        warn!(error = ?e, "failed to wait for process");
    }
}
