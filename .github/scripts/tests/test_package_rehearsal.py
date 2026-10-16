"""Original archive-boundary fixtures, without Cargo, extraction or network."""

import importlib.util
from pathlib import Path
import tarfile
import unittest
from unittest.mock import MagicMock, patch

SPEC = importlib.util.spec_from_file_location(
    "package_rehearsal", Path(__file__).resolve().parents[1] / "rehearse-library.py")
rehearsal = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(rehearsal)
PREFIX = "agent-skills-kit-0.1.0"


def entry(path, kind=tarfile.REGTYPE, size=1):
    member = tarfile.TarInfo(path)
    member.type, member.size = kind, size
    return member


class PackageRehearsalTests(unittest.TestCase):
    def inspect(self, members):
        archive = MagicMock()
        archive.getmembers.return_value = members
        return rehearsal.inspect_archive(archive, PREFIX)

    def test_valid_generated_vcs_metadata_and_regular_sources_are_inspectable(self):
        members = [entry(PREFIX + "/.cargo_vcs_info.json"), entry(PREFIX + "/src/lib.rs")]
        self.assertEqual(self.inspect(members), members)

    def test_links_special_entries_escape_hidden_and_alias_paths_are_rejected(self):
        for kind in (tarfile.SYMTYPE, tarfile.LNKTYPE, tarfile.FIFOTYPE, tarfile.DIRTYPE):
            with self.assertRaises(ValueError):
                self.inspect([entry(PREFIX + "/src/lib.rs", kind)])
        for path in ("/absolute/file", "elsewhere/src/lib.rs", PREFIX + "/../escape",
                     PREFIX + "/.env", PREFIX + "/.cargo/credentials.toml", PREFIX + "/src/./lib.rs",
                     PREFIX + "//src/lib.rs", PREFIX + "/src\\lib.rs", PREFIX + "/file:stream",
                     PREFIX + "/line\nbreak"):
            with self.subTest(path=path), self.assertRaises(ValueError):
                self.inspect([entry(path)])
        with self.assertRaises(ValueError):
            self.inspect([entry(PREFIX + "/src/lib.rs")] * 2)

    def test_size_count_and_unreviewed_toolchain_fail_before_execution(self):
        with self.assertRaises(ValueError):
            self.inspect([entry(PREFIX + "/src/lib.rs", size=16 * 1024 * 1024 + 1)])
        with self.assertRaises(ValueError):
            self.inspect([entry(PREFIX + f"/src/{index}.rs") for index in range(1025)])
        with patch.object(rehearsal.subprocess, "run") as launch:
            with self.assertRaises(ValueError):
                rehearsal.rehearse("unreviewed-toolchain")
            launch.assert_not_called()


if __name__ == "__main__":
    unittest.main()
