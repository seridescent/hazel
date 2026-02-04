import os
import signal
import subprocess
import sys
import time
from pathlib import Path


class HazelRunner:
    """Builds and runs the hazel binary as a subprocess."""

    def __init__(self, hazel_repo_dir: str, env: dict[str, str]):
        self.hazel_repo_dir = Path(hazel_repo_dir)
        self.env = env
        self.process: subprocess.Popen | None = None
        self._stdout_path: Path | None = None
        self._stderr_path: Path | None = None

    def build(self) -> None:
        """Run cargo build --release."""
        print("Building hazel (cargo build --release)...", flush=True)
        result = subprocess.run(
            ["cargo", "build", "--release"],
            cwd=self.hazel_repo_dir,
            capture_output=True,
            text=True,
        )
        if result.returncode != 0:
            print(f"cargo build stdout:\n{result.stdout}", file=sys.stderr)
            print(f"cargo build stderr:\n{result.stderr}", file=sys.stderr)
            raise RuntimeError(f"cargo build failed with exit code {result.returncode}")
        print("Build complete.", flush=True)

    def start(self) -> None:
        """Start the hazel binary."""
        binary = self.hazel_repo_dir / "target" / "release" / "hazel"
        if not binary.exists():
            raise FileNotFoundError(f"Binary not found: {binary}")

        log_dir = Path("/tmp/hazel-e2e-data")
        log_dir.mkdir(parents=True, exist_ok=True)
        self._stdout_path = log_dir / "hazel-stdout.log"
        self._stderr_path = log_dir / "hazel-stderr.log"

        full_env = {**os.environ, **self.env}

        self.process = subprocess.Popen(
            [str(binary)],
            env=full_env,
            stdout=open(self._stdout_path, "w"),
            stderr=open(self._stderr_path, "w"),
        )
        # Give it a moment to start
        time.sleep(2)
        if self.process.poll() is not None:
            raise RuntimeError(
                f"Hazel exited immediately with code {self.process.returncode}\n"
                f"stderr: {self._stderr_path.read_text()}"
            )
        print(f"Hazel started (pid={self.process.pid})", flush=True)

    def stop(self) -> None:
        """Send SIGINT for graceful shutdown, then wait."""
        if self.process is None:
            return
        if self.process.poll() is not None:
            return

        print("Stopping hazel (SIGINT)...", flush=True)
        self.process.send_signal(signal.SIGINT)
        try:
            self.process.wait(timeout=30)
        except subprocess.TimeoutExpired:
            print("Hazel did not stop in time, killing...", file=sys.stderr)
            self.process.kill()
            self.process.wait()
        print("Hazel stopped.", flush=True)

    def dump_logs(self) -> None:
        """Print captured logs for debugging."""
        if self._stdout_path and self._stdout_path.exists():
            print(f"\n--- hazel stdout ({self._stdout_path}) ---")
            print(self._stdout_path.read_text()[-5000:])
        if self._stderr_path and self._stderr_path.exists():
            print(f"\n--- hazel stderr ({self._stderr_path}) ---")
            print(self._stderr_path.read_text()[-5000:])
