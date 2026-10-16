"""Original synthetic evidence fixtures; no network, skills or user stores."""

import copy
from datetime import date
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

SCRIPT = Path(__file__).resolve().parents[1] / "catalog_drift.py"
SPEC = importlib.util.spec_from_file_location("catalog_drift", SCRIPT)
audit = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(audit)
AS_OF = date(2026, 10, 16)
URL = "https://docs.example.invalid/skills/"


def rule(accessed="2026-10-16"):
    return {"path": {"base": {"kind": "project"}, "components": [".example", "skills"]},
            "evidence": {"documentation": "reviewed", "source": {
                "url": URL, "section": "Original synthetic fixture", "accessed": accessed,
                "applicability": "Fictional host; never a real installation rule"},
                "resolver_tests": ["fixture::recorded_reference"], "discovery_tests": []}}


def fixture():
    return {"schema_version": 1, "targets": [
        {"id": "example-cli", "product_id": "example", "variant": "cli", "aliases": ["old-example"],
         "scopes": [{"scope": "project", "status": "documented", "preferred_write": rule("2026-07-18"),
                     "discovery": [{"location": rule(), "traversal": "exact"}]}]},
        {"id": "example-ide", "product_id": "example", "variant": "ide", "aliases": [],
         "scopes": [{"scope": "project", "status": "configuration-required", "preferred_write": rule(), "discovery": []}]},
        {"id": "example-web", "product_id": "example-web", "variant": "web", "aliases": [],
         "scopes": [{"scope": "user", "status": "import-only", "preferred_write": None, "discovery": []}]},
        {"id": "example-fork", "product_id": "example-fork", "variant": "cli", "aliases": [],
         "scopes": [{"scope": "project", "status": "unsupported", "preferred_write": None, "discovery": []}]},
    ]}


class EvidenceAuditTests(unittest.TestCase):
    def test_age_boundary_future_leap_and_missing_dates(self):
        self.assertEqual(audit.evidence_age("2026-07-19", AS_OF, 90), {"state": "within_window", "days": 89})
        self.assertEqual(audit.evidence_age("2026-07-18", AS_OF, 90), {"state": "stale", "days": 90})
        self.assertEqual(audit.evidence_age("2026-10-17", AS_OF, 90), {"state": "future", "days": -1})
        self.assertEqual(audit.evidence_age("2024-02-29", date(2024, 3, 1), 90)["days"], 1)
        for value in (None, ""):
            self.assertEqual(audit.evidence_age(value, AS_OF, 90)["state"], "missing")
        for value in ("2026-02-29", "20261016", "2026-W42-5", False, 0, [], "not a date"):
            self.assertEqual(audit.evidence_age(value, AS_OF, 90)["state"], "invalid")

    def test_groups_url_references_without_losing_old_rule_or_variant(self):
        report = audit.build_report(fixture(), AS_OF)
        self.assertEqual(len(report["sources"]), 1)
        references = report["sources"][0]["references"]
        self.assertEqual(len(references), 3)
        self.assertEqual(sorted(row["age"]["state"] for row in references), ["stale", "within_window", "within_window"])
        products = {row["product_id"]: row["variants"] for row in report["products"]}
        self.assertEqual(len(products), 3)
        self.assertEqual([row["id"] for row in products["example"]], ["example-cli", "example-ide"])
        self.assertEqual(products["example"][0]["aliases"], ["old-example"])
        self.assertEqual(products["example-web"][0]["scopes"][0]["status"], "import-only")
        self.assertEqual(products["example-fork"][0]["scopes"][0]["status"], "unsupported")
        self.assertEqual(report["sources"][0]["probe"], {"state": "not_run"})
        self.assertEqual(report["path_correctness"], "not_assessed")
        self.assertTrue(report["human_review_required_before_catalog_changes"])
        self.assertEqual(len([gap for gap in report["gaps"] if gap["kind"] == "no_rule_evidence"]), 2)

    def test_deterministic_read_only_metadata_with_no_network(self):
        document = fixture()
        before = copy.deepcopy(document)
        with patch("socket.socket", side_effect=AssertionError("network forbidden")):
            first = audit.build_report(document, AS_OF)
            document["targets"].reverse()
            second = audit.build_report(document, AS_OF)
        self.assertEqual(first, second)
        document["targets"].reverse()
        self.assertEqual(document, before)

    def test_missing_sources_dates_sections_and_bad_reference_types_are_gaps(self):
        document = fixture()
        scope = document["targets"][0]["scopes"][0]
        scope["preferred_write"]["evidence"]["source"] = None
        reader = scope["discovery"][0]["location"]["evidence"]
        reader["documentation"] = "unverified"
        reader["source"]["accessed"] = "invalid"
        reader["source"]["section"] = ""
        reader["source"].pop("applicability")
        reader["resolver_tests"] = "not an array of references"
        kinds = {gap["kind"] for gap in audit.build_report(document, AS_OF)["gaps"]}
        self.assertTrue({"missing_source", "accessed_invalid", "missing_section", "missing_applicability", "invalid_resolver_tests", "documentation_not_reviewed"} <= kinds)

    def test_malformed_urls_are_not_echoed_or_treated_as_verified(self):
        for url in ("http://docs.example.invalid/", "https://", "https://user:secret@example.invalid/", "https://[broken", "https://example.invalid:bad/", "https://example.invalid/line\nbreak"):
            document = fixture()
            evidence = document["targets"][0]["scopes"][0]["preferred_write"]["evidence"]
            evidence["source"]["url"] = url
            report = audit.build_report(document, AS_OF)
            self.assertTrue(any(gap["kind"] == "invalid_url" for gap in report["gaps"]))
            self.assertNotIn("secret", json.dumps(report))

    def test_input_shape_schema_identity_and_rule_budgets_reject(self):
        for document in (None, [], {"schema_version": True, "targets": []}, {"schema_version": 2, "targets": []}, {"schema_version": 1, "targets": "bad"}):
            with self.assertRaises(ValueError):
                audit.build_report(document, AS_OF)
        for days in (0, -1, 3651, True):
            with self.assertRaises(ValueError):
                audit.build_report(fixture(), AS_OF, days)
        document = fixture()
        document["targets"][1]["aliases"] = ["old-example"]
        with self.assertRaisesRegex(ValueError, "duplicate"):
            audit.build_report(document, AS_OF)
        document = fixture()
        document["targets"][1]["variant"] = "cli"
        with self.assertRaisesRegex(ValueError, "duplicate"):
            audit.build_report(document, AS_OF)
        document = {"schema_version": 1, "targets": [{}] * 513}
        with self.assertRaisesRegex(ValueError, "512"):
            audit.build_report(document, AS_OF)
        document = fixture()
        document["targets"][0]["scopes"] = []
        self.assertTrue(any(gap["kind"] == "no_scope_evidence" for gap in audit.build_report(document, AS_OF)["gaps"]))
        with patch.object(audit, "MAX_RULES", 1):
            with self.assertRaisesRegex(ValueError, "rule count"):
                audit.build_report(fixture(), AS_OF)

    def test_cli_is_read_only_json_and_reports_invalid_or_oversize_input(self):
        with tempfile.TemporaryDirectory(prefix="ask-evidence-") as root:
            path = Path(root) / "catalog.json"
            payload = json.dumps(fixture()).encode()
            path.write_bytes(payload)
            command = [sys.executable, "-B", str(SCRIPT), "--catalog", str(path), "--as-of", AS_OF.isoformat()]
            result = subprocess.run(command, capture_output=True, text=True, check=False)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(json.loads(result.stdout)["mode"], "offline")
            self.assertEqual(path.read_bytes(), payload)
            self.assertEqual(list(Path(root).iterdir()), [path])
            path.write_text('{"schema_version": 2, "schema_version": 1, "targets": []}', encoding="utf-8")
            result = subprocess.run(command, capture_output=True, text=True, check=False)
            self.assertEqual(result.returncode, 2)
            self.assertIn("duplicate JSON", result.stderr)
            path.write_text("{", encoding="utf-8")
            result = subprocess.run(command, capture_output=True, text=True, check=False)
            self.assertEqual(result.returncode, 2)
            self.assertEqual(result.stdout, "")
            path.write_text("[" * 2000 + "]" * 2000, encoding="utf-8")
            result = subprocess.run(command, capture_output=True, text=True, check=False)
            self.assertEqual(result.returncode, 2)
            self.assertNotIn("Traceback", result.stderr)
            path.write_bytes(b" " * (audit.MAX_INPUT_BYTES + 1))
            result = subprocess.run(command, capture_output=True, text=True, check=False)
            self.assertEqual(result.returncode, 2)
            self.assertIn("2 MiB", result.stderr)


if __name__ == "__main__":
    unittest.main()
