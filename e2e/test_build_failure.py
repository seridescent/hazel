"""Tests for build failure handling."""

from conftest import E2E_PR_NUMBER, TEST_REPO_OWNER, TEST_REPO_NAME
from helpers.git_ops import ensure_clone, push_broken_commit, revert_last_commit
from helpers.wait import wait_for_deploy_comment


def test_build_failure(staging_info):
    """Push a broken commit, verify failure comment, then revert."""
    ensure_clone()

    try:
        # Push broken commit
        broken_sha = push_broken_commit("e2e/staging")

        # Wait for deploy comment to update with the broken SHA
        comment = wait_for_deploy_comment(
            TEST_REPO_OWNER, TEST_REPO_NAME, E2E_PR_NUMBER, broken_sha, timeout=600
        )

        # Assert failure
        assert comment["success"] is False
        assert comment["sha"] == broken_sha[:7]
        assert comment["url"] is None

    finally:
        # Always revert the broken commit
        revert_last_commit("e2e/staging")
