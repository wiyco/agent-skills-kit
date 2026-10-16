"""Host-independent safety regressions; never launch a real host in PR CI."""

import importlib.util
import os
from pathlib import Path
import unittest
from unittest.mock import MagicMock, patch

SPEC = importlib.util.spec_from_file_location(
    "smoke_codex", Path(__file__).resolve().parents[1] / "smoke_codex.py")
smoke = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(smoke)


class HostSmokeTests(unittest.TestCase):
    def test_child_environment_does_not_inherit_secrets_or_mutate_parent(self):
        before = dict(os.environ)
        environment = smoke.child_environment(Path("/owned-fixture"))
        self.assertEqual(set(environment), {"PATH", "HOME", "CODEX_HOME", "XDG_CONFIG_HOME",
                         "XDG_DATA_HOME", "XDG_CACHE_HOME", "XDG_STATE_HOME", "TMPDIR",
                         "TERM", "LANG"})
        self.assertEqual(environment["HOME"], "/owned-fixture/home")
        self.assertEqual(environment["CODEX_HOME"], "/owned-fixture/state")
        self.assertEqual(dict(os.environ), before)

    def test_isolation_rejects_overlapping_or_relative_roots(self):
        for root, home in (("relative", "/home/user"), ("/tmp/test", "relative"),
                           ("/home/user", "/home/user"), ("/home/user/fixture", "/home/user"),
                           ("/", "/home/user")):
            with self.assertRaises(ValueError):
                smoke.isolation_profile(Path(root), Path(home))
        profile = smoke.isolation_profile(Path('/tmp/owned-"fixture'), Path("/home/user"))
        self.assertIn('(deny network*)', profile)
        self.assertIn('(deny file-write*)', profile)
        self.assertIn('(deny file-read* (subpath "/home/user"))', profile)
        self.assertIn('owned-\\"fixture', profile)

    def test_unreviewed_platform_fails_before_launching_or_creating_state(self):
        with patch.object(smoke.platform, "system", return_value="Unreviewed"), \
                patch.object(smoke.subprocess, "Popen") as launch, \
                patch.object(smoke.tempfile, "TemporaryDirectory") as temporary:
            with self.assertRaisesRegex(ValueError, "no fallback"):
                smoke.smoke(Path("/host"), Path("/installer"), "project")
            launch.assert_not_called()
            temporary.assert_not_called()

    def test_rpc_errors_closed_stream_and_deadline_never_become_a_passing_result(self):
        process, selector = MagicMock(), MagicMock()
        with self.assertRaisesRegex(RuntimeError, "rejected"):
            smoke.receive(process, selector, b'{"id":1,"error":{"code":-1}}\n', 1)
        with patch.object(smoke.os, "read", return_value=b""):
            selector.select.return_value = [(1, 1)]
            with self.assertRaisesRegex(RuntimeError, "closed stdout"):
                smoke.receive(process, selector, b"", 1)
        with patch.object(smoke.time, "monotonic", side_effect=[0, 21]):
            with self.assertRaises(TimeoutError):
                smoke.receive(process, selector, b"", 1)
        result, remainder = smoke.receive(process, selector,
            b'{"method":"fixture-notification"}\n{"id":1,"result":{"data":[]}}\nnext', 1)
        self.assertEqual(result, {"data": []})
        self.assertEqual(remainder, b"next")


if __name__ == "__main__":
    unittest.main()
