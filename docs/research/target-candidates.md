# Target research candidates

Planning review: **2026-10-16**. This is a non-exhaustive research queue, not a supported-target list or a popularity ranking. No implementation or actual-host testing has been completed by this bootstrap.

Some documentation pages were retrieved during planning; only the three core path examples are summarized in `seed-evidence.md`. Every candidate still requires per-rule evidence and tests before shipping. Products are not described as newly launched merely because they appear here. ASK-021 establishes dated discovery and launch evidence.

| Candidate | Group | Entry point | Task | Research note |
| --- | --- | --- | --- | --- |
| Codex | Core | [Reference](https://developers.openai.com/codex/skills/) | ASK-006 | Seed paths reviewed; re-verify all shipped rules. |
| Claude Code | Core | [Reference](https://code.claude.com/docs/en/skills) | ASK-006 | Seed paths reviewed; target-specific reserved names need review. |
| OpenCode | Core | [Reference](https://opencode.ai/docs/skills/) | ASK-006 | Seed paths reviewed; shared-reader behavior matters. |
| Cursor | Expansion | [Reference](https://cursor.com/docs/skills) | ASK-016 | Check local, remote, and cloud variants separately. |
| Gemini CLI | Expansion | [Reference](https://geminicli.com/docs/cli/skills/) | ASK-016 | Verify defaults and overrides. |
| GitHub Copilot CLI | Expansion | [Reference](https://docs.github.com/en/copilot/how-tos/copilot-cli/customize-copilot/add-skills) | ASK-016 | Do not infer VS Code behavior from CLI docs. |
| GitHub Copilot in VS Code | Expansion | [Reference](https://docs.github.com/en/copilot/how-tos/copilot-cli/customize-copilot/add-skills) | ASK-016 | Discover the matching official editor docs. |
| Cline | Expansion | [Reference](https://docs.cline.bot/customization/skills) | ASK-017 | Check current editor/CLI variants. |
| Roo Code | Expansion | [Reference](https://docs.roocode.com/features/skills) | ASK-017 | Follow official documentation redirects. |
| Kilo Code | Expansion | [Reference](https://kilo.ai/docs/customize/skills) | ASK-017 | Check IDE/CLI differences and compatibility settings. |
| Junie CLI | Additional CLI/IDE | [Reference](https://junie.jetbrains.com/docs/agent-skills.html) | ASK-018 | Verify applicable product version. |
| Junie IDE | Additional CLI/IDE | [Reference](https://junie.jetbrains.com/docs/agent-skills.html) | ASK-018 | Separate from CLI if behavior differs. |
| Factory Droid | Additional CLI/IDE | [Reference](https://docs.factory.com/harness/skills) | ASK-018 | Current skills docs are under harness. |
| Kimi Code CLI | Additional CLI/IDE | [Reference](https://moonshotai.github.io/kimi-cli/en/customization/skills.html) | ASK-018 | Check configurable state home. |
| Qwen Code | Additional CLI/IDE | [Reference](https://qwenlm.github.io/qwen-code-docs/en/users/features/skills/) | ASK-018 | Verify user/project support. |
| Mistral Vibe | Additional CLI/IDE | [Reference](https://docs.mistral.ai/vibe/code/cli/skills) | ASK-019 | Verify current skills behavior. |
| Amp | Additional CLI/IDE | [Reference](https://ampcode.com/docs) | ASK-019 | Official docs entry point; locate skills-specific page. |
| Pi | Additional CLI/IDE | [Reference](https://pi.dev/) | ASK-019 | Official product entry point; locate versioned skills docs. |
| Crush | Additional CLI/IDE | [Reference](https://github.com/charmbracelet/crush) | ASK-019 | Official repository; locate user documentation and overrides. |
| Kiro IDE | Workspace/variant | [Reference](https://kiro.dev/docs/skills/) | ASK-020 | Do not assume CLI parity. |
| Kiro CLI | Workspace/variant | [Reference](https://kiro.dev/docs/skills/) | ASK-020 | Locate CLI-specific official evidence. |
| Antigravity IDE/App | Workspace/variant | [Reference](https://antigravity.google/docs/skills) | ASK-020 | Verify platform and product variant. |
| Antigravity CLI | Workspace/variant | [Reference](https://antigravity.google/docs/skills) | ASK-020 | Do not reuse an IDE user path by assumption. |
| Goose | Workspace/variant | [Reference](https://github.com/vercel-labs/skills) | ASK-020 | Find host-owned docs; discovery catalog is not authority. |
| OpenClaw | Workspace/variant | [Reference](https://docs.openclaw.ai/tools/skills) | ASK-020 | Application workspace is not automatically a Git root. |
| Windsurf / Devin Desktop | Workspace/variant | [Reference](https://docs.windsurf.com/windsurf/cascade/skills) | ASK-020 | Official URL currently redirects to Devin docs; verify migration details. |
| Zed | Workspace/variant | [Reference](https://zed.dev/docs/ai/skills) | ASK-020 | Verify editor discovery and compatible hosts. |
| Warp | Discovery queue | To be discovered from official sources | ASK-021 | Find current official Agent Skills support evidence. |
| Augment | Discovery queue | To be discovered from official sources | ASK-021 | Identify the precise product/variant first. |
| Continue | Discovery queue | To be discovered from official sources | ASK-021 | Do not conflate rules/prompts with Agent Skills. |
| Replit | Discovery queue | To be discovered from official sources | ASK-021 | Determine whether filesystem copying is supported at all. |
| Trae | Discovery queue | To be discovered from official sources | ASK-021 | Verify official local installation mechanism. |

## Evidence rule

Host-owned official documentation is the authority for installation paths. A third-party registry is only a lead. Official implementation source may explain conflicts but does not silently replace missing documentation. Record uncertainty and a concrete follow-up when the documentation is incomplete.

Copy-only support is not universal. Cloud/import-only or configuration-required products remain explicit non-copy outcomes unless an approved scope adds their installation mechanism. Do not guess a `.brand/skills` path.

## Discovery cadence

Use an explicit review task or a low-frequency reporting workflow, not runtime catalog downloads. Scan recent official announcements/releases, record actual event dates separately from document update dates, deduplicate aliases/forks, and ask for human review before shipping new rules.
