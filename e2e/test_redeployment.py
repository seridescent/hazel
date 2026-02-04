"""Tests for redeployment after pushing a new commit."""

import httpx

from conftest import E2E_PR_NUMBER, TEST_REPO_OWNER, TEST_REPO_NAME, PORT_MIN, PORT_MAX
from helpers.git_ops import ensure_clone, push_version_change
from helpers.port_discovery import discover_staging_port_from_comment
from helpers.wait import wait_for_deploy_comment, wait_for_http, wait_for_http_down


def test_redeployment(staging_info):
    old_port, old_status = staging_info
    old_version = old_status["version"]

    # Compute new version
    try:
        new_version = str(int(old_version) + 1)
    except ValueError:
        new_version = "2"

    # Push version change
    ensure_clone()
    new_sha = push_version_change("e2e/staging", new_version)

    # Wait for deploy comment to update with new SHA
    comment = wait_for_deploy_comment(
        TEST_REPO_OWNER, TEST_REPO_NAME, E2E_PR_NUMBER, new_sha, timeout=600
    )
    assert comment["success"] is True

    # Discover new port from updated comment
    new_port = discover_staging_port_from_comment(
        TEST_REPO_OWNER, TEST_REPO_NAME, E2E_PR_NUMBER
    )
    assert new_port is not None
    assert PORT_MIN <= new_port <= PORT_MAX

    # Verify new deployment has updated version
    resp = wait_for_http(f"http://localhost:{new_port}/api/status", timeout=120)
    new_status = resp.json()
    assert new_status["version"] == new_version
    assert new_status["variant"] == "staging"

    # Verify old port no longer responds (if port changed)
    if new_port != old_port:
        wait_for_http_down(f"http://localhost:{old_port}/api/status", timeout=60)
