import time
from collections.abc import Callable
from typing import TypeVar

import httpx

T = TypeVar("T")


def wait_for_http(url: str, *, timeout: float = 120) -> httpx.Response:
    """Poll until endpoint responds with 200."""
    deadline = time.monotonic() + timeout
    last_exc: Exception | None = None
    while time.monotonic() < deadline:
        try:
            resp = httpx.get(url, timeout=5)
            if resp.status_code == 200:
                return resp
        except (httpx.ConnectError, httpx.ReadTimeout, httpx.ConnectTimeout) as e:
            last_exc = e
        time.sleep(3)
    raise TimeoutError(
        f"Timed out waiting for {url} after {timeout}s (last error: {last_exc})"
    )


def wait_for_condition(
    fn: Callable[[], T | None], *, timeout: float = 120, interval: float = 5
) -> T:
    """Poll until fn returns a truthy value."""
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        result = fn()
        if result:
            return result
        time.sleep(interval)
    raise TimeoutError(f"Condition not met after {timeout}s")


def wait_for_deploy_comment(
    owner: str,
    repo: str,
    pr: int,
    expected_sha: str,
    *,
    timeout: float = 600,
) -> dict:
    """Poll PR comment until it references the expected SHA."""
    from helpers.github_api import get_deploy_comment

    def check():
        comment = get_deploy_comment(owner, repo, pr)
        if comment and comment["sha"] == expected_sha[:7]:
            return comment
        return None

    return wait_for_condition(check, timeout=timeout, interval=10)


def wait_for_http_down(url: str, *, timeout: float = 60) -> None:
    """Poll until endpoint stops responding."""
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        try:
            httpx.get(url, timeout=3)
        except (httpx.ConnectError, httpx.ReadTimeout, httpx.ConnectTimeout):
            return
        time.sleep(2)
    raise TimeoutError(f"Endpoint {url} still responding after {timeout}s")
