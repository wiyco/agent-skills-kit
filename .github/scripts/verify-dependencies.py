"""Run the real locked dependency checks, then prove two controlled rejections."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]


def rejection(command, marker, expected_code):
    result = subprocess.run(command, cwd=ROOT, capture_output=True, text=True, timeout=60)
    if result.returncode != expected_code or marker not in result.stderr:
        raise RuntimeError("negative gate did not produce the expected policy diagnostic")
    print("Confirmed controlled rejection:", marker)


def verify(tool):
    cargo_home = Path(os.environ["CARGO_HOME"])
    if not cargo_home.is_absolute() or not tool.is_absolute() or not tool.is_file():
        raise ValueError("use explicit absolute task-owned CARGO_HOME and trusted tool paths")
    version = subprocess.run([str(tool), "--version"], check=True, capture_output=True,
                             text=True, timeout=10).stdout.strip()
    if version != "cargo-deny 0.20.2":
        raise ValueError("the reviewed cargo-deny version is 0.20.2")
    locks = [ROOT / "Cargo.lock", ROOT / "examples/external-consumer/Cargo.lock"]
    before = {path: hashlib.sha256(path.read_bytes()).digest() for path in locks}
    git_environment = dict(os.environ, GIT_CONFIG_GLOBAL=os.devnull,
                           GIT_CONFIG_NOSYSTEM="1", GIT_TERMINAL_PROMPT="0")
    subprocess.run([str(tool), "--locked", "--all-features", "check"], cwd=ROOT,
                   env=git_environment, check=True, timeout=180)
    databases = [path for path in (cargo_home / "advisory-dbs").iterdir()
                 if path.is_dir() and (path / ".git").exists()]
    if len(databases) != 1:
        raise RuntimeError("expected one official database from the successful positive check")
    with tempfile.TemporaryDirectory(prefix="ask-dependency-gates-") as temporary:
        root = Path(temporary).resolve()
        license_config = root / "rejected-license.toml"
        license_config.write_text('[licenses]\nallow = []\n[licenses.private]\nignore = false\n', encoding="utf-8")
        rejection([str(tool), "--locked", "--offline", "--config", str(license_config),
                   "check", "licenses"], "error[rejected]", 4)
        # Use a separate local clone, not the production advisory cache. Git
        # config/hooks/templates are disabled; no skill or database content runs.
        database = root / "advisory-dbs" / databases[0].name
        database.parent.mkdir()

        def git(*args, cwd=root):
            subprocess.run(["git", "-c", "core.hooksPath=" + os.devnull,
                            "-c", "init.templateDir=", *args], cwd=cwd, env=git_environment,
                           check=True, capture_output=True, timeout=30)

        git("clone", "--quiet", "--no-hardlinks", str(databases[0]), str(database))
        fixture = database / "crates/serde_json/RUSTSEC-2026-9999.md"
        fixture.parent.mkdir(exist_ok=True)
        fixture.write_bytes((ROOT / "fixtures/security/RUSTSEC-2026-9999.md").read_bytes())
        git("add", "crates", cwd=database)
        git("-c", "user.name=ASK fixture", "-c", "user.email=fixture@example.invalid",
            "commit", "--quiet", "-m", "test(security): add synthetic advisory fixture", cwd=database)
        advisory_config = root / "synthetic-advisory.toml"
        # TOML basic strings accept these JSON-escaped paths on the measured
        # macOS/Linux platforms; no Windows execution is claimed for this tool.
        advisory_config.write_text('[advisories]\ndb-path = ' + json.dumps(str(database.parent))
                                   + '\ndb-urls = ["https://github.com/RustSec/advisory-db"]\n', encoding="utf-8")
        rejection([str(tool), "--locked", "--offline", "--config", str(advisory_config),
                   "check", "advisories"], "ID: RUSTSEC-2026-9999", 1)
    if any(hashlib.sha256(path.read_bytes()).digest() != before[path] for path in locks):
        raise RuntimeError("a locked dependency check changed a committed lockfile")
    print("Production policy passed; synthetic license/advisory rejections passed; lockfiles unchanged.")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cargo-deny", type=Path, required=True)
    arguments = parser.parse_args()
    verify(arguments.cargo_deny)
