use std::path::Path;
use tokio::process::Child;

use crate::Sha;
use crate::deploy::{BuildLogs, build_derivation, kill_process, run_deployment};

pub struct StagingDeployment {
    pub sha: Sha,
    pub port: u16,
    pub process: Child,
}

/// Result type for staging deployments that includes logs in both success and failure cases.
pub type StagingResult = Result<(StagingDeployment, BuildLogs), BuildLogs>;

pub async fn kill_staging(deployment: &mut StagingDeployment) {
    kill_process(&mut deployment.process).await;
}

pub async fn deploy_staging(
    sha: &Sha,
    checkout_dir: &Path,
    run_dir: &Path,
    port: u16,
    tailscale_hostname: &str,
) -> StagingResult {
    let mut logs = BuildLogs::default();

    // Build preStart
    let pre_start_result = build_derivation(checkout_dir, "hazel-preStart").await;
    let pre_start_failed = pre_start_result.is_err();
    logs.pre_start_build = Some(pre_start_result);
    if pre_start_failed {
        return Err(logs);
    }

    // Build executable
    let executable_result = build_derivation(checkout_dir, "hazel-executable").await;
    let executable_failed = executable_result.is_err();
    logs.executable_build = Some(executable_result);
    if executable_failed {
        return Err(logs);
    }

    let origin = format!("http://{}:{}", tailscale_hostname, port);
    let (process, logs) = run_deployment(
        checkout_dir,
        run_dir,
        port,
        &origin,
        "hazel-preStart",
        "hazel-executable",
        logs,
    )
    .await?;

    Ok((
        StagingDeployment {
            sha: sha.clone(),
            port,
            process,
        },
        logs,
    ))
}
