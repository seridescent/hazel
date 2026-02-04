import json
import re
import subprocess


MARKER = "<!-- hazel-deploy -->"


def _gh_api(*args: str) -> subprocess.CompletedProcess:
    """Run gh api with caching disabled."""
    return subprocess.run(
        ["gh", "api", "--cache=0s", *args],
        capture_output=True,
        text=True,
        check=True,
    )


def get_deploy_comment(owner: str, repo: str, pr: int) -> dict | None:
    """Get hazel deploy comment from a PR.

    Returns dict with keys: url (str|None), sha (str), success (bool)
    or None if no deploy comment found.
    """
    result = _gh_api(f"repos/{owner}/{repo}/issues/{pr}/comments", "--paginate")
    comments = json.loads(result.stdout)

    for comment in comments:
        body = comment.get("body", "")
        if MARKER not in body:
            continue

        success = "## 🚀 Preview deployed" in body
        sha_match = re.search(r"\*\*Commit:\*\* `([a-f0-9]+)`", body)
        sha = sha_match.group(1) if sha_match else ""

        url = None
        if success:
            url_match = re.search(r"\*\*URL:\*\* (http\S+)", body)
            url = url_match.group(1) if url_match else None

        return {"url": url, "sha": sha, "success": success}

    return None


def delete_deploy_comments(owner: str, repo: str, pr: int) -> int:
    """Delete all hazel deploy comments from a PR. Returns count deleted."""
    result = _gh_api(f"repos/{owner}/{repo}/issues/{pr}/comments", "--paginate")
    comments = json.loads(result.stdout)

    deleted = 0
    for comment in comments:
        body = comment.get("body", "")
        if MARKER not in body:
            continue
        comment_id = comment["id"]
        subprocess.run(
            ["gh", "api", "--method=DELETE", f"repos/{owner}/{repo}/issues/comments/{comment_id}"],
            capture_output=True,
            text=True,
            check=True,
        )
        deleted += 1

    return deleted


def get_branch_head_sha(owner: str, repo: str, branch: str) -> str:
    """Get HEAD SHA of a branch."""
    result = _gh_api(f"repos/{owner}/{repo}/git/ref/heads/{branch}")
    data = json.loads(result.stdout)
    return data["object"]["sha"]
