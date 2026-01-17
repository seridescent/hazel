use anyhow::{Context, bail};
use std::path::{Path, PathBuf};
use tokio::process::{Child, Command};
use tracing::{info, warn};

/// Configuration for a deployment.
pub struct DeployConfig<'a> {
    pub checkout_dir: &'a Path,
    pub run_dir: &'a Path,
    pub port: u16,
    pub origin: &'a str,
    pub pre_start_attr: Option<&'a str>,
    pub executable_attr: &'a str,
}

/// Result of a successful deployment.
pub struct DeployResult {
    pub process: Child,
    pub executable_store_path: PathBuf,
    pub pre_start_store_path: Option<PathBuf>,
}

/// Builds a nix derivation and returns its store path.
pub async fn build_derivation(checkout_dir: &Path, attr: &str) -> anyhow::Result<PathBuf> {
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

    let store_path = String::from_utf8(output.stdout)
        .context("nix build output not valid UTF-8")?
        .trim()
        .to_string();

    info!(attr = attr, store_path = %store_path, "built derivation");

    Ok(PathBuf::from(store_path))
}

/// Deploys an application using the provided configuration.
pub async fn deploy(config: DeployConfig<'_>) -> anyhow::Result<DeployResult> {
    tokio::fs::create_dir_all(config.run_dir).await?;

    info!(
        port = config.port,
        run_dir = %config.run_dir.display(),
        "starting deployment"
    );

    // Build and run preStart if provided
    let pre_start_store_path = if let Some(pre_start_attr) = config.pre_start_attr {
        let store_path = build_derivation(config.checkout_dir, pre_start_attr).await?;

        let bin_path = store_path.join("bin").join(pre_start_attr);
        let status = Command::new(&bin_path)
            .env("HAZEL_RUN_DIR", config.run_dir)
            .env("HAZEL_ORIGIN", config.origin)
            .current_dir(config.run_dir)
            .status()
            .await
            .context("failed to run preStart")?;

        if !status.success() {
            bail!("preStart failed");
        }

        Some(store_path)
    } else {
        None
    };

    // Build and spawn executable
    let executable_store_path = build_derivation(config.checkout_dir, config.executable_attr).await?;

    let bin_path = executable_store_path
        .join("bin")
        .join(config.executable_attr);

    let process = Command::new(&bin_path)
        .env("HAZEL_PORT", config.port.to_string())
        .env("HAZEL_RUN_DIR", config.run_dir)
        .env("HAZEL_ORIGIN", config.origin)
        .current_dir(config.run_dir)
        .spawn()
        .context("failed to spawn executable")?;

    info!(
        port = config.port,
        pid = ?process.id(),
        store_path = %executable_store_path.display(),
        "deployment started"
    );

    Ok(DeployResult {
        process,
        executable_store_path,
        pre_start_store_path,
    })
}

/// Kills a deployment process gracefully.
pub async fn kill_process(process: &mut Child) {
    if let Err(e) = process.kill().await {
        warn!(error = ?e, "failed to kill process");
    }

    if let Err(e) = process.wait().await {
        warn!(error = ?e, "failed to wait for process");
    }
}
