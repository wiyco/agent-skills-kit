"""Original candidate-archive fixtures; no compiler, host tool or model runs."""

import importlib.util
from pathlib import Path
import stat
import tempfile
import unittest
import zipfile

SPEC = importlib.util.spec_from_file_location(
    "binary_rehearsal", Path(__file__).resolve().parents[1] / "rehearse-binary.py")
binary = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(binary)


class BinaryRehearsalTests(unittest.TestCase):
    def test_only_explicit_native_cpu_os_abi_and_rehearsal_configuration_are_accepted(self):
        config = {"schema_version": 1, "rehearsal_only": True, "targets": {
            "x86_64-pc-windows-msvc": {"archive": "zip", "executable": "agent-skills-kit.exe"}}}
        self.assertEqual(binary.native_target("host: x86_64-pc-windows-msvc", config)[0], "x86_64-pc-windows-msvc")
        with self.assertRaises(ValueError):
            binary.native_target("host: aarch64-pc-windows-msvc", config)
        for key, value in (("rehearsal_only", False), ("schema_version", True), ("schema_version", 2)):
            changed = dict(config, **{key: value})
            with self.assertRaises(ValueError):
                binary.native_target("host: x86_64-pc-windows-msvc", changed)

    def test_unsafe_payload_names_and_unreviewed_executable_paths_are_refused(self):
        for name in ("../escape", "/absolute", "dir/../escape", "dir\\file", "file:stream", ".env", "dir//file", "line\nbreak"):
            with self.assertRaises(ValueError):
                binary.safe_relative(name)
        config = {"schema_version": 1, "rehearsal_only": True, "targets": {
            "x86_64-pc-windows-msvc": {"archive": "zip", "executable": "../escape.exe"}}}
        with self.assertRaises(ValueError):
            binary.native_target("host: x86_64-pc-windows-msvc", config)

    def test_checksum_corruption_is_refused_before_any_extraction(self):
        with tempfile.TemporaryDirectory(prefix="ask-binary-fixture-") as temporary:
            root = Path(temporary)
            artifact = root / "fixture.zip"
            artifact.write_bytes(b"original inert bytes")
            expected = binary.digest(artifact)
            artifact.write_bytes(b"changed inert bytes")
            output = root / "output"
            output.mkdir()
            with self.assertRaisesRegex(ValueError, "checksum"):
                binary.extract_verified(artifact, expected, "zip", "fixture", output)
            self.assertEqual(list(output.iterdir()), [])

    def test_zip_escape_link_and_duplicates_are_rejected_and_regular_bytes_survive(self):
        with tempfile.TemporaryDirectory(prefix="ask-binary-fixture-") as temporary:
            root = Path(temporary)
            for index, name in enumerate(("fixture/../escape", "fixture/.env", "fixture\\escape")):
                artifact = root / f"bad-{index}.zip"
                with zipfile.ZipFile(artifact, "x") as archive:
                    archive.writestr(name, b"inert")
                output = root / f"out-{index}"
                output.mkdir()
                with self.assertRaises(ValueError):
                    binary.extract_verified(artifact, binary.digest(artifact), "zip", "fixture", output)
                self.assertEqual(list(output.iterdir()), [])
            artifact = root / "duplicate.zip"
            with zipfile.ZipFile(artifact, "x") as archive:
                archive.writestr("fixture/README.txt", b"first")
                with self.assertWarns(UserWarning):
                    archive.writestr("fixture/README.txt", b"second")
            output = root / "duplicate-output"
            output.mkdir()
            with self.assertRaises(ValueError):
                binary.extract_verified(artifact, binary.digest(artifact), "zip", "fixture", output)
            self.assertEqual(list(output.iterdir()), [])
            artifact = root / "link.zip"
            with zipfile.ZipFile(artifact, "x") as archive:
                member = zipfile.ZipInfo("fixture/link")
                member.external_attr = (stat.S_IFLNK | 0o777) << 16
                archive.writestr(member, b"../outside")
            with self.assertRaises(ValueError):
                binary.extract_verified(artifact, binary.digest(artifact), "zip", "fixture", root)
            artifact = root / "regular.zip"
            with zipfile.ZipFile(artifact, "x") as archive:
                archive.writestr("fixture/README.txt", b"original inert fixture")
            binary.extract_verified(artifact, binary.digest(artifact), "zip", "fixture", root)
            self.assertEqual((root / "fixture/README.txt").read_bytes(), b"original inert fixture")

class AlphaExportTests(unittest.TestCase):
    def evidence(self, artifact, version="0.1.0-alpha.1"):
        return {"version": version, "artifact_sha256": binary.digest(artifact),
                "target": "aarch64-apple-darwin", "candidate": "1" * 40}

    def test_export_rejects_stable_versions_corruption_checkout_paths_and_existing_outputs(self):
        with tempfile.TemporaryDirectory(prefix="ask-alpha-export-test-") as temporary:
            root = Path(temporary)
            artifact = root / "original.tar.gz"
            artifact.write_bytes(b"original inert artifact fixture")
            for version in ("0.1.0", "0.1.0-beta.1", "0.1.0-alpha.0", "0.1.0-alpha.1/path"):
                with self.assertRaisesRegex(ValueError, "alpha"):
                    binary.export_alpha(artifact, self.evidence(artifact, version), root / "stable-output")
            output = root / "export"
            evidence = self.evidence(artifact)
            artifact.write_bytes(b"corrupted fixture")
            with self.assertRaisesRegex(ValueError, "changed"):
                binary.export_alpha(artifact, evidence, output)
            self.assertFalse(output.exists())
            evidence = self.evidence(artifact)
            with self.assertRaisesRegex(ValueError, "outside"):
                binary.export_alpha(artifact, evidence, binary.ROOT / "never-create-alpha-test")
            output.mkdir()
            sentinel = output / "sentinel"
            sentinel.write_bytes(b"preserve existing data")
            with self.assertRaises(FileExistsError):
                binary.export_alpha(artifact, evidence, output)
            self.assertEqual(sentinel.read_bytes(), b"preserve existing data")

    def test_verified_export_keeps_exact_archive_checksum_and_candidate_evidence(self):
        import json
        with tempfile.TemporaryDirectory(prefix="ask-alpha-export-test-") as temporary:
            root = Path(temporary)
            artifact = root / "fixture.tar.gz"
            artifact.write_bytes(b"original inert verified archive")
            output = root / "new-output"
            evidence = self.evidence(artifact)
            binary.export_alpha(artifact, evidence, output)
            self.assertEqual((output / artifact.name).read_bytes(), artifact.read_bytes())
            self.assertEqual((output / (artifact.name + ".sha256")).read_text(),
                             evidence["artifact_sha256"] + "  " + artifact.name + "\n")
            record = json.loads((output / "build-evidence-aarch64-apple-darwin.json").read_text())
            self.assertEqual(record, evidence)
            self.assertEqual(len(list(output.iterdir())), 3)


class BinaryPrivacyTests(unittest.TestCase):
    def test_known_absolute_prefixes_are_rejected_in_native_and_encoded_forms(self):
        with tempfile.TemporaryDirectory(prefix="ask-binary-privacy-") as temporary:
            binary_path = Path(temporary) / "inert-binary"
            for text, encoding in (("/home/original-user/.rustup/toolchain", "utf-8"),
                                   ("C:\\Users\\Original-User\\.rustup", "utf-16le"),
                                   ("C:/Users/Original-User/.rustup", "utf-8")):
                binary_path.write_bytes(b"inert" + text.encode(encoding))
                with self.assertRaisesRegex(ValueError, "absolute"):
                    binary.reject_build_prefixes(binary_path, ["/home/original-user", "C:\\Users\\Original-User"])
            binary_path.write_bytes(b"inert /ask-rehearsal/source /rustc/official-source")
            binary.reject_build_prefixes(binary_path, ["/home/original-user", "C:\\Users\\Original-User"])


if __name__ == "__main__":
    unittest.main()
