# Deploy Management

## Directory Structure

```
data/
  repos/{owner}/{repo}/repo.git   # bare repos (git objects, fetch target)
  checkouts/{sha}/                # extracted source (for nix builds)
  deploys/{sha}/                  # runtime dir (HAZEL_RUN_DIR)
```

Checkouts and deploys are SHA-keyed and repo-agnostic.
Downstream code only needs `checkouts/{sha}` and `deploys/{sha}` - no repo awareness.

## Process Tracking

`DeploymentManager` tracks running deployments in `HashMap<sha, Deployment>`:

```rust
struct Deployment {
    sha: String,
    port: u16,
    process: Child,  // tokio::process::Child
}
```

When a new commit comes in for a PR:
1. Look up existing deployment by SHA
2. Kill the old process
3. Remove old deploy dir
4. Start new deployment with new SHA

## Port Allocation

Configurable range via `HAZEL_PORT_MIN` / `HAZEL_PORT_MAX` env vars.
Simple incrementing allocator for now.

Future: reclaimed ports heap - draw from freed ports before incrementing.

## Lifecycle

1. `git::extract_commit` - extract source to `checkouts/{sha}`
2. `nix run path#hazel-preStart` - populate `deploys/{sha}`
3. `nix run path#hazel-executable` - spawn, track handle
4. On new commit: kill old, cleanup, restart
5. On PR close: kill, cleanup
6. On shutdown: kill all tracked processes

## Testing Ideas

### Unit Tests

- **Port allocation**: test increment, range exhaustion, future reclamation
- **DeploymentManager state**: insert/remove deployments, lookup by SHA

### Integration Tests

- **Mock checkout dir**: create temp dir with minimal flake that just sleeps
- **Process lifecycle**: start deployment, verify process running, kill, verify dead
- **Port assignment**: start multiple deployments, verify unique ports

### Test Fixtures

Create a minimal test flake:
```nix
{
  outputs = { self, nixpkgs }: {
    packages.x86_64-linux = {
      hazel-preStart = pkgs.writeShellApplication {
        name = "hazel-preStart";
        text = "touch $HAZEL_RUN_DIR/started";
      };
      hazel-executable = pkgs.writeShellApplication {
        name = "hazel-executable";
        text = "sleep infinity";
      };
    };
  };
}
```

### E2E Tests

- Spin up hazel with test repo, verify deployments start on correct ports
- Push commit, verify old deployment killed, new one started
- Close PR, verify deployment cleaned up

## Future

- Capture stdout/stderr to files in deploy dir
- tailscale serve to expose `https://{sha}.{tailnet}.ts.net` or similar
- Health checks / restart on crash
