# Hazel

GitHub App for self-hosted PR deployments using Nix and Tailscale.

## Architecture

Hazel polls a single GitHub repository for open PRs and deploys each PR's head commit. Deployments are accessible via Tailscale MagicDNS.

### Modules

- `main.rs` - Entry point, polling loop, deployment orchestration
- `installation.rs` - GitHub App installation, token management, PR fetching
- `deploy.rs` - Nix-based deployment lifecycle (start/kill)
- `git.rs` - Bare repo management, commit extraction via git archive
- `port_allocator.rs` - Port range management for deployments
- `cached_token.rs` - GitHub installation token caching

### Deployment Flow

1. Poll GitHub API for open PRs
2. For new commits: fetch to bare repo, extract via `git archive`
3. Run `nix run .#hazel-preStart` then spawn `nix run .#hazel-executable`
4. Post preview URL comment on PR
5. Kill deployments when PRs are closed/merged

## Environment Variables

Required:
- `HAZEL_DATA_DIR` - Directory for repos, checkouts, and deploys
- `GITHUB_APP_ID` - GitHub App ID
- `GITHUB_APP_KEY_PATH` - Path to GitHub App private key
- `HAZEL_WATCHED_REPO_OWNER` - Repository owner to watch
- `HAZEL_WATCHED_REPO_NAME` - Repository name to watch
- `HAZEL_PORT_MIN` / `HAZEL_PORT_MAX` - Port range for deployments
- `HAZEL_POLL_INTERVAL_SECS` - Polling interval in seconds

Passed to deployments:
- `HAZEL_PORT` - Assigned port for the deployment
- `HAZEL_RUN_DIR` - Working directory for the deployment
- `HAZEL_ORIGIN` - Full origin URL (e.g., `http://hostname:port`)

## Logging

Uses `tracing` with default INFO level. Override with `RUST_LOG` env var.

Info-level logs appear for:
- Startup (`hazel started`)
- New deployments detected and lifecycle events
- Shutdown

Debug-level logs (enable with `RUST_LOG=debug`):
- Poll status every interval
- Token refresh events
- PR comment updates

## Target Repository Requirements

Repositories must expose two Nix flake outputs:
- `hazel-preStart` - Setup script (runs once before deployment)
- `hazel-executable` - Long-running server process
