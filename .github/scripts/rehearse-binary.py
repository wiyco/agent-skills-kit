"""Build/check a native candidate in owned state; no hosting or public installer."""

import argparse
import gzip
import hashlib
import importlib.util
import json
import os
from pathlib import Path, PurePosixPath
import platform
import re
import shutil
import stat
import subprocess
import tarfile
import tempfile
import tomllib
import zipfile

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location("library_rehearsal", Path(__file__).with_name("rehearse-library.py"))
library_rehearsal = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(library_rehearsal)


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def safe_relative(name):
    path = PurePosixPath(name)
    if (path.is_absolute() or not path.parts or ".." in path.parts or name != path.as_posix()
            or "\\" in name or ":" in name or any(part.startswith(".") for part in path.parts)
            or any(ord(char) < 32 or ord(char) == 127 for char in name)):
        raise ValueError("unsafe artifact-relative name")
    return path


def native_target(rustc, configuration):
    target = next(line.removeprefix("host: ") for line in rustc.splitlines() if line.startswith("host: "))
    if (type(configuration.get("schema_version")) is not int or configuration["schema_version"] != 1
            or configuration.get("rehearsal_only") is not True):
        raise ValueError("unsupported or publishing-enabled binary configuration")
    if target not in configuration["targets"]:
        raise ValueError("native CPU/OS/ABI is not in the measured rehearsal target set")
    settings = configuration["targets"][target]
    expected = "agent-skills-kit.exe" if "windows" in target else "agent-skills-kit"
    if settings["executable"] != expected or settings["archive"] not in ("tar.gz", "zip"):
        raise ValueError("unreviewed binary filename or archive format")
    return target, settings


def notice_inventory(metadata, lock, snapshot, bundle):
    packages = {package["id"]: package for package in metadata["packages"]}
    nodes = {node["id"]: node for node in metadata["resolve"]["nodes"]}
    cli = next(package for package in packages.values() if package["name"] == "agent-skills-kit-cli")
    library = next(package for package in packages.values() if package["name"] == "agent-skills-kit")
    if (Path(library["manifest_path"]).resolve() != (snapshot / "crates/agent-skills-kit/Cargo.toml").resolve()
            or cli["version"] != library["version"]
            or not any(edge["pkg"] == library["id"] for edge in nodes[cli["id"]]["deps"])):
        raise ValueError("CLI does not use the same reviewed local library candidate")
    checksums = {(package["name"], package["version"]): package.get("checksum") for package in lock["package"]}
    pending, visited, components = [cli["id"]], set(), []
    while pending:
        identifier = pending.pop()
        if identifier in visited:
            continue
        visited.add(identifier)
        pending.extend(edge["pkg"] for edge in nodes[identifier]["deps"]
                       if any(kind["kind"] != "dev" for kind in edge["dep_kinds"]))
        package = packages[identifier]
        component = {"name": package["name"], "version": package["version"],
                     "declared_license": package["license"], "repository": package["repository"], "notices": []}
        if package["source"] is None:
            if package["name"] not in ("agent-skills-kit", "agent-skills-kit-cli"):
                raise ValueError("unreviewed additional workspace component")
            component["source"] = "same committed project snapshot"
        else:
            component["source"] = package["source"]
            component["crate_checksum"] = checksums[(package["name"], package["version"])]
            if not component["crate_checksum"]:
                raise ValueError("registry component has no locked checksum")
            label = package["name"] + "-" + package["version"]
            if not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._+-]*", label):
                raise ValueError("unsupported component filename")
            source = Path(package["manifest_path"]).parent
            files = [path for path in source.iterdir()
                     if path.name.lower().startswith(("license", "copying", "notice", "copyright"))]
            additional = source / ".licenses"
            if additional.is_symlink():
                raise ValueError("linked notice directory")
            if additional.is_dir():
                files.extend(additional.iterdir())
            if not files:
                raise ValueError("component notice files require manual review")
            for path in sorted(files):
                if path.is_symlink() or not path.is_file() or path.stat().st_size > 1024 * 1024:
                    raise ValueError("unsupported notice entry")
                relative = path.relative_to(source).as_posix()
                mapped = "notice-files/" + path.name if path.parent == additional else path.name
                destination = "third-party/" + label + "/" + mapped
                safe_relative(destination)
                output = bundle.joinpath(*PurePosixPath(destination).parts)
                output.parent.mkdir(parents=True, exist_ok=True)
                with output.open("xb") as target:
                    target.write(path.read_bytes())
                component["notices"].append({"source_file": relative, "bundle_file": destination, "sha256": digest(output)})
        components.append(component)
    return sorted(components, key=lambda component: (component["name"], component["version"]))


def extract_verified(artifact, expected_sha256, archive_kind, prefix, destination):
    if digest(artifact) != expected_sha256:
        raise ValueError("archive checksum mismatch before extraction")
    if archive_kind == "tar.gz":
        with tarfile.open(artifact, "r:gz") as archive:
            members = library_rehearsal.inspect_archive(archive, prefix)
            for member in members:
                path = destination.joinpath(*PurePosixPath(member.name).parts)
                path.parent.mkdir(parents=True, exist_ok=True)
                with archive.extractfile(member) as source, path.open("xb") as output:
                    shutil.copyfileobj(source, output)
    elif archive_kind == "zip":
        with zipfile.ZipFile(artifact) as archive:
            entries, members = archive.infolist(), []
            for entry in entries:
                mode = entry.external_attr >> 16
                if entry.is_dir() or stat.S_IFMT(mode) not in (0, stat.S_IFREG):
                    raise ValueError("non-regular zip entry")
                member = tarfile.TarInfo(entry.filename)
                member.size = entry.file_size
                members.append(member)
            library_rehearsal.inspect_members(members, prefix)
            for entry in entries:
                path = destination.joinpath(*PurePosixPath(entry.filename).parts)
                path.parent.mkdir(parents=True, exist_ok=True)
                with archive.open(entry) as source, path.open("xb") as output:
                    shutil.copyfileobj(source, output)
    else:
        raise ValueError("unreviewed archive format")


def manager_smoke(executable, root, environment):
    """Run the actual unpacked binary against original, owned inert skill data."""
    home = root / "manager-smoke" / "home"
    source = home / ".agents/skills/sample"
    source.mkdir(parents=True)
    metadata = b"---\nname: sample\ndescription: Original inert packaged CLI fixture.\n---\nNever execute.\n"
    (source / "SKILL.md").write_bytes(metadata)
    resource = source / "original.txt"
    resource.write_bytes(b"original inert resource")
    sentinel = home / "unrelated-sentinel"
    sentinel.write_bytes(b"unrelated original data")
    results = []

    def command(scenario, *args, expected=0):
        result = subprocess.run([str(executable), *map(str, args), "--json"],
                                cwd=home, env=environment, capture_output=True,
                                text=True, encoding="utf-8", timeout=30)
        if result.returncode != expected or result.stderr:
            raise ValueError(f"packaged manager {scenario} failed: {result.returncode}: {result.stdout}: {result.stderr}")
        value = json.loads(result.stdout)
        if value["schema_version"] != 1:
            raise ValueError("unexpected packaged CLI envelope")
        results.append({"scenario": scenario, "exit": result.returncode, "status": value["status"]})
        return value["data"] if "data" in value else value

    def selection(agent):
        return ["--from", agent, "--scope", "user", "--user-home", home]

    listed = command("list-existing", "skills", "list", *selection("codex"))
    location = next(row["number"] for row in listed["locations"] if row["preferred_write"])
    copy = ["skills", "copy", "sample", "--location", location, "--target", "claude-code", *selection("codex")]
    command("copy-approval-required", *copy, expected=3)
    command("copy-dry-run", *copy, "--dry-run")
    destination = home / ".claude/skills/sample"
    if destination.exists():
        raise ValueError("unapproved packaged copy wrote data")
    command("copy-approved", *copy, "--yes")
    command("copy-unchanged", *copy, "--yes")
    if (destination / "SKILL.md").read_bytes() != metadata or (destination / "original.txt").read_bytes() != resource.read_bytes():
        raise ValueError("packaged copy changed original bytes")
    delete = ["skills", "delete", "sample", "--location", location, *selection("codex")]
    preview = command("delete-preview", *delete, expected=3)
    if (preview["apply_attempted"] or len(preview["entries"]) != 2 or not preview["known_readers"]
            or not preview["unresolved_readers"] or preview["exhaustive_host_discovery"]):
        raise ValueError("packaged deletion preview omitted contents/impact")
    command("delete-dry-run", *delete, "--dry-run")
    command("delete-generic-yes-refused", *delete, "--yes", expected=2)
    command("delete-wrong-confirmation", *delete, "--confirm-delete", "wrong", expected=1)
    resource.write_bytes(b"edited since preview")
    command("delete-stale-confirmation", *delete, "--confirm-delete", preview["confirmation_token"], expected=1)
    if not (source / "SKILL.md").is_file() or resource.read_bytes() != b"edited since preview":
        raise ValueError("stale packaged deletion modified the source")
    preview = command("delete-fresh-preview", *delete, expected=3)
    report = command("delete-approved", *delete, "--confirm-delete", preview["confirmation_token"])["report"]
    if not report["successful"] or not all(row["state"] == "removed" for row in report["outcomes"]):
        raise ValueError("packaged deletion did not confirm all removals")
    if source.exists() or list(source.parent.iterdir()):
        raise ValueError("packaged deletion retained a payload/enclosure")
    if (destination / "original.txt").read_bytes() != b"original inert resource":
        raise ValueError("deleting shared source changed the separate copy")
    command("list-after-removal", "skills", "list", *selection("codex"))
    if sentinel.read_bytes() != b"unrelated original data":
        raise ValueError("packaged manager changed unrelated data")
    return results


def reject_build_prefixes(binary, prefixes):
    """Bounded privacy regression check, not an independent secrets audit."""
    data = binary.read_bytes().lower()
    for prefix in prefixes:
        for text in {str(prefix), str(prefix).replace("\\", "/"), str(prefix).replace("/", "\\")}:
            for encoding in ("utf-8", "utf-16le"):
                if text.encode(encoding).lower() in data:
                    raise ValueError("known absolute home/build/cache prefix remains in the native binary")


def process_native_binary(binary, target, environment):
    if target == "aarch64-apple-darwin":
        # Use the already-required Apple SDK, not an inferred rust-objcopy path
        # or newly installed tool. Remove debug/STABS archive paths only.
        sdk_strip = subprocess.run(["/usr/bin/xcrun", "--find", "strip"], env=environment,
                                   check=True, capture_output=True, text=True, timeout=10).stdout.strip()
        subprocess.run([sdk_strip, "-S", str(binary)], env=environment, check=True,
                       capture_output=True, text=True, timeout=30)
        sdk = subprocess.run(["/usr/bin/xcrun", "--show-sdk-version"], env=environment,
                             check=True, capture_output=True, text=True, timeout=10).stdout.strip()
        return {"operation": "Apple SDK strip -S (debug/STABS only)", "sdk_version": sdk}
    if target == "x86_64-pc-windows-msvc":
        return {"operation": "MSVC link /PDBALTPATH:agent-skills-kit.pdb; no PDB distributed"}
    return {"operation": "unstripped; compiler source/build/cache prefix remapping"}


def export_alpha(artifact, evidence, output):
    """Export only a verified experimental bundle to a new caller-owned directory."""
    if not re.fullmatch(r"0\.1\.0-alpha\.[1-9][0-9]*", evidence["version"]):
        raise ValueError("export requires an explicitly experimental alpha version")
    if digest(artifact) != evidence["artifact_sha256"]:
        raise ValueError("archive changed before export")
    output = Path(output).resolve()
    if output == ROOT or ROOT in output.parents:
        raise ValueError("export must be outside the source checkout")
    output.mkdir(mode=0o700)
    shutil.copyfile(artifact, output / artifact.name)
    (output / (artifact.name + ".sha256")).write_text(evidence["artifact_sha256"] + "  " + artifact.name + "\n", encoding="ascii")
    (output / ("build-evidence-" + evidence["target"] + ".json")).write_text(json.dumps(evidence, sort_keys=True, indent=2) + "\n", encoding="utf-8")
    if digest(output / artifact.name) != evidence["artifact_sha256"]:
        raise ValueError("exported archive checksum mismatch")


def rehearse(alpha_output=None):
    pin = tomllib.loads((ROOT / "rust-toolchain.toml").read_text(encoding="utf-8"))["toolchain"]["channel"]
    environment = dict(os.environ, RUSTUP_TOOLCHAIN=pin, GIT_CONFIG_GLOBAL=os.devnull,
                       GIT_CONFIG_NOSYSTEM="1", GIT_TERMINAL_PROMPT="0")
    # Resolve existing official rustup toolchain executables; this command does
    # not install a missing pin or depend on a user compiler wrapper.
    def executable(name):
        return subprocess.run(["rustup", "which", "--toolchain", pin, name], check=True,
                              capture_output=True, text=True, encoding="utf-8", timeout=10).stdout.strip()

    compiler, cargo_tool = executable("rustc"), executable("cargo")
    environment["RUSTC"] = compiler
    for variable in ("RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER", "CARGO_BUILD_RUSTC_WRAPPER", "CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER"):
        environment.pop(variable, None)

    def git(*args, cwd=ROOT):
        return subprocess.run(["git", "-c", "core.hooksPath=" + os.devnull, "-c", "init.templateDir=", *args],
                              cwd=cwd, env=environment, check=True, capture_output=True, text=True, encoding="utf-8", timeout=30).stdout.strip()

    if git("status", "--porcelain", "--untracked-files=no"):
        raise ValueError("commit tracked changes before rehearsing")
    revision = git("rev-parse", "HEAD")
    with tempfile.TemporaryDirectory(prefix="ask-native-binary-") as temporary:
        root = Path(temporary).resolve()
        snapshot = root / "snapshot"
        git("clone", "--quiet", "--no-hardlinks", "--no-checkout", str(ROOT), str(snapshot))
        git("checkout", "--quiet", "--detach", revision, cwd=snapshot)
        workspace = tomllib.loads((snapshot / "Cargo.toml").read_text(encoding="utf-8"))["workspace"]["package"]
        if workspace["publish"] is not False:
            raise ValueError("development publish=false gate must remain in place")
        environment["CARGO_TARGET_DIR"] = str(root / "build")
        # Disable Cargo optional strip-tool inference; reviewed native processing
        # below removes debug path leakage before hashing/packaging/smoke tests.
        environment["CARGO_PROFILE_RELEASE_STRIP"] = "none"
        cargo_home = Path(environment.get("CARGO_HOME", str(Path.home() / ".cargo"))).resolve()
        environment["CARGO_ENCODED_RUSTFLAGS"] = "\x1f".join([
            "--remap-path-prefix=" + str(root) + "=/ask-rehearsal",
            "--remap-path-prefix=" + str(cargo_home) + "=/cargo-home"])
        rustc = subprocess.run([compiler, "-Vv"], cwd=snapshot, env=environment,
                               check=True, capture_output=True, text=True, encoding="utf-8").stdout.strip()
        target, settings = native_target(rustc, json.loads((snapshot / ".github/binary-rehearsal.json").read_text(encoding="utf-8")))

        if target == "x86_64-pc-windows-msvc":
            environment["CARGO_ENCODED_RUSTFLAGS"] += "\x1f-Clink-arg=/PDBALTPATH:agent-skills-kit.pdb"

        def cargo(*args, capture=False):
            return subprocess.run([cargo_tool, *args], cwd=snapshot, env=environment,
                                  check=True, capture_output=capture, text=True, encoding="utf-8", timeout=600)

        cargo("build", "-p", "agent-skills-kit-cli", "--bin", "agent-skills-kit", "--release", "--target", target, "--locked", "--offline")
        cargo("test", "--workspace", "--all-features", "--release", "--target", target, "--locked", "--offline")
        metadata = json.loads(cargo("metadata", "--format-version", "1", "--filter-platform", target, "--locked", "--offline", capture=True).stdout)
        prefix = f"agent-skills-kit-{workspace['version']}-{target}-{revision[:12]}"
        safe_relative(prefix)
        bundle = root / prefix
        bundle.mkdir()
        binary = root / "build" / target / "release" / settings["executable"]
        if binary.is_symlink() or not binary.is_file():
            raise ValueError("build output is not a regular native executable")
        bundled_binary = bundle / settings["executable"]
        shutil.copyfile(binary, bundled_binary)
        processing = process_native_binary(bundled_binary, target, environment)
        reject_build_prefixes(bundled_binary, [root, cargo_home, Path.home()])
        for name in ("LICENSE-MIT", "LICENSE-APACHE"):
            shutil.copyfile(snapshot / name, bundle / name)
        components = notice_inventory(metadata, tomllib.loads((snapshot / "Cargo.lock").read_text(encoding="utf-8")), snapshot, bundle)
        inventory = {"schema_version": 1, "rehearsal_only": alpha_output is None,
                     "channel": "rehearsal" if alpha_output is None else "experimental-alpha", "candidate": revision, "target": target,
                     "version": workspace["version"], "rustc": rustc, "components": components,
                     "binary_sha256": digest(bundled_binary), "binary_processing": processing,
                     "known_absolute_prefix_scan": "no matches; bounded home/build/cache UTF-8/UTF-16 check", "os": platform.system(), "os_release": platform.release(),
                     "os_version": platform.version(), "libc": list(platform.libc_ver())}
        (bundle / "component-inventory.json").write_text(json.dumps(inventory, sort_keys=True, indent=2) + "\n", encoding="utf-8")
        bundle_readme = ("WIP rehearsal only; not a published release or supported installation.\n" if alpha_output is None else
                         "Experimental CLI alpha; not ready for production or real skill stores. The embedded Rust library is unpublished.\n")
        bundle_readme += ("Permanent deletion has no undo. Preview all contents/shared readers, stop skill writers and explicitly confirm.\n"
            "No installer, updater, signing, notarization, certified SBOM or provenance attestation is provided.\n"
            "Confidential security/conduct and independent appeal routes remain unavailable; do not disclose sensitive details publicly.\n"
            "No supported-version response commitment or public contribution intake is offered.\n"
            "Use only owned temporary fixtures. Keep notices with the binary. See component-inventory.json.\n"
            "Source and current limitations: https://github.com/wiyco/agent-skills-kit\n")
        (bundle / "README.txt").write_text(bundle_readme, encoding="utf-8")
        files = sorted(path for path in bundle.rglob("*") if path.is_file())
        artifact = root / (prefix + "." + settings["archive"])
        if settings["archive"] == "tar.gz":
            with artifact.open("xb") as output, gzip.GzipFile(filename="", mode="wb", fileobj=output, mtime=0) as compressed, \
                    tarfile.open(fileobj=compressed, mode="w") as archive:
                for path in files:
                    member = tarfile.TarInfo(prefix + "/" + path.relative_to(bundle).as_posix())
                    member.size = path.stat().st_size
                    member.mode = 0o755 if path.name == settings["executable"] else 0o644
                    with path.open("rb") as source:
                        archive.addfile(member, source)
        elif settings["archive"] == "zip":
            with zipfile.ZipFile(artifact, "x", compression=zipfile.ZIP_DEFLATED) as archive:
                for path in files:
                    entry = zipfile.ZipInfo(prefix + "/" + path.relative_to(bundle).as_posix(), date_time=(1980, 1, 1, 0, 0, 0))
                    entry.external_attr = (stat.S_IFREG | 0o644) << 16
                    entry.compress_type = zipfile.ZIP_DEFLATED
                    archive.writestr(entry, path.read_bytes())
        else:
            raise ValueError("unreviewed archive format")
        checksum = digest(artifact)
        checksum_file = root / "SHA256SUMS"
        checksum_file.write_text(checksum + "  " + artifact.name + "\n", encoding="ascii")
        declared_checksum, declared_name = checksum_file.read_text(encoding="ascii").strip().split("  ", 1)
        if declared_name != artifact.name:
            raise ValueError("checksum file does not identify the built archive")
        unpack = root / "unpack"
        unpack.mkdir()
        extract_verified(artifact, declared_checksum, settings["archive"], prefix, unpack)
        for path in files:
            if digest(path) != digest(unpack / prefix / path.relative_to(bundle)):
                raise ValueError("unpacked artifact content mismatch")
        install = root / "installation/bin"
        install.mkdir(parents=True)
        executable = install / settings["executable"]
        shutil.copyfile(unpack / prefix / settings["executable"], executable)
        executable.chmod(0o755)
        version = subprocess.run([str(executable), "--version"], cwd=install.parent, env=environment,
                                 check=True, capture_output=True, text=True, encoding="utf-8", timeout=10)
        help_result = subprocess.run([str(executable), "--help"], cwd=install.parent, env=environment,
                                     check=True, capture_output=True, text=True, encoding="utf-8", timeout=10)
        if (version.stdout.strip() != "agent-skills-kit " + workspace["version"] or version.stderr
                or "targets list" not in help_result.stdout or help_result.stderr
                or digest(executable) != inventory["binary_sha256"]):
            raise ValueError("installed binary smoke or identity check failed")
        scenarios = manager_smoke(executable, root, environment)
        evidence = {"candidate": revision, "target": target, "toolchain": rustc,
                "version": workspace["version"], "rehearsal_only": alpha_output is None,
                "channel": "rehearsal" if alpha_output is None else "experimental-alpha",
                "artifact": artifact.name, "artifact_sha256": checksum, "artifact_bytes": artifact.stat().st_size,
                "binary_sha256": inventory["binary_sha256"], "binary_processing": processing,
                "known_absolute_prefix_scan": inventory["known_absolute_prefix_scan"], "components": len(components), "files": len(files),
                "os": inventory["os"], "os_release": inventory["os_release"], "os_version": inventory["os_version"],
                "libc": inventory["libc"], "manager_smoke": scenarios,
                "result": "optimized tests, archive/checksums/notices, isolated help/version and actual packaged list/copy/delete passed"}
        if alpha_output is not None:
            export_alpha(artifact, evidence, alpha_output)
        return evidence


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--export-alpha", type=Path, help="export a verified alpha to a new external directory; never publishes")
    options = parser.parse_args()
    print(json.dumps(rehearse(options.export_alpha), sort_keys=True))
