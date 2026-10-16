# Security policy (unreleased development)

## Supported versions

No crate or binary release is available. Reports about the current development branch are reviewed on a best-effort basis; no released-version support commitment or response deadline is established. Include the affected commit and native OS/filesystem when reporting.

The installer treats skills as untrusted data. Valid metadata and safe copying do not certify that a skill is safe for an AI agent to execute. See the [architecture](docs/architecture.md), [threat model](docs/threat-model.md) and [native verification limits](docs/verification/ask-012.md).

## Confidential reporting availability

**Pending maintainer verification: confidential reporting arrangements are deferred for the WIP source-only preview and must be finalized before public contribution intake or supported library/CLI publication.** Do not submit exploitable details in an Issue, PR, review or Project item. Repository visibility can change, and those surfaces are not confidential channels.

GitHub private vulnerability reporting is the intended repository-local option after public visibility. It has not been enabled or verified for this transition. Only use the repository's **Security → Advisories → Report a vulnerability** form when that private reporting feature is actually available. The maintainer must verify access and notification delivery; a link or documentation change does not enable it. If the form is unavailable, do not fall back to a public issue or disclose sensitive details. No approved confidential contact is advertised for this WIP preview; none is invented here.

The [source visibility checklist](docs/public-readiness.md) records this deferred release/collaboration gate. No reporting service, email address or support SLA is implied by this policy.

## Handling a report when a private channel is available

After the reporting route is verified, provide a concise impact description, affected commit/platform, inert temporary-root reproduction, and expected versus observed behavior. Omit credentials, personal data, real user skills and executable malicious payloads. Do not run untrusted skill contents to demonstrate a defect.

The maintainer reviews scope and impact, seeks clarification through the verified private channel, coordinates remediation and affected-version guidance, and agrees on disclosure with the reporter when practical. Restrict access to people needed for investigation; no channel can promise absolute confidentiality. Rotate any exposed credential separately rather than sending it in a report. An unresolved critical safety finding blocks publication and release.
