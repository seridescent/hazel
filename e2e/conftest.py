import os
import shutil
import subprocess
from pathlib import Path

import httpx
import pytest

from helpers.github_api import delete_deploy_comments, get_deploy_comment
from helpers.hazel_runner import HazelRunner
from helpers.port_discovery import (
    discover_staging_port_from_comment,
    scan_for_staging_port,
)
from helpers.wait import wait_for_condition, wait_for_http

# -- Config --

HAZEL_REPO_DIR = Path(__file__).resolve().parent.parent  # hazel/
TEST_REPO_OWNER = "seridescent"
TEST_REPO_NAME = "hazel-test-repo"
E2E_PR_NUMBER = 10
PRODUCTION_PORT = 19900
PORT_MIN = 19901
PORT_MAX = 19910
DATA_DIR = Path("/tmp/hazel-e2e-data/ephemeral")
PROD_RUN_DIR = Path("/tmp/hazel-e2e-data/prod")

# Production data.txt content -- seeded by test harness since the prod run dir
# is persistent and the flake's preStart isn't used for production data seeding.
PROD_DATA_TXT = "Hello from the filesystem! I have been modified from main!\nThis file is read at runtime by the Bun service.\n"


def _load_env_file() -> dict[str, str]:
    """Load key=value pairs from hazel/.env."""
    env = {}
    env_file = HAZEL_REPO_DIR / ".env"
    if not env_file.exists():
        pytest.fail(f"Missing .env file at {env_file}")
    for line in env_file.read_text().splitlines():
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        key, _, value = line.partition("=")
        # Strip optional quotes
        value = value.strip().strip('"').strip("'")
        env[key.strip()] = value
    return env


# -- Pre-flight checks --


def pytest_configure(config):
    """Verify prerequisites before running any tests."""
    # Tailscale running
    result = subprocess.run(
        ["tailscale", "status", "--self", "--json"],
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        pytest.exit("Pre-flight failed: tailscale is not running", returncode=1)

    # gh CLI authenticated
    result = subprocess.run(
        ["gh", "auth", "status"],
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        pytest.exit("Pre-flight failed: gh CLI is not authenticated", returncode=1)

    # .env exists
    env_file = Path(__file__).resolve().parent.parent / ".env"
    if not env_file.exists():
        pytest.exit(f"Pre-flight failed: {env_file} not found", returncode=1)

    # e2e PR is open
    result = subprocess.run(
        [
            "gh",
            "api",
            f"repos/{TEST_REPO_OWNER}/{TEST_REPO_NAME}/pulls/{E2E_PR_NUMBER}",
        ],
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        pytest.exit(
            f"Pre-flight failed: PR #{E2E_PR_NUMBER} not accessible", returncode=1
        )


# -- Session fixtures --


@pytest.fixture(scope="session")
def hazel_env() -> dict[str, str]:
    """Build the env dict for hazel from .env + e2e overrides."""
    base_env = _load_env_file()

    # Ensure data dirs exist
    DATA_DIR.mkdir(parents=True, exist_ok=True)
    PROD_RUN_DIR.mkdir(parents=True, exist_ok=True)

    # Seed production data.txt -- the prod run dir is persistent and production
    # doesn't use preStart for data seeding (unlike staging which gets a fresh
    # run dir each deploy). We write it here so the service can read it.
    prod_data_file = PROD_RUN_DIR / "data.txt"
    if not prod_data_file.exists():
        prod_data_file.write_text(PROD_DATA_TXT)

    return {
        "GITHUB_APP_ID": base_env["GITHUB_APP_ID"],
        "GITHUB_APP_KEY_PATH": str(
            (HAZEL_REPO_DIR / base_env["GITHUB_APP_KEY_PATH"]).resolve()
        ),
        "HAZEL_DATA_DIR": str(DATA_DIR),
        "HAZEL_WATCHED_REPO_OWNER": TEST_REPO_OWNER,
        "HAZEL_WATCHED_REPO_NAME": TEST_REPO_NAME,
        "HAZEL_PORT_MIN": str(PORT_MIN),
        "HAZEL_PORT_MAX": str(PORT_MAX),
        "HAZEL_POLL_INTERVAL_SECS": "10",
        "HAZEL_PRODUCTION_ENABLE": "true",
        "HAZEL_PRODUCTION_PORT": str(PRODUCTION_PORT),
        "HAZEL_PRODUCTION_RUN_DIR": str(PROD_RUN_DIR),
        "HAZEL_PRODUCTION_ORIGIN": f"http://localhost:{PRODUCTION_PORT}",
        "HAZEL_PRODUCTION_BRANCH": "e2e/production",
        "RUST_LOG": "debug",
    }


@pytest.fixture(scope="session")
def hazel_process(hazel_env):
    """Build and start hazel, yield, then stop on teardown."""
    # Clean up stale deploy comments so this run starts from a known state.
    deleted = delete_deploy_comments(TEST_REPO_OWNER, TEST_REPO_NAME, E2E_PR_NUMBER)
    if deleted:
        print(f"Cleaned up {deleted} stale deploy comment(s) from PR #{E2E_PR_NUMBER}")

    runner = HazelRunner(str(HAZEL_REPO_DIR), hazel_env)
    runner.build()
    runner.start()
    yield runner
    runner.stop()
    runner.dump_logs()


@pytest.fixture(scope="session")
def production_ready(hazel_process) -> dict:
    """Wait for production to respond on its port, return status JSON."""
    url = f"http://localhost:{PRODUCTION_PORT}/api/status"
    resp = wait_for_http(url, timeout=600)
    return resp.json()


@pytest.fixture(scope="session")
def staging_info(hazel_process) -> tuple[int, dict]:
    """Wait for staging deploy comment, discover port, return (port, status)."""
    # Wait for deploy comment to appear
    comment = wait_for_condition(
        lambda: get_deploy_comment(TEST_REPO_OWNER, TEST_REPO_NAME, E2E_PR_NUMBER),
        timeout=600,
        interval=10,
    )

    port = discover_staging_port_from_comment(
        TEST_REPO_OWNER, TEST_REPO_NAME, E2E_PR_NUMBER
    )
    if port is None:
        # Fallback to port scanning
        port = scan_for_staging_port(PORT_MIN, PORT_MAX)
    if port is None:
        pytest.fail("Could not discover staging port from comment or port scan")

    resp = wait_for_http(f"http://localhost:{port}/api/status", timeout=120)
    return (port, resp.json())
