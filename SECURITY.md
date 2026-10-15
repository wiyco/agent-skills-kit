# Security policy (pre-release)

No released version is currently supported. This policy must be finalized before the first public release.

The installer treats skills as untrusted data. Valid metadata and a safe copy operation do not certify that a skill is safe for an AI agent to execute. See `docs/architecture.md` and ASK-008 for the threat model.

During private development, report security concerns through the restricted repository to the maintainer, omitting credentials and unnecessary sensitive data. Do not post an exploitable vulnerability publicly.

Before public release, the maintainer must enable and verify a private reporting channel (such as GitHub private vulnerability reporting), update this document with actual supported versions, and define a response process. This document does not claim that a reporting feature has already been enabled.

Do not invent or publish an email address for security reports.
