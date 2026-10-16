"""Opt-in public HTTPS HEAD observations, never document or skill execution."""

import copy
import http.client
import ipaddress
import socket
import ssl
import time
from urllib.parse import urldefrag, urljoin, urlsplit, urlunsplit

MAX_REDIRECTS = 5
MAX_URLS = 64


def network_url(url):
    """Conservative public-report URL syntax, before any DNS or connection."""
    if not isinstance(url, str) or len(url) > 4096:
        raise ValueError("unsupported URL")
    if not url.isascii() or any(ord(char) <= 32 or ord(char) == 127 for char in url):
        raise ValueError("unsupported URL spelling")
    parts = urlsplit(url)
    if (parts.scheme != "https" or not parts.hostname or parts.port not in (None, 443)
            or parts.username is not None or parts.password is not None):
        raise ValueError("only unauthenticated HTTPS on port 443 is probed")
    host = parts.hostname.rstrip(".").lower()
    if host == "localhost" or host.endswith((".localhost", ".local", ".internal")):
        raise ValueError("local host is not a public source")
    try:
        address = ipaddress.ip_address(host)
    except ValueError:
        address = None
    if address is not None and not address.is_global:
        raise ValueError("nonpublic address")
    return urldefrag(url)[0]


def display_url(url):
    # Redirect response queries/fragments may contain tokens. Do not copy them
    # into public reports, and never echo malformed redirect/userinfo values.
    parts = urlsplit(network_url(url))
    return urlunsplit((parts.scheme, parts.netloc, parts.path, "", ""))


def request_head(url, timeout):
    """Resolve public addresses once; connect to that address with verified TLS."""
    parts = urlsplit(network_url(url))
    addresses = socket.getaddrinfo(parts.hostname, 443, type=socket.SOCK_STREAM)
    if not addresses or any(not ipaddress.ip_address(row[4][0]).is_global for row in addresses):
        raise ValueError("DNS did not return exclusively public addresses")
    context = ssl.create_default_context()
    last_error = OSError("no usable public address")
    for family, kind, protocol, _name, address in addresses[:4]:
        connection = http.client.HTTPSConnection(parts.hostname, 443, timeout=timeout, context=context)
        raw = None
        try:
            raw = socket.socket(family, kind, protocol)
            raw.settimeout(timeout)
            raw.connect(address)
            # Binding the validated address prevents a second DNS lookup from
            # redirecting the actual connection. SNI/certificate verification
            # still uses the original documentation hostname.
            connection.sock = context.wrap_socket(raw, server_hostname=parts.hostname)
            raw = None
            path = urlunsplit(("", "", parts.path or "/", parts.query, ""))
            connection.request("HEAD", path, headers={"User-Agent": "agent-skills-kit-evidence/1", "Accept": "text/html,text/plain"})
            response = connection.getresponse()
            try:
                return response.status, response.getheader("Location")
            finally:
                response.close()
        except (OSError, ssl.SSLError) as error:
            last_error = error
        finally:
            connection.close()
            if raw is not None:
                raw.close()
    raise last_error


def probe_url(url, fetch=request_head, clock=time.monotonic, deadline=None, timeout=8):
    chain, visited = [], set()
    try:
        current = network_url(url)
    except ValueError:
        return {"state": "blocked_url", "chain": []}
    for _hop in range(MAX_REDIRECTS + 1):
        if current in visited:
            return {"state": "redirect_loop", "chain": chain}
        if deadline is not None and clock() >= deadline:
            return {"state": "budget_exhausted", "chain": chain}
        visited.add(current)
        remaining = timeout if deadline is None else min(timeout, max(0.001, deadline - clock()))
        try:
            status, location = fetch(current, remaining)
        except ValueError:
            return {"state": "blocked_url", "chain": chain}
        except (OSError, ssl.SSLError, http.client.HTTPException) as error:
            # Exceptions may contain reflected URLs or headers; only the class
            # is needed to distinguish a transport failure from an HTTP result.
            return {"state": "transport_error", "error": type(error).__name__, "chain": chain}
        chain.append({"url": display_url(current), "status": status})
        if 200 <= status < 300:
            return {"state": "redirected" if len(chain) > 1 else "reachable", "chain": chain}
        if status not in (301, 302, 303, 307, 308):
            return {"state": "http_error", "chain": chain}
        if not location:
            return {"state": "redirect_missing_location", "chain": chain}
        try:
            current = network_url(urljoin(current, location))
        except ValueError:
            return {"state": "blocked_redirect", "chain": chain}
    return {"state": "redirect_limit", "chain": chain}


def observe_report(report, fetch=request_head, clock=time.monotonic, timeout=8, budget=180):
    """Return a separate report; no age, test reference or adapter is updated."""
    if not 0 < timeout <= 30 or not 0 < budget <= 300:
        raise ValueError("timeout must be 0<seconds<=30 and budget 0<seconds<=300")
    result = copy.deepcopy(report)
    result["mode"] = "online_head_observations"
    result["transport_notice"] = "HTTP status/redirects only; HEAD restrictions may require manual review. No response body is read. Reachability does not prove path correctness or host ownership."
    deadline = clock() + budget
    cache = {}
    for source in result["sources"]:
        try:
            key = network_url(source["url"])
        except ValueError:
            source["probe"] = {"state": "blocked_url", "chain": []}
            continue
        if key not in cache:
            if len(cache) >= MAX_URLS or clock() >= deadline:
                source["probe"] = {"state": "budget_exhausted", "chain": []}
                continue
            cache[key] = probe_url(key, fetch=fetch, clock=clock, deadline=deadline, timeout=timeout)
        source["probe"] = copy.deepcopy(cache[key])
    return result
