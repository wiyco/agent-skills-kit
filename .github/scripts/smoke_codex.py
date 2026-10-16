"""Explicit macOS-only Codex listing smoke test; never a required host CI job."""

import argparse
import json
import os
from pathlib import Path
import platform
import selectors
import shutil
import signal
import subprocess
import sys
import tempfile
import time

NAME = "ask-discovery-probe"
FIXTURE = Path(__file__).resolve().parents[2] / "fixtures/discovery" / NAME


def child_environment(root):
    """A new child-only environment, without inherited credentials/configuration."""
    return {
        "PATH": "/usr/bin:/bin:/opt/homebrew/bin",
        "HOME": str(root / "home"), "CODEX_HOME": str(root / "state"),
        "XDG_CONFIG_HOME": str(root / "config"), "XDG_DATA_HOME": str(root / "data"),
        "XDG_CACHE_HOME": str(root / "cache"), "XDG_STATE_HOME": str(root / "state"),
        "TMPDIR": str(root / "tmp"), "TERM": "dumb", "LANG": "en_US.UTF-8",
    }


def isolation_profile(root, real_home):
    """Deny network, real-home reads and persistent writes outside owned state."""
    if (not root.is_absolute() or not real_home.is_absolute()
            or root.is_relative_to(real_home) or real_home.is_relative_to(root)):
        raise ValueError("isolation requires an absolute temporary root outside the real home")
    return ('(version 1)(allow default)(deny network*)(deny file-write*)'
            '(allow file-write* (subpath ' + json.dumps(str(root)) + '))'
            '(allow file-write* (literal "/dev/null"))'
            '(deny file-read* (subpath ' + json.dumps(str(real_home)) + '))'
            '(deny mach-lookup (global-name "com.apple.securityd")'
            '(global-name "com.apple.securityd.xpc"))')


def verify_isolation(profile, environment, root, real_home):
    """Prove denial before launching a host, using only an owned write sentinel."""
    probe = '''import os, socket, sys
def denied(operation):
    try:
        operation()
    except PermissionError:
        return
    raise RuntimeError("isolation denial not demonstrated")
denied(lambda: os.stat(sys.argv[1]))
denied(lambda: open(sys.argv[2], "wb"))
with socket.socket() as connection:
    denied(lambda: connection.connect(("127.0.0.1", 9)))
'''
    with tempfile.TemporaryDirectory(prefix="ask-smoke-guard-", dir="/private/tmp") as guard:
        sentinel = Path(guard) / "sentinel"
        sentinel.write_bytes(b"owned isolation sentinel")
        subprocess.run(["/usr/bin/sandbox-exec", "-p", profile, sys.executable, "-I", "-S",
                        "-c", probe, str(real_home), str(sentinel)], cwd=root, env=environment,
                       capture_output=True, check=True, timeout=10)
        if sentinel.read_bytes() != b"owned isolation sentinel":
            raise RuntimeError("isolation sentinel changed")


def receive(process, selector, buffer, identifier):
    deadline = time.monotonic() + 20
    total = len(buffer)
    while time.monotonic() < deadline:
        while b"\n" in buffer:
            line, buffer = buffer.split(b"\n", 1)
            message = json.loads(line)
            if message.get("id") == identifier:
                if "error" in message:
                    raise RuntimeError("Codex rejected the discovery-only RPC")
                return message["result"], buffer
        if selector.select(0.1):
            data = os.read(process.stdout.fileno(), 65536)
            if not data:
                raise RuntimeError("Codex closed stdout before the discovery response")
            total += len(data)
            if total > 2 * 1024 * 1024:
                raise RuntimeError("Codex response exceeded the 2 MiB observation limit")
            buffer += data
    raise TimeoutError("Codex discovery response exceeded 20 seconds")


def list_skills(command, environment, project, errors):
    """Only initialize/initialized/skills-list; never thread-start or turn-start."""
    process = subprocess.Popen(command + ["app-server", "--stdio"], cwd=project,
                               env=environment, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                               stderr=errors, start_new_session=True)
    selector = selectors.DefaultSelector()
    selector.register(process.stdout, selectors.EVENT_READ)

    def send(message):
        process.stdin.write(json.dumps(message).encode() + b"\n")
        process.stdin.flush()

    try:
        send({"id": 0, "method": "initialize", "params": {
            "clientInfo": {"name": "ask_discovery_smoke", "version": "0.1.0"}}})
        _initialized, buffer = receive(process, selector, b"", 0)
        send({"method": "initialized", "params": {}})
        send({"id": 1, "method": "skills/list", "params": {
            "cwds": [str(project)], "forceReload": True}})
        result, _buffer = receive(process, selector, buffer, 1)
        rows = result["data"]
        if len(rows) != 1 or rows[0].get("cwd") != str(project) or rows[0].get("errors"):
            raise RuntimeError("unexpected discovery scope or host discovery errors")
        if any(row.get("enabled") is not True for row in rows[0]["skills"] if row["name"] == NAME):
            raise RuntimeError("fixture was listed but disabled")
        return [{"name": row["name"], "scope": row["scope"], "path": row["path"]}
                for row in rows[0]["skills"] if row["name"] == NAME]
    finally:
        # This is our own new process group, never an existing shared daemon.
        try:
            os.killpg(process.pid, signal.SIGTERM)
        except (ProcessLookupError, PermissionError):
            if process.poll() is None:
                raise
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            os.killpg(process.pid, signal.SIGKILL)
            process.wait(timeout=5)
        selector.close()
        process.stdin.close()
        process.stdout.close()


def smoke(codex, installer, scope):
    if platform.system() != "Darwin" or not Path("/usr/bin/sandbox-exec").is_file():
        raise ValueError("only the reviewed macOS sandbox backend is available; no fallback")
    for executable in (codex, installer):
        if not executable.is_absolute() or not executable.is_file():
            raise ValueError("provide absolute paths to existing trusted executables")
    with tempfile.TemporaryDirectory(prefix="ask-host-smoke-", dir="/private/tmp") as temporary:
        root = Path(temporary).resolve()
        for name in ("home", "state", "config", "data", "cache", "tmp", "project", NAME):
            (root / name).mkdir()
        # Copy exactly the original inert fixture, never caller-supplied skills.
        payload = (FIXTURE / "SKILL.md").read_bytes()
        source = root / NAME
        (source / "SKILL.md").write_bytes(payload)
        (root / "state/config.toml").write_text(
            'cli_auth_credentials_store = "file"\n[analytics]\nenabled = false\n', encoding="utf-8")
        environment = child_environment(root)
        real_home = Path.home().resolve()
        profile = isolation_profile(root, real_home)
        verify_isolation(profile, environment, root, real_home)
        command = ["/usr/bin/sandbox-exec", "-p", profile, str(codex)]
        # The build may live under the denied real home. Copy only this trusted
        # executable into owned state, then constrain the installer as well.
        isolated_installer = root / "installer"
        shutil.copyfile(installer, isolated_installer)
        isolated_installer.chmod(0o700)
        version = subprocess.run(command + ["--version"], cwd=root, env=environment,
                                 capture_output=True, text=True, check=True, timeout=20).stdout.strip()
        with (root / "host-stderr").open("wb") as errors:
            if list_skills(command, environment, root / "project", errors):
                raise RuntimeError("fixture unexpectedly present in the empty baseline")
            # The existing CLI calls the public library planner/executor. This
            # harness does not implement an installation engine or path registry.
            subprocess.run(["/usr/bin/sandbox-exec", "-p", profile, str(isolated_installer),
                            "install", str(source), "--target", "codex",
                            "--scope", scope, "--project-root", str(root / "project"),
                            "--user-home", str(root / "home"), "--yes", "--json"],
                           cwd=root, env=environment, stdout=subprocess.DEVNULL,
                           stderr=errors, check=True, timeout=20)
            found = list_skills(command, environment, root / "project", errors)
        base = root / ("project" if scope == "project" else "home")
        expected = base / ".agents/skills" / NAME / "SKILL.md"
        if found != [{"name": NAME, "scope": "repo" if scope == "project" else "user", "path": str(expected)}]:
            raise RuntimeError("host did not list exactly the installed fixture in the requested scope")
        if expected.read_bytes() != payload or (source / "SKILL.md").read_bytes() != payload:
            raise RuntimeError("fixture bytes changed during discovery")
        return {"host": version, "os": platform.mac_ver()[0], "architecture": platform.machine(),
                "scope": scope, "empty_baseline": True, "host_smoke_tested": True,
                "result": "listed installed metadata; no invocation or model turn",
                "path": str(expected).replace(str(root), "<temporary-root>")}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--codex", type=Path, required=True)
    parser.add_argument("--installer", type=Path, required=True)
    parser.add_argument("--scope", choices=("project", "user"), required=True)
    args = parser.parse_args()
    try:
        print(json.dumps(smoke(args.codex, args.installer, args.scope), sort_keys=True))
    except (OSError, ValueError, RuntimeError, KeyError, subprocess.SubprocessError) as error:
        # Raw host diagnostics can contain local paths. Keep those out of the
        # sanitized report; failure is never converted into a skipped success.
        parser.exit(1, f"Discovery smoke failed ({type(error).__name__}); no passing result recorded.\n")


if __name__ == "__main__":
    main()
