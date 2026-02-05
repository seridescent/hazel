use std::path::Path;
use tokio::process::{Child, Command};
use tracing::warn;

#[derive(Debug, Clone)]
pub struct BuildOutput {
    pub stdout: String,
    pub stderr: String,
}

pub type BuildResult = Result<BuildOutput, BuildOutput>;

/// Collected logs from all deployment stages.
#[derive(Debug, Clone, Default)]
pub struct BuildLogs {
    pub pre_start_build: Option<BuildResult>,
    pub executable_build: Option<BuildResult>,
    pub pre_start_run: Option<BuildResult>,
}

/// Builds a nix derivation and returns the captured output.
/// Returns Ok(BuildOutput) on success, Err(BuildOutput) on failure.
pub async fn build_derivation(checkout_dir: &Path, attr: &str) -> Result<BuildOutput, BuildOutput> {
    let flake_ref = format!("{}#{}", checkout_dir.display(), attr);

    let output = match Command::new("nix")
        .args([
            "build",
            "--no-link",
            "--print-out-paths",
            "--print-build-logs",
            &flake_ref,
        ])
        .output()
        .await
    {
        Ok(output) => output,
        Err(e) => {
            // TODO: proper discriminated error type for this failure case?
            return Err(BuildOutput {
                stdout: String::new(),
                stderr: format!("failed to run nix build: {}", e),
            });
        }
    };

    let build_output = BuildOutput {
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
    };

    if output.status.success() {
        Ok(build_output)
    } else {
        Err(build_output)
    }
}

/// Runs a deployment: executes preStart, then spawns the executable.
/// Takes pre-captured build logs and returns the child process with complete logs.
/// Extra env vars are applied first, then runtime vars (HAZEL_PORT, etc.) override.
pub async fn run_deployment<'a, I>(
    checkout_dir: &Path,
    run_dir: &Path,
    port: u16,
    origin: &str,
    pre_start_attr: &str,
    executable_attr: &str,
    extra_env: I,
    mut logs: BuildLogs,
) -> Result<(Child, BuildLogs), BuildLogs>
where
    I: Iterator<Item = (&'a str, &'a str)> + Clone,
{
    if let Err(e) = tokio::fs::create_dir_all(run_dir).await {
        // If we can't create run_dir, add a synthetic error to logs
        logs.pre_start_run = Some(Err(BuildOutput {
            stdout: String::new(),
            stderr: format!("failed to create run_dir: {}", e),
        }));
        return Err(logs);
    }

    let env_vars = [
        ("HAZEL_PORT", port.to_string()),
        ("HAZEL_RUN_DIR", run_dir.display().to_string()),
        ("HAZEL_ORIGIN", origin.to_string()),
    ];

    let pre_start_output = Command::new("nix")
        .args([
            "run",
            &format!("{}#{}", checkout_dir.display(), pre_start_attr),
        ])
        .current_dir(run_dir)
        .envs(extra_env.clone())
        .envs(env_vars.clone())
        .output()
        .await;

    match pre_start_output {
        Ok(output) => {
            let build_output = BuildOutput {
                stdout: String::from_utf8_lossy(&output.stdout).to_string(),
                stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            };
            let success = output.status.success();
            logs.pre_start_run = Some(if success {
                Ok(build_output)
            } else {
                Err(build_output)
            });
            if !success {
                return Err(logs);
            }
        }
        Err(e) => {
            logs.pre_start_run = Some(Err(BuildOutput {
                stdout: String::new(),
                stderr: format!("failed to run preStart: {}", e),
            }));
            return Err(logs);
        }
    }

    let child = Command::new("nix")
        .args([
            "run",
            &format!("{}#{}", checkout_dir.display(), executable_attr),
        ])
        .current_dir(run_dir)
        .envs(extra_env)
        .envs(env_vars)
        .spawn();

    match child {
        Ok(child) => Ok((child, logs)),
        Err(e) => {
            // Add error info to logs - using pre_start_run to indicate spawn failure
            // since there's no separate field for executable spawn
            if let Some(Ok(ref mut pre_start) | Err(ref mut pre_start)) = logs.pre_start_run {
                pre_start
                    .stderr
                    .push_str(&format!("\n\nExecutable spawn failed: {}", e));
            }
            Err(logs)
        }
    }
}

pub async fn kill_process(process: &mut Child) {
    if let Err(e) = process.kill().await {
        warn!(error = ?e, "failed to kill process");
    }

    if let Err(e) = process.wait().await {
        warn!(error = ?e, "failed to wait for process");
    }
}
