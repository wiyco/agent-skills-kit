# ADR 0005: Names, licensing, and implementation ownership

Status: proposed; explicit maintainer approval is required before adopting names or a license.

## Naming proposal and evidence

Keep the repository `wiyco/agent-skills-kit` and library package `agent-skills-kit`. Use the display name **Agent Skills Kit (Rust)**, CLI package `agent-skills-kit-cli`, and executable `agent-skills-kit`. Retire the provisional `skillctl` name to avoid a confirmed collision. This identifies the library and executable together without a generic `ask` command or a second product identity.

Live checks on 2026-10-16:

| Primary source | Observed result |
| --- | --- |
| [crates.io agent-skills-kit API](https://crates.io/api/v1/crates/agent-skills-kit) | 404 with a structured crate-does-not-exist response; underscore spelling was also checked. |
| [crates.io agent-skills-kit-cli API](https://crates.io/api/v1/crates/agent-skills-kit-cli) | 404 with a structured crate-does-not-exist response. |
| [crates.io skillctl API](https://crates.io/api/v1/crates/skillctl) | Registered package, latest version 0.2.0 at review, linking to [umanio-agency/skillctl](https://github.com/umanio-agency/skillctl), a skills CLI. |
| [bcastelino/agent-skills-kit](https://github.com/bcastelino/agent-skills-kit) | Existing unrelated repository for authoring and packaging skills. The repository-name similarity is real; the Rust installer description and owner must remain visible. |

A missing registry entry is not a reservation or a promise of future availability. These checks do not establish trademark clearance, executable-name uniqueness on every package manager, or absence of confusing similarity. The maintainer must approve the remaining repository-name similarity. Recheck registries before any separately approved publication. No names were reserved or repositories renamed during this review.

## License proposal

Propose `MIT OR Apache-2.0` for original project material, subject to explicit maintainer approval. No license texts or Cargo license metadata are added by this proposal. The maintainer must have authority over the original contributions before approving this choice. This is a project decision, not a claim that a registry or hosting service grants third-party rights.

After approval, add the full license texts, workspace/package metadata, and the approved decision record together. `publish = false` remains in place. Third-party material keeps its original terms and notices; a project license does not relicense it. See [attribution policy](../attribution.md).

## Build versus adopt

The comparison below uses primary project documentation and package manifests reviewed on 2026-10-16. It evaluates architectural fit, not the security of code that has not been audited or executed.

| Candidate and reviewed revision | Relevant documented behavior | Decision for this project's MVP |
| --- | --- | --- |
| [skill-one/agents-skills README](https://github.com/skill-one/agents-skills/blob/4d595aa717e8ff75361ff2cd68a650640235a00c/README.md) and [Cargo manifest](https://github.com/skill-one/agents-skills/blob/4d595aa717e8ff75361ff2cd68a650640235a00c/Cargo.toml) | Rust library/CLI, canonical shared placement and directory links, remote acquisition and lifecycle operations; manifest declares `MIT OR Apache-2.0`, Rust 1.88, and network/archive dependencies. GitHub's license classifier reports Apache-2.0, so rely on the actual license files before any reuse. | Do not adopt the installation engine: shared-link placement, remote acquisition, and lifecycle scope differ from explicit local-copy plans and native placements. No claim is made that an undocumented race guarantee exists or is absent. |
| [vercel-labs/skills README](https://github.com/vercel-labs/skills/blob/13e4063a1cf913f5606d57d42ab83a86f5001e04/README.md) and [package manifest](https://github.com/vercel-labs/skills/blob/13e4063a1cf913f5606d57d42ab83a86f5001e04/package.json) | JavaScript/TypeScript skills CLI; manifest declares MIT, Node >=22.20.0, remote/archive ecosystem dependencies, and a Node executable. | Do not shell out to this CLI or introduce Node into the Rust product build. Use it only as a research candidate source, never as authority for host placement rules. |

Continue building the small library-first local-copy API. No code, catalog entries, fixtures, or installer logic from these candidates is incorporated. Their declared licenses provide possible reuse options, but compatibility is conditional on reviewing the exact material and retaining required notices: MIT requires its copyright/permission notice, while Apache-2.0 also has applicable change/NOTICE obligations. No concrete third-party reuse is approved here. Primary terms: [MIT](https://opensource.org/license/mit), [Apache-2.0](https://www.apache.org/licenses/LICENSE-2.0).

The crates.io `agent-skill` entry still points to `luckie2076/agent-skill`, but its live GitHub API returned 404 during this review. Cached documentation is insufficient evidence of current maintainability or an adoption-ready API; do not substitute it for a reviewed accessible revision.

## Conduct and reporting proposal

Use an original conduct policy and the existing restricted repository issue route during private development. The repository has Issues enabled and remains private; this is a real current channel for authorized participants, not a fabricated address or an enabled public security-reporting service. Public reporting arrangements and an alternate contact for concerns involving the maintainer must be approved and verified before visibility changes. ASK-026 must retain that publication gate. Neither confidential email details nor service activation are inferred from a public GitHub profile.
