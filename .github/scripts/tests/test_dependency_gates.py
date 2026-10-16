"""Controlled gate classification only; no advisory fetch or dependency install."""

import importlib.util
from pathlib import Path
import subprocess
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location(
    "dependency_gates", Path(__file__).resolve().parents[1] / "verify-dependencies.py")
gates = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(gates)


class DependencyGateTests(unittest.TestCase):
    def test_negative_check_cannot_pass_on_success_panic_or_unrelated_failure(self):
        for code, diagnostic in ((0, "error[rejected]"), (101, "error[rejected]"),
                                 (4, "configuration could not be read"), (1, "error[rejected]")):
            with patch.object(gates.subprocess, "run", return_value=
                              subprocess.CompletedProcess([], code, "", diagnostic)):
                with self.assertRaises(RuntimeError):
                    gates.rejection(["fixture-only"], "error[rejected]", 4)

    def test_expected_policy_diagnostic_and_exact_exit_are_both_required(self):
        with patch.object(gates.subprocess, "run", return_value=
                          subprocess.CompletedProcess([], 4, "", "error[rejected]: synthetic fixture")):
            gates.rejection(["fixture-only"], "error[rejected]", 4)


if __name__ == "__main__":
    unittest.main()
