"""Original transport fixtures: no DNS, credentials, downloads or skill execution."""

import copy
import importlib.util
import json
from pathlib import Path
import socket
import unittest
from unittest.mock import MagicMock, patch

SPEC = importlib.util.spec_from_file_location(
    "evidence_transport", Path(__file__).resolve().parents[1] / "evidence_transport.py")
transport = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(transport)
URL = "https://docs.example.invalid/skills"


class TransportTests(unittest.TestCase):
    def test_syntax_blocks_credentials_private_addresses_and_non_https(self):
        for url in ("http://example.invalid", "https://user:secret@example.invalid",
                    "https://127.0.0.1/", "https://[::1]/", "https://localhost/",
                    "https://host.local/", "https://host.internal/", "https://[broken",
                    "https://example.invalid:444/", "https://example.invalid/\n",
                    "https://example.invalid/日本語"):
            with self.subTest(url=url):
                fetch = MagicMock(side_effect=AssertionError("network forbidden"))
                self.assertEqual(transport.probe_url(url, fetch=fetch)["state"], "blocked_url")
                fetch.assert_not_called()

    def test_relative_cross_host_redirects_strip_query_and_fragment_from_report(self):
        fetch = MagicMock(side_effect=[(301, "/next?token=secret#anchor"),
                                       (307, "https://other.example.invalid/final"), (204, None)])
        result = transport.probe_url(URL, fetch=fetch)
        self.assertEqual(result["state"], "redirected")
        self.assertEqual([row["status"] for row in result["chain"]], [301, 307, 204])
        self.assertEqual(fetch.call_args_list[1].args[0], "https://docs.example.invalid/next?token=secret")
        self.assertNotIn("secret", json.dumps(result))
        self.assertNotIn("anchor", json.dumps(result))

    def test_redirect_failures_and_http_errors_are_distinct(self):
        for location in ("http://example.invalid/", "https://127.0.0.1/",
                         "https://user:secret@example.invalid/", "https://[broken"):
            result = transport.probe_url(URL, fetch=lambda *_: (302, location))
            self.assertEqual(result["state"], "blocked_redirect")
            self.assertNotIn("secret", json.dumps(result))
        self.assertEqual(transport.probe_url(URL, fetch=lambda *_: (302, None))["state"], "redirect_missing_location")
        self.assertEqual(transport.probe_url(URL, fetch=lambda *_: (302, URL))["state"], "redirect_loop")
        fetch = MagicMock(side_effect=[(302, f"/hop/{i}") for i in range(6)])
        self.assertEqual(transport.probe_url(URL, fetch=fetch)["state"], "redirect_limit")
        self.assertEqual(fetch.call_count, 6)
        for status in (403, 404, 405, 429, 500):
            result = transport.probe_url(URL, fetch=lambda *_: (status, None))
            self.assertEqual(result["state"], "http_error")
            self.assertEqual(result["chain"][0]["status"], status)

    def test_transport_errors_do_not_echo_exception_details(self):
        fetch = MagicMock(side_effect=TimeoutError("private token secret"))
        result = transport.probe_url(URL, fetch=fetch)
        self.assertEqual(result["state"], "transport_error")
        self.assertEqual(result["error"], "TimeoutError")
        self.assertNotIn("secret", json.dumps(result))

    def test_expired_budget_never_starts_request(self):
        fetch = MagicMock(side_effect=AssertionError("network forbidden"))
        result = transport.probe_url(URL, fetch=fetch, clock=lambda: 10, deadline=10)
        self.assertEqual(result["state"], "budget_exhausted")
        fetch.assert_not_called()

    def test_report_deduplicates_fragments_preserves_input_and_caps_requests(self):
        report = {"sources": [{"url": URL + "#a"}, {"url": URL + "#b"},
                               {"url": "https://other.example.invalid/"}],
                  "path_correctness": "not_assessed", "gaps": [{"kind": "fixture"}]}
        before = copy.deepcopy(report)
        fetch = MagicMock(return_value=(200, None))
        with patch.object(transport, "MAX_URLS", 1):
            result = transport.observe_report(report, fetch=fetch, clock=lambda: 0)
        self.assertEqual(report, before)
        self.assertEqual(fetch.call_count, 1)
        self.assertEqual(result["sources"][0]["probe"], result["sources"][1]["probe"])
        self.assertEqual(result["sources"][2]["probe"]["state"], "budget_exhausted")
        self.assertEqual(result["path_correctness"], "not_assessed")
        self.assertEqual(result["gaps"], report["gaps"])
        for value in (0, -1, float("nan"), float("inf"), 301):
            with self.assertRaises(ValueError):
                transport.observe_report(report, budget=value)
        for value in (0, -1, float("nan"), float("inf"), 31):
            with self.assertRaises(ValueError):
                transport.observe_report(report, timeout=value)

    def test_dns_private_or_mixed_answers_never_open_socket(self):
        public = (socket.AF_INET, socket.SOCK_STREAM, 6, "", ("93.184.216.34", 443))
        private = (socket.AF_INET, socket.SOCK_STREAM, 6, "", ("127.0.0.1", 443))
        for answers in ([], [private], [public, private]):
            with patch.object(transport.socket, "getaddrinfo", return_value=answers), \
                    patch.object(transport.socket, "socket") as create_socket:
                with self.assertRaises(ValueError):
                    transport.request_head(URL, 1)
                create_socket.assert_not_called()

    def test_pinned_public_connection_uses_original_tls_hostname_and_head_only(self):
        address = ("93.184.216.34", 443)
        answers = [(socket.AF_INET, socket.SOCK_STREAM, 6, "", address)]
        raw, tls, context, connection, response = [MagicMock() for _ in range(5)]
        context.wrap_socket.return_value = tls
        connection.getresponse.return_value = response
        response.status = 200
        response.getheader.return_value = None
        with patch.object(transport.socket, "getaddrinfo", return_value=answers) as resolve, \
                patch.object(transport.socket, "socket", return_value=raw), \
                patch.object(transport.ssl, "create_default_context", return_value=context), \
                patch.object(transport.http.client, "HTTPSConnection", return_value=connection):
            self.assertEqual(transport.request_head(URL, 2), (200, None))
        resolve.assert_called_once_with("docs.example.invalid", 443, type=socket.SOCK_STREAM)
        raw.connect.assert_called_once_with(address)
        raw.settimeout.assert_called_once_with(2)
        context.wrap_socket.assert_called_once_with(raw, server_hostname="docs.example.invalid")
        self.assertIs(connection.sock, tls)
        args, kwargs = connection.request.call_args
        self.assertEqual(args, ("HEAD", "/skills"))
        self.assertEqual(set(kwargs["headers"]), {"User-Agent", "Accept"})
        response.read.assert_not_called()
        response.close.assert_called_once()
        connection.close.assert_called_once()


if __name__ == "__main__":
    unittest.main()
