use anyhow::{Context, bail};
use std::collections::HashMap;
use std::path::Path;
use tokio::process::{Child, Command};
use tracing::{info, warn};

pub struct Deployment {
    pub sha: String,
    pub port: u16,
    pub process: Child,
    pub serve: Child,
}

pub struct DeploymentManager {
    tailscale_proxy_port: u16,
    port_min: u16,
    port_max: u16,
    next_port: u16,
    // TODO: add reclaimed_ports: BinaryHeap<Reverse<u16>> for port reuse
    pub deployments: HashMap<String, Deployment>,
}

impl DeploymentManager {
    pub fn new(tailscale_proxy_port: u16, port_min: u16, port_max: u16) -> Self {
        Self {
            tailscale_proxy_port,
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

    pub async fn start(
        &mut self,
        sha: &str,
        checkout_dir: &Path,
        run_dir: &Path,
    ) -> anyhow::Result<()> {
        let port = self.allocate_port()?;

        // TODO: lift the rest of this except the map insert into a function
        // that takes a port so it can be its own task

        tokio::fs::create_dir_all(run_dir).await?;

        info!(sha = %sha, port = port, "starting deployment");

        // TODO: provide a host env var (e.g. a <device-name>.tailnet-id.ts.net)
        // so services can allow-list it during staging.
        // can get this DNSName with equivalent of `tailscale status --self --json | jq ".Self.DNSName"`

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
                &format!("--http={}", self.tailscale_proxy_port),
                &format!("--set-path={sha}"),
                &format!("localhost:{port}"),
            ])
            .current_dir(run_dir)
            .spawn()
            .context("failed to serve via tailscale")?;

        info!(sha = %sha, serve_pid = ?serve.id(), "tailscale serve started");

        self.deployments.insert(
            sha.to_string(),
            Deployment {
                sha: sha.to_string(),
                port,
                process,
                serve,
            },
        );

        Ok(())
    }

    pub async fn kill_all(&mut self) {
        for (sha, deployment) in &mut self.deployments {
            info!(sha = %sha, port = deployment.port, "killing deployment");

            if let Err(e) = deployment.serve.kill().await {
                warn!(sha = %sha, error = ?e, "failed to kill tailscale serve");
            }

            if let Err(e) = deployment.process.kill().await {
                warn!(sha = %sha, error = ?e, "failed to kill deployment");
            }
        }
        self.deployments.clear();
    }
}
