"""Build/check a native candidate in owned state; no hosting or public installer."""

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


def rehearse():
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
        cargo_home = Path(environment.get("CARGO_HOME", str(Path.home() / ".cargo"))).resolve()
        environment["CARGO_ENCODED_RUSTFLAGS"] = "\x1f".join([
            "--remap-path-prefix=" + str(root) + "=/ask-rehearsal",
            "--remap-path-prefix=" + str(cargo_home) + "=/cargo-home"])
        rustc = subprocess.run([compiler, "-Vv"], cwd=snapshot, env=environment,
                               check=True, capture_output=True, text=True, encoding="utf-8").stdout.strip()
        target, settings = native_target(rustc, json.loads((snapshot / ".github/binary-rehearsal.json").read_text(encoding="utf-8")))

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
        shutil.copyfile(binary, bundle / settings["executable"])
        for name in ("LICENSE-MIT", "LICENSE-APACHE"):
            shutil.copyfile(snapshot / name, bundle / name)
        components = notice_inventory(metadata, tomllib.loads((snapshot / "Cargo.lock").read_text(encoding="utf-8")), snapshot, bundle)
        inventory = {"schema_version": 1, "rehearsal_only": True, "candidate": revision, "target": target,
                     "version": workspace["version"], "rustc": rustc, "components": components,
                     "binary_sha256": digest(binary), "os": platform.system(), "os_release": platform.release(),
                     "os_version": platform.version(), "libc": list(platform.libc_ver())}
        (bundle / "component-inventory.json").write_text(json.dumps(inventory, sort_keys=True, indent=2) + "\n", encoding="utf-8")
        (bundle / "README.txt").write_text("WIP rehearsal only; not a published release or supported installation.\n"
            "No installer, updater, signing, notarization, SBOM certification or provenance attestation is provided.\n"
            "Keep notices with the binary. See component-inventory.json and the repository release gates.\n", encoding="utf-8")
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
        return {"candidate": revision, "target": target, "toolchain": rustc, "rehearsal_only": True,
                "artifact": artifact.name, "artifact_sha256": checksum, "artifact_bytes": artifact.stat().st_size,
                "binary_sha256": inventory["binary_sha256"], "components": len(components), "files": len(files),
                "os": inventory["os"], "os_release": inventory["os_release"], "os_version": inventory["os_version"],
                "libc": inventory["libc"], "result": "optimized tests, archive/checksums/notices and isolated help/version passed"}


if __name__ == "__main__":
    print(json.dumps(rehearse(), sort_keys=True))
