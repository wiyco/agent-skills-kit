# Security policy (pre-release)

No released version is currently supported. This policy must be finalized before the first public release.

The installer treats skills as untrusted data. Valid metadata and a safe copy operation do not certify that a skill is safe for an AI agent to execute. See `docs/architecture.md` and ASK-008 for the threat model.

During private development, authorized participants can report concerns in a [restricted repository issue](https://github.com/wiyco/agent-skills-kit/issues/new). Confirm the repository is still private and the audience is suitable before including details: all repository members with issue access may read them. Omit credentials and unnecessary sensitive data. This is access-controlled reporting, not a single-recipient confidential channel. If that audience is unsuitable, obtain a suitable private channel through an existing trusted contact before sharing details. Do not post an exploitable vulnerability publicly.

Before public release, the maintainer must enable and verify a private reporting channel (such as GitHub private vulnerability reporting), update this document with actual supported versions, and define a response process. This document does not claim that a reporting feature has already been enabled.

Do not invent or publish an email address for security reports.
