use anyhow::{Context, bail};
use std::collections::HashMap;
use std::path::Path;
use tokio::process::{Child, Command};
use tracing::{info, warn};

pub struct Deployment {
    pub sha: String,
    pub port: u16,
    pub process: Child,
}

pub struct DeploymentManager {
    port_min: u16,
    port_max: u16,
    next_port: u16,
    // TODO: add reclaimed_ports: BinaryHeap<Reverse<u16>> for port reuse
    pub deployments: HashMap<String, Deployment>,
}

impl DeploymentManager {
    pub fn new(port_min: u16, port_max: u16) -> Self {
        Self {
            port_min,
            port_max,
            next_port: port_min,
            deployments: HashMap::new(),
        }
    }

    fn allocate_port(&mut self) -> anyhow::Result<u16> {
        // TODO: check reclaimed_ports heap first
        if self.next_port > self.port_max {
            bail!("port range exhausted ({}-{})", self.port_min, self.port_max);
        }
        let port = self.next_port;
        self.next_port += 1;
        Ok(port)
    }

    /// Starts a deployment for the given SHA.
    /// Runs preStart then spawns the executable, tracking the process handle.
    pub async fn start(
        &mut self,
        sha: &str,
        checkout_dir: &Path,
        run_dir: &Path,
    ) -> anyhow::Result<()> {
        let port = self.allocate_port()?;

        tokio::fs::create_dir_all(run_dir).await?;

        info!(sha = %sha, port = port, "starting deployment");

        // Run preStart script
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

        // Spawn the executable (keep handle)
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

        self.deployments.insert(
            sha.to_string(),
            Deployment {
                sha: sha.to_string(),
                port,
                process,
            },
        );

        Ok(())
    }

    pub async fn kill_all(&mut self) {
        for (sha, deployment) in &mut self.deployments {
            info!(sha = %sha, port = deployment.port, "killing deployment");
            if let Err(e) = deployment.process.kill().await {
                warn!(sha = %sha, error = ?e, "failed to kill deployment");
            }
        }
        self.deployments.clear();
    }
}
