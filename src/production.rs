use std::path::Path;
use tokio::process::Child;

use crate::Sha;
use crate::deploy::{BuildLogs, build_derivation, kill_process, run_deployment};

pub struct ProductionDeployment {
    pub sha: Sha,
    pub process: Child,
}

pub async fn kill_production(deployment: &mut ProductionDeployment) {
    kill_process(&mut deployment.process).await;
}

/// Builds production derivations. Call before killing old deployment to minimize downtime.
pub async fn build_production(checkout_dir: &Path) -> anyhow::Result<()> {
    build_derivation(checkout_dir, "hazel-production-preStart")
        .await
        .map_err(|e| anyhow::anyhow!("hazel-production-preStart build failed: {}", e.stderr))?;

    build_derivation(checkout_dir, "hazel-production-executable")
        .await
        .map_err(|e| anyhow::anyhow!("hazel-production-executable build failed: {}", e.stderr))?;

    Ok(())
}

/// Runs production deployment. Assumes derivations are already built.
pub async fn run_production(
    sha: &Sha,
    checkout_dir: &Path,
    run_dir: &Path,
    port: u16,
    origin: &str,
) -> anyhow::Result<ProductionDeployment> {
    // Production doesn't need to track logs for PR comments
    let logs = BuildLogs::default();

    let (process, _logs) = run_deployment(
        checkout_dir,
        run_dir,
        port,
        origin,
        "hazel-production-preStart",
        "hazel-production-executable",
        logs,
    )
    .await
    .map_err(|logs| {
        let msg = logs
            .pre_start_run
            .map(|r| match r {
                Ok(o) | Err(o) => o.stderr,
            })
            .unwrap_or_else(|| "unknown error".to_string());
        anyhow::anyhow!("production deployment failed: {}", msg)
    })?;

    Ok(ProductionDeployment {
        sha: sha.clone(),
        process,
    })
}
