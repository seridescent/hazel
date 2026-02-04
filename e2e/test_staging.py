"""Tests for staging deployment."""

from conftest import E2E_PR_NUMBER, PRODUCTION_PORT, PORT_MIN, PORT_MAX, TEST_REPO_OWNER, TEST_REPO_NAME
from helpers.github_api import get_deploy_comment


def test_staging_responds(staging_info):
    port, status = staging_info
    assert "variant" in status
    assert "version" in status
    assert "env" in status


def test_staging_variant(staging_info):
    _, status = staging_info
    assert status["variant"] == "staging"


def test_staging_test_var(staging_info):
    _, status = staging_info
    assert status["env"]["HAZEL_TEST_VAR"] == "test var set by start script!"


def test_staging_data_file(staging_info):
    _, status = staging_info
    contents = status["data_file_contents"]
    assert "Hello from the filesystem!" in contents


def test_staging_port_in_range(staging_info):
    port, _ = staging_info
    assert PORT_MIN <= port <= PORT_MAX


def test_staging_port_differs_from_production(staging_info):
    port, _ = staging_info
    assert port != PRODUCTION_PORT


def test_staging_deploy_comment_exists(staging_info):
    comment = get_deploy_comment(TEST_REPO_OWNER, TEST_REPO_NAME, E2E_PR_NUMBER)
    assert comment is not None
    assert comment["success"] is True
    assert comment["url"] is not None
