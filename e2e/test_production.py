"""Tests for production deployment."""


def test_production_responds(production_ready):
    status = production_ready
    assert "variant" in status
    assert "version" in status
    assert "env" in status


def test_production_variant(production_ready):
    assert production_ready["variant"] == "production"


def test_production_port(production_ready):
    assert production_ready["env"]["HAZEL_PORT"] == "19900"


def test_production_test_var(production_ready):
    assert production_ready["env"]["HAZEL_TEST_VAR"] == "test var set by prod script!"


def test_production_data_file(production_ready):
    contents = production_ready["data_file_contents"]
    assert "Hello from the filesystem!" in contents


def test_production_version(production_ready):
    # version.txt starts at "1" and increments over test runs
    version = production_ready["version"]
    assert version.isdigit()


def test_production_custom_env_shared(production_ready):
    """Base env var passed through to production."""
    # Production doesn't override, so base value should appear
    assert production_ready["env"]["HAZEL_CUSTOM_SHARED"] == "shared-value-from-base"


def test_production_custom_env_specific(production_ready):
    """Production-specific env var passed through."""
    assert production_ready["env"]["HAZEL_CUSTOM_PRODUCTION"] == "production-specific-value"
