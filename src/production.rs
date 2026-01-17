use std::path::{Path, PathBuf};
use tokio::process::Child;
use tracing::info;

use crate::Sha;
use crate::deploy::{DeployConfig, deploy, kill_process};

pub struct ProductionDeployment {
    pub sha: Sha,
    pub process: Child,
    pub executable_store_path: PathBuf,
}

/// Kills a production deployment's process.
pub async fn kill_production(deployment: &mut ProductionDeployment) {
    info!(sha = %deployment.sha, "killing production deployment");
    kill_process(&mut deployment.process).await;
}

/// Deploys production for a given SHA. Returns a ProductionDeployment on success.
pub async fn deploy_production(
    sha: &Sha,
    checkout_dir: &Path,
    run_dir: &Path,
    port: u16,
    tailscale_hostname: &str,
) -> anyhow::Result<ProductionDeployment> {
    info!(sha = %sha, port = port, "starting production deployment");

    let origin = format!("http://{}:{}", tailscale_hostname, port);

    let result = deploy(DeployConfig {
        checkout_dir,
        run_dir,
        port,
        origin: &origin,
        pre_start_attr: Some("hazel-production-preStart"),
        executable_attr: "hazel-production-executable",
    })
    .await?;

    info!(sha = %sha, port = port, pid = ?result.process.id(), "production deployment started");

    Ok(ProductionDeployment {
        sha: sha.clone(),
        process: result.process,
        executable_store_path: result.executable_store_path,
    })
}
