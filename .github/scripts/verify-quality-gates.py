"""Prove quality commands fail on controlled regressions in temporary copies."""

from pathlib import Path
import shutil
import subprocess
import tempfile


ROOT = Path(__file__).resolve().parents[2]


def expect_rejection(command, source, diagnostic):
    with tempfile.TemporaryDirectory(prefix="ask-quality-") as temporary:
        destination = Path(temporary)
        for name in ("Cargo.toml", "Cargo.lock", "rust-toolchain.toml"):
            shutil.copy2(ROOT / name, destination / name)
        shutil.copytree(ROOT / "crates", destination / "crates")
        library = destination / "crates/agent-skills-kit/src/lib.rs"
        with library.open("a", encoding="utf-8") as output:
            output.write(source)
        result = subprocess.run(
            command, cwd=destination, capture_output=True, text=True, check=False
        )
        combined = result.stdout + result.stderr
        if result.returncode == 0 or diagnostic not in combined:
            raise RuntimeError(f"Expected quality rejection from {command}:\n{combined}")
        print(f"Confirmed rejection: {' '.join(command)} ({diagnostic})")


expect_rejection(
    ["cargo", "fmt", "--all", "--", "--check"],
    "\npub fn formatting_probe( ){ }\n",
    "Diff in",
)
expect_rejection(
    [
        "cargo", "clippy", "--workspace", "--all-targets", "--all-features",
        "--locked", "--offline", "--", "-D", "warnings",
    ],
    "\npub fn lint_probe(value: bool) -> bool { value == true }\n",
    "bool_comparison",
)
