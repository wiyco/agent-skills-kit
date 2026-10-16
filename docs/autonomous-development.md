# Autonomous development, review, and merge policy

## Status and precedence

This policy becomes effective after it is merged into `main`. It governs issue delivery through M0–M4 up to, but not including, the explicit OSS publication gate. M5 requires separately approved scope.

Once effective, this policy provides standing maintainer authorization for routine self-reviewed PR merges **only when every safeguard below is satisfied**. It supersedes the historical generic 'do not merge without maintainer approval' boilerplate in imported ASK issues, but not explicit task-specific approval gates, this document's stop conditions, or publication restrictions. If another instruction genuinely conflicts, stop and request clarification.

Self-review is not independent approval and cannot prove an absence of defects. Report remaining uncertainty. Never misrepresent a missing check as successful.

## Milestone autonomy

- Read `AGENTS.md`, the live GitHub Project, issue descriptions, dependencies, relevant ADRs, and this policy. Pick the earliest authorized incomplete milestone. Prefer highest-priority unblocked work.
- Work through dependency-ready issues, including follow-on issues in the same milestone after earlier PRs are merged. Do not blindly implement every open issue in numerical order.
- Make a small, reviewable plan per issue and update the Project `Workflow` field: `Ready` -> `In Progress` -> `In Review` -> `Done`. Mark `Done` only after a successful merge and satisfied acceptance criteria.
- One issue may use several PRs if they represent independent, testable changes; one PR may close an issue only when its remaining acceptance criteria are met. Use `Refs #N` for intermediate PRs; `Closes #N` on the final PR.
- Never create competing implementations of the same issue. Parallel work is permitted only for independent changes in isolated branches/worktrees, and must not compromise reproducibility.
- Before moving to the next milestone, verify its exit criteria and all required issue/PR states. If requirements are incomplete, record blockers; do not silently skip them.
- Stop before any public release, crates.io publish, GitHub release/binary upload, repository or Project visibility change, secret/service activation, purchase, or other externally consequential action. ASK-026 requires explicit maintainer decision; do not treat the absence of failures as publication approval.

## Conventional Commits and PR titles

Every new commit subject and PR title must follow Conventional Commits 1.0.0:

```text
<type>[optional scope][!]: <imperative summary>
```

Allowed types: `feat`, `fix`, `docs`, `test`, `refactor`, `chore`, `ci`, `build`, `perf`, `style`, `revert`. Use a meaningful lowercase scope such as `registry`, `resolver`, `installer`, `cli`, `tooling`, `testing`, `ci`, `security`, or `docs` where useful. Keep the entire subject at or below 100 characters; use English, no trailing period, no `WIP`, and no issue number as a title prefix.

Examples:

```text
chore(tooling): bootstrap the Rust workspace
feat(resolver): resolve documented project paths
fix(installer): reject symlink escapes
test(security): cover destination race conditions
docs(workflow): describe autonomous QA gates
feat(api)!: change install plan serialization
```

Explain breaking changes in the PR and use an explicit `BREAKING CHANGE:` footer when relevant. Titles are automatically validated by the PR-title workflow; missing enforcement is a blocker for autonomous merging.

Commit in logical, compilable checkpoints when feasible. Do not create meaningless micro-commits, giant mixed-purpose commits, or cleanup-only churn to meet a numeric target. Keep the PR itself the smallest independently understandable, testable change. Prefer one reviewed PR = one squash commit on `main`; work-in-progress branch commits remain inspectable in the PR.

## PR design and stacked PRs

- Default to one issue/independent logical change per PR. Keep unrelated refactors, generated noise, or adjacent feature work out of the diff. Explain scope and non-goals.
- Use the official [`gh stack`](https://github.com/github/gh-stack) extension for **linear dependent** changes that benefit from being reviewed before the lower layer lands. Do not stack unrelated parallel work. Install the extension only with the local maintainer's permission where required.
- This extension is in public preview; check availability and current commands before relying on it. It works with same-repository branches, not cross-fork stacks. Prefer short stacks (about 2–4 layers), unless a longer stack is justified.
- Use `gh stack init`, `gh stack add`, and `gh stack submit --open` for active review. Use `gh stack rebase` / `gh stack sync` after lower-layer changes; verify stack topology and changed diffs before merging.
- Each stacked PR must independently satisfy its acceptance criteria, checks, and QA requirements. Top-of-stack merging also merges all unmerged lower layers. Do not run `gh stack merge --yes --squash` until **every included PR** independently qualifies.
- If an independent PR has no dependency benefit from stacking, prefer an ordinary focused PR.

## Required self-review and merge gate

Self-merging is allowed for implementation PRs only **after ASK-003 establishes working CI and repository merge safeguards** (or equivalent verified safeguards) and all of these conditions are met:

1. The exact final revision and, for a stack, all included PR revisions have passing required GitHub checks. Include native Linux/macOS/Windows tests, formatting, Clippy, documentation, and any security/QA checks applicable to the changed area. Never bypass or fabricate a required status.
2. The live issue acceptance criteria and dependencies are satisfied. The PR title follows the convention; the PR body includes actual commands/results, limitations, and final issue references.
3. The author re-reads the *final diff*, evaluates API compatibility, tests, error handling, untrusted inputs, permissions, security boundaries, docs, and dependency/license changes. Fix findings and rerun affected checks before merging.
4. There are no unresolved review threads, unresolved conflicts, draft status, skipped mandatory QA, unexplained flaky tests, or other material doubts. Do not merge when verification cannot run on a required platform.
5. For QA-sensitive work, post the detailed evidence in a separate PR conversation comment **after the final changes**, with the verified commit SHA. Check all checks again if later pushes invalidate that evidence.
6. The repository settings permit squash merging, the final squash subject is the Conventional PR title, and protection/required checks are enabled where the GitHub plan supports them. Never push directly to `main`.

Before CI and protections exist, bootstrap PRs require maintainer confirmation to merge. Do not infer a green status from an empty checks list. Where private-repository plan limits prevent enforceable protection, document the limitation and seek maintainer authorization instead of silently lowering the bar.

GitHub PR authors cannot approve their own PRs. Self-review may be documented in a comment/checklist but must never be impersonated as an independent approval. If a required human review exists, wait for the actual reviewer; do not bypass it.

Use **Squash merge** as the sole normal merge method. For stacks, use GitHub's official stack merge with `--squash` after checking every layer. Plain `gh pr merge --squash` is appropriate for unstacked PRs. `gh stack merge` is an explicit merge action; GitHub's auto-merge mode is not currently supported for stacks. Monitor the operation to completion before marking issues Done.

## QA evidence comment

PR bodies contain a concise QA summary. For changes involving filesystem writes, path containment, symlinks/reparse points, concurrency/races, cross-platform behavior, CLI interaction, untrusted skill inputs, security, or release packaging, add a separate **QA Evidence** PR comment with:

- final commit SHA and affected PR/stack layer; test date; OS/architecture and toolchain or host version
- acceptance criteria mapped to exact commands, integration scenarios, fixture details, negative/boundary tests, and observed outcomes
- CI run URLs, logs or excerpts where appropriate, and notes on known gaps, manual steps, or unsupported environments
- reproduction steps for anything not covered by automated tests; any known failures and their disposition

A QA claim describes demonstrated behavior, not universal correctness. If the target host, OS, or required security condition cannot be tested, mark that limitation explicitly and do not advertise the combination as validated.

## Required GitHub settings (maintainer-owned)

Enable only **Squash merge**, choose **PR title** for the squash commit subject, and consider automatic branch deletion after merge once stack behavior is understood. Require PRs, current CI checks, and linear history on `main` when available; disallow bypasses and direct pushes. The official stack workflow evaluates protections against the trunk, including each stack layer.

For a solo-agent workflow, a required approving human review blocks self-merge because authors cannot approve their own PRs. Keep that requirement disabled only for the explicitly authorized routine workflow, or obtain a real independent reviewer where required. The maintainer controls GitHub settings and retains final approval over all publication gates.

References: https://www.conventionalcommits.org/en/v1.0.0/ ; https://docs.github.com/en/pull-requests/reference/stacked-pull-requests ; https://github.com/github/gh-stack ; https://docs.github.com/en/pull-requests/reference/pull-request-merges ; https://docs.github.com/en/pull-requests/how-tos/review-pull-requests/approving-a-pull-request-with-required-reviews
