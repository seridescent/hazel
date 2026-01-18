use std::path::Path;
use tokio::process::Child;

use crate::Sha;
use crate::deploy::{build_derivation, kill_process, run_deployment};

pub struct StagingDeployment {
    pub sha: Sha,
    pub port: u16,
    pub process: Child,
}

pub async fn kill_staging(deployment: &mut StagingDeployment) {
    kill_process(&mut deployment.process).await;
}

pub async fn deploy_staging(
    sha: &Sha,
    checkout_dir: &Path,
    run_dir: &Path,
    port: u16,
    tailscale_hostname: &str,
) -> anyhow::Result<StagingDeployment> {
    build_derivation(checkout_dir, "hazel-preStart").await?;
    build_derivation(checkout_dir, "hazel-executable").await?;

    let process = run_deployment(
        checkout_dir,
        run_dir,
        tailscale_hostname,
        port,
        "hazel-preStart",
        "hazel-executable",
    )
    .await?;

    Ok(StagingDeployment {
        sha: sha.clone(),
        port,
        process,
    })
}
