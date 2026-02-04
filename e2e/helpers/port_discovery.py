import re

import httpx

from helpers.github_api import get_deploy_comment


def discover_staging_port_from_comment(
    owner: str, repo: str, pr: int
) -> int | None:
    """Extract port from PR deploy comment's preview URL."""
    comment = get_deploy_comment(owner, repo, pr)
    if not comment or not comment.get("url"):
        return None
    match = re.search(r":(\d+)/?$", comment["url"])
    if match:
        return int(match.group(1))
    return None


def scan_for_staging_port(
    port_min: int = 19901, port_max: int = 19910
) -> int | None:
    """Scan port range for a responding /api/status endpoint."""
    for port in range(port_min, port_max + 1):
        try:
            resp = httpx.get(f"http://localhost:{port}/api/status", timeout=2)
            if resp.status_code == 200:
                data = resp.json()
                if data.get("variant") == "staging":
                    return port
        except (httpx.ConnectError, httpx.ReadTimeout, httpx.ConnectTimeout):
            continue
    return None
