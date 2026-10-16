# Attribution and third-party material

Original code and test fixtures are preferred. Do not copy third-party installers, catalogs, example skills, scripts, or documentation merely because they are publicly readable.

Before incorporating material, record its owner, exact source URL/revision, affected files, license text/version, required notices, modifications, and the review approving reuse. Keep required copyright and license notices with the material and add any applicable NOTICE information. If rights or terms are unclear, leave the material out and record the gap. A root project license does not replace third-party terms.

References and paraphrased research findings should link to their primary source, relevant section, review date, version, and uncertainty. Third-party path catalogs are candidate lists, not authority for host paths. Adapter evidence still requires host-owned documentation under AGENTS.md.

The bootstrap CLI stdout fixture was authored for this repository. It contains no third-party skill payload. No source code, fixtures, or catalog data from the installers compared in [ADR 0005](adr/0005-naming-licensing-and-reuse.md) has been incorporated. Official GitHub checkout/cache actions are invoked at reviewed SHAs rather than copied into this repository; action updates remain reviewable dependency changes.

Adding a Rust dependency requires maintenance, license, MSRV, and necessity review. Record findings in its PR and keep Cargo.lock tracked. Dependency license and advisory automation is implemented separately in ASK-023; this policy does not claim that an unrun audit passed.

The YAML parser dependency `saphyr-parser` 0.2.2 requires notices for both Chen Yuheng's original code and Ethiraric's modifications. Two unmodified MIT license texts from the exact published crate's `.licenses/` directory are retained in `docs/third-party/saphyr-parser-0.2.2/`; no parser source is vendored. Source: https://crates.io/api/v1/crates/saphyr-parser/0.2.2/download and https://github.com/saphyr-rs/saphyr, reviewed 2026-10-16 under routine implementation dependency review. The parser is used through Cargo, not by executing README commands. See [validation dependency review](validation.md#dependency-review) for alternatives, maintenance and MSRV. Future release packaging must include both author notices; the root project license does not replace them.
