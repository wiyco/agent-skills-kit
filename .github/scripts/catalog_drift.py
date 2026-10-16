"""Evidence-age audit; opt-in HEAD observations never establish path correctness."""

import argparse
from datetime import date, datetime, timezone
import json
from pathlib import Path
import re
import sys
from urllib.parse import urlsplit

MAX_INPUT_BYTES = 2 * 1024 * 1024
MAX_RULES = 4096


def iso_date(value):
    if not isinstance(value, str) or not re.fullmatch(r"[0-9]{4}-[0-9]{2}-[0-9]{2}", value):
        raise ValueError("expected a YYYY-MM-DD date")
    return date.fromisoformat(value)


def evidence_age(accessed, as_of, stale_after_days):
    if accessed is None or accessed == "":
        return {"state": "missing", "days": None}
    try:
        age = (as_of - iso_date(accessed)).days
    except ValueError:
        return {"state": "invalid", "days": None}
    state = "future" if age < 0 else "stale" if age >= stale_after_days else "within_window"
    return {"state": state, "days": age}


def load_catalog(path):
    with path.open("rb") as source:
        payload = source.read(MAX_INPUT_BYTES + 1)
    if len(payload) > MAX_INPUT_BYTES:
        raise ValueError("catalog exceeds the 2 MiB audit input limit")
    return json.loads(payload, object_pairs_hook=unique_object, parse_constant=reject_constant)


def reject_constant(_value):
    raise ValueError("non-finite numbers are not valid evidence JSON")


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError("duplicate JSON object key")
        result[key] = value
    return result


def required_text(record, field):
    value = record.get(field)
    if not isinstance(value, str) or not value:
        raise ValueError(f"missing or invalid {field}")
    return value


def build_report(document, as_of, stale_after_days=90):
    """Group canonical identities and URL references without interpreting paths."""
    if type(stale_after_days) is not int or not 1 <= stale_after_days <= 3650:
        raise ValueError("stale threshold must be an integer from 1 to 3650 days")
    if not isinstance(document, dict) or type(document.get("schema_version")) is not int or document.get("schema_version") != 1:
        raise ValueError("expected catalog schema_version 1")
    targets = document.get("targets")
    if not isinstance(targets, list) or len(targets) > 512:
        raise ValueError("expected at most 512 target records")
    products, sources, gaps, namespace = {}, {}, [], set()
    rule_count = 0
    for target in targets:
        if not isinstance(target, dict):
            raise ValueError("expected a target object")
        identity = required_text(target, "id")
        product = required_text(target, "product_id")
        variant = required_text(target, "variant")
        aliases = target.get("aliases", [])
        scopes = target.get("scopes")
        if not isinstance(aliases, list) or not all(isinstance(alias, str) and alias for alias in aliases):
            raise ValueError("expected nonempty alias strings")
        if not isinstance(scopes, list):
            raise ValueError("expected target scopes")
        for name in [identity, *aliases]:
            if name in namespace:
                raise ValueError("duplicate target ID or alias")
            namespace.add(name)
        members = products.setdefault(product, [])
        if any(member["variant"] == variant for member in members):
            raise ValueError("duplicate product/variant identity")
        scope_rows = []
        if not scopes:
            gaps.append({"target": identity, "kind": "no_scope_evidence"})
        members.append({"id": identity, "variant": variant, "aliases": sorted(aliases), "scopes": scope_rows})
        for scope in scopes:
            if not isinstance(scope, dict):
                raise ValueError("expected a scope object")
            scope_name = required_text(scope, "scope")
            status = required_text(scope, "status")
            scope_rows.append({"scope": scope_name, "status": status})
            rules = []
            if scope.get("preferred_write") is not None:
                rules.append(("preferred_write", scope["preferred_write"]))
            readers = scope.get("discovery", [])
            if not isinstance(readers, list):
                raise ValueError("expected discovery records")
            for index, reader in enumerate(readers):
                if not isinstance(reader, dict):
                    raise ValueError("expected a discovery object")
                rules.append((f"discovery[{index}]", reader.get("location")))
            if not rules:
                gaps.append({"target": identity, "scope": scope_name, "kind": "no_rule_evidence", "status": status})
            for role, rule in rules:
                rule_count += 1
                if rule_count > MAX_RULES:
                    raise ValueError("audit rule count exceeds 4096")
                reference = {"target": identity, "product": product, "variant": variant, "scope": scope_name, "role": role}
                if not isinstance(rule, dict) or not isinstance(rule.get("evidence"), dict):
                    gaps.append({**reference, "kind": "missing_evidence"})
                    continue
                evidence = rule["evidence"]
                source = evidence.get("source")
                if not isinstance(source, dict):
                    gaps.append({**reference, "kind": "missing_source"})
                    continue
                url = source.get("url")
                if not isinstance(url, str) or not url:
                    gaps.append({**reference, "kind": "missing_url"})
                    continue
                try:
                    parts = urlsplit(url)
                    valid_url = parts.scheme == "https" and bool(parts.hostname) and parts.username is None and parts.password is None
                    valid_url = valid_url and not any(ord(character) <= 32 or ord(character) == 127 for character in url)
                    _ = parts.port
                except ValueError:
                    valid_url = False
                if not valid_url:
                    # Do not echo a malformed URL that may contain credentials.
                    gaps.append({**reference, "kind": "invalid_url"})
                    continue
                counts = {}
                for field in ("resolver_tests", "discovery_tests"):
                    recorded = evidence.get(field, [])
                    if not isinstance(recorded, list) or not all(isinstance(item, str) and item for item in recorded):
                        gaps.append({**reference, "kind": f"invalid_{field}"})
                        recorded = []
                    counts[field] = len(recorded)
                age = evidence_age(source.get("accessed"), as_of, stale_after_days)
                row = {**reference, "accessed": source.get("accessed"), "age": age,
                       "section": source.get("section"), "applicability": source.get("applicability"),
                       "documentation": evidence.get("documentation"),
                       "resolver_references_recorded": counts["resolver_tests"],
                       "discovery_references_recorded": counts["discovery_tests"]}
                sources.setdefault(url, []).append(row)
                for field in ("section", "applicability"):
                    if not isinstance(source.get(field), str) or not source[field]:
                        gaps.append({**reference, "kind": f"missing_{field}"})
                if age["state"] != "within_window":
                    gaps.append({**reference, "kind": f"accessed_{age['state']}"})
                if evidence.get("documentation") != "reviewed":
                    gaps.append({**reference, "kind": "documentation_not_reviewed"})
        scope_rows.sort(key=lambda row: (row["scope"], row["status"]))
    key = lambda row: json.dumps(row, sort_keys=True, ensure_ascii=True)
    return {
        "report_version": 1, "mode": "offline", "as_of": as_of.isoformat(),
        "stale_after_days": stale_after_days, "path_correctness": "not_assessed",
        "human_review_required_before_catalog_changes": True,
        "notice": "Age and recorded test references are metadata, not verification performed by this command. No adapters are modified.",
        "identity_policy": "Group aliases/variants by canonical product; do not infer fork equivalence or count aliases as products.",
        "products": [{"product_id": product, "variants": sorted(members, key=key)} for product, members in sorted(products.items())],
        "sources": [{"url": url, "references": sorted(references, key=key), "probe": {"state": "not_run"}} for url, references in sorted(sources.items())],
        "gaps": sorted(gaps, key=key),
    }


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--catalog", type=Path, required=True, help="explicit validated catalog JSON export")
    parser.add_argument("--as-of", default=datetime.now(timezone.utc).date().isoformat())
    parser.add_argument("--stale-after-days", type=int, default=90)
    parser.add_argument("--online", action="store_true", help="opt in to public HTTPS HEAD observations")
    parser.add_argument("--timeout-seconds", type=float, default=8)
    parser.add_argument("--budget-seconds", type=float, default=180)
    args = parser.parse_args(argv)
    try:
        report = build_report(load_catalog(args.catalog), iso_date(args.as_of), args.stale_after_days)
        if args.online:
            from evidence_transport import observe_report
            report = observe_report(report, timeout=args.timeout_seconds, budget=args.budget_seconds)
    except (OSError, ValueError, TypeError, RecursionError) as error:
        parser.exit(2, f"Evidence audit failed: {error}\n")
    print(json.dumps(report, sort_keys=True, indent=2, ensure_ascii=True))
    return 0


if __name__ == "__main__":
    sys.exit(main())
