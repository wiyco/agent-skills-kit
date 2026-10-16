# Attribution and third-party material

Original code and test fixtures are preferred. Do not copy third-party installers, catalogs, example skills, scripts, or documentation merely because they are publicly readable.

Before incorporating material, record its owner, exact source URL/revision, affected files, license text/version, required notices, modifications, and the review approving reuse. Keep required copyright and license notices with the material and add any applicable NOTICE information. If rights or terms are unclear, leave the material out and record the gap. A root project license does not replace third-party terms.

References and paraphrased research findings should link to their primary source, relevant section, review date, version, and uncertainty. Third-party path catalogs are candidate lists, not authority for host paths. Adapter evidence still requires host-owned documentation under AGENTS.md.

The bootstrap CLI stdout fixture was authored for this repository. It contains no third-party skill payload. No source code, fixtures, or catalog data from the installers compared in [ADR 0005](adr/0005-naming-licensing-and-reuse.md) has been incorporated. Official GitHub checkout/cache actions are invoked at reviewed SHAs rather than copied into this repository; action updates remain reviewable dependency changes.

Adding a Rust dependency requires maintenance, license, MSRV, and necessity review. Record findings in its PR and keep Cargo.lock tracked. Dependency license and advisory automation is implemented separately in ASK-023; this policy does not claim that an unrun audit passed.
