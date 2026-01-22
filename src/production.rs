use std::path::Path;
use tokio::process::Child;

use crate::Sha;
use crate::deploy::{build_derivation, kill_process, run_deployment};

pub struct ProductionDeployment {
    pub sha: Sha,
    pub process: Child,
}

pub async fn kill_production(deployment: &mut ProductionDeployment) {
    kill_process(&mut deployment.process).await;
}

/// Builds production derivations. Call before killing old deployment to minimize downtime.
pub async fn build_production(checkout_dir: &Path) -> anyhow::Result<()> {
    build_derivation(checkout_dir, "hazel-production-preStart").await?;
    build_derivation(checkout_dir, "hazel-production-executable").await
}

/// Runs production deployment. Assumes derivations are already built.
pub async fn run_production(
    sha: &Sha,
    checkout_dir: &Path,
    run_dir: &Path,
    port: u16,
    origin: &str,
) -> anyhow::Result<ProductionDeployment> {
    let process = run_deployment(
        checkout_dir,
        run_dir,
        port,
        origin,
        "hazel-production-preStart",
        "hazel-production-executable",
    )
    .await?;

    Ok(ProductionDeployment {
        sha: sha.clone(),
        process,
    })
}
