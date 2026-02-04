import subprocess
from pathlib import Path

CLONE_DIR = Path("/tmp/hazel-e2e-data/repo")


def _run_git(*args: str, cwd: Path | None = None) -> subprocess.CompletedProcess:
    result = subprocess.run(
        ["git", *args],
        cwd=cwd or CLONE_DIR,
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        raise RuntimeError(
            f"git {' '.join(args)} failed:\nstdout: {result.stdout}\nstderr: {result.stderr}"
        )
    return result


def ensure_clone(repo_url: str = "git@github.com:seridescent/hazel-test-repo.git") -> Path:
    """Clone if not present, otherwise fetch."""
    if (CLONE_DIR / ".git").exists():
        _run_git("fetch", "origin")
    else:
        CLONE_DIR.parent.mkdir(parents=True, exist_ok=True)
        subprocess.run(
            ["git", "clone", repo_url, str(CLONE_DIR)],
            check=True,
            capture_output=True,
            text=True,
        )
    return CLONE_DIR


def push_version_change(branch: str, new_version: str) -> str:
    """Checkout branch, modify version.txt, commit, push. Returns new SHA."""
    _run_git("checkout", branch)
    _run_git("pull", "origin", branch)

    version_file = CLONE_DIR / "version.txt"
    version_file.write_text(f"{new_version}\n")

    _run_git("add", "version.txt")
    _run_git("commit", "-m", f"bump version to {new_version}")
    _run_git("push", "origin", branch)

    result = _run_git("rev-parse", "HEAD")
    return result.stdout.strip()


def push_broken_commit(branch: str) -> str:
    """Append invalid TypeScript to index.ts, commit, push. Returns new SHA."""
    _run_git("checkout", branch)
    _run_git("pull", "origin", branch)

    index_file = CLONE_DIR / "index.ts"
    content = index_file.read_text()
    content += "\n// INTENTIONAL BREAK FOR E2E TEST\nthis is not valid javascript at all {{{\n"
    index_file.write_text(content)

    _run_git("add", "index.ts")
    _run_git("commit", "-m", "intentional build break for e2e test")
    _run_git("push", "origin", branch)

    result = _run_git("rev-parse", "HEAD")
    return result.stdout.strip()


def revert_last_commit(branch: str) -> str:
    """Reset HEAD~1 and force push. Returns new SHA."""
    _run_git("checkout", branch)
    _run_git("reset", "--hard", "HEAD~1")
    _run_git("push", "--force", "origin", branch)

    result = _run_git("rev-parse", "HEAD")
    return result.stdout.strip()
