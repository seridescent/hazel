# Hazel E2E Tests

End-to-end test suite for hazel using Python (uv + pytest). Tests run against the `seridescent/hazel-test-repo` repository using dedicated `e2e/production` and `e2e/staging` branches.

## Prerequisites

- **Tailscale** running (hazel uses Tailscale for deployment URLs)
- **`gh` CLI** authenticated (`gh auth status`)
- **hazel `.env`** file present at repo root with `GITHUB_APP_ID` and `GITHUB_APP_KEY_PATH`
- **E2E PR #10** open in hazel-test-repo (`e2e/staging` -> `e2e/production`)

## Running Tests

```sh
cd e2e

# Install dependencies (first time or after pyproject.toml changes)
uv sync

# Run all tests
uv run pytest

# Verbose output with full tracebacks
uv run pytest -v --tb=long

# Run a specific test file
uv run pytest test_production.py
uv run pytest test_staging.py

# Run a single test by name
uv run pytest -k test_production_variant
uv run pytest -k test_redeployment

# Show print output (stdout) during test run
uv run pytest -s

# Combine flags for max debugging info
uv run pytest -v -s --tb=long

# Run only non-mutating tests (skip redeployment + build failure)
uv run pytest test_production.py test_staging.py
```

## Test Structure

Tests share a **session-scoped** hazel process. The first test that depends on `hazel_process`, `production_ready`, or `staging_info` triggers:

1. `cargo build --release` of hazel
2. Starting hazel as a subprocess with e2e-specific env vars
3. Waiting for production/staging deployments to come up

Tests are collected alphabetically, so ordering is implicit:

| File | What it tests | Mutating? |
|------|--------------|-----------|
| `test_build_failure.py` | Push broken commit, verify failure comment, revert | Yes (reverts) |
| `test_production.py` | Production deployment health, env vars, data file | No |
| `test_redeployment.py` | Push version bump, verify redeploy with new version | Yes |
| `test_staging.py` | Staging deployment health, env vars, deploy comment | No |

## Configuration

All config is in `conftest.py`:

| Value | Setting |
|-------|---------|
| Test repo | `seridescent/hazel-test-repo` |
| E2E PR | `#10` (`e2e/staging` -> `e2e/production`) |
| Production port | `19900` |
| Staging port range | `19901-19910` |
| Data dir | `/tmp/hazel-e2e-data/ephemeral` |
| Prod run dir | `/tmp/hazel-e2e-data/prod` |
| Poll interval | `10s` |
| Log level | `debug` |

## Runtime Directories

```
/tmp/hazel-e2e-data/
  repo/          # Local clone of hazel-test-repo (used by git_ops for pushing)
  ephemeral/     # HAZEL_DATA_DIR (repos, checkouts, staging subdirs) -- cleaned each run
  prod/          # HAZEL_PRODUCTION_RUN_DIR (persistent, data.txt seeded by test harness)
```

## Helper Modules

- `helpers/hazel_runner.py` -- Build + start/stop hazel binary
- `helpers/git_ops.py` -- Clone/push commits to test repo branches
- `helpers/port_discovery.py` -- Extract staging port from PR deploy comment
- `helpers/wait.py` -- Polling/retry helpers (HTTP, conditions, deploy comments)
- `helpers/github_api.py` -- Read PR comments and branch SHAs via `gh api`

## Hazel Logs

On test failure (or always at teardown), hazel's stdout/stderr are dumped. Logs are also written to:

- `/tmp/hazel-e2e-data/hazel-stdout.log`
- `/tmp/hazel-e2e-data/hazel-stderr.log`

## Test Repo Branches

- **`e2e/production`** -- Production branch hazel watches. Has `/api/status` JSON endpoint, `version.txt`, `HAZEL_VARIANT="production"` in flake.
- **`e2e/staging`** -- Staging branch with open PR against `e2e/production`. Has `HAZEL_VARIANT="staging"` in flake.

The `/api/status` endpoint returns:

```json
{
  "variant": "staging|production|unknown",
  "version": "1",
  "data_file_contents": "...",
  "env": { "HAZEL_PORT": "...", "HAZEL_RUN_DIR": "...", ... }
}
```

## Notes

- First run is slow due to Nix builds; subsequent runs use cached derivations.
- `test_redeployment` and `test_build_failure` push to the test repo and wait for hazel to detect + redeploy (up to 600s timeout).
- `test_build_failure` always reverts its broken commit in a `finally` block.
- Production `data.txt` is seeded by the test harness (not by hazel's preStart) because the prod run dir is persistent across redeploys.
