# Seed evidence for core target research

Accessed during planning on **2026-10-16**. These are documentation observations, not tested adapters. Re-read the source before implementation; confirm OS-specific behavior and environment overrides separately.

| Target | Documented project skills root | Documented user skills root | Official source |
| --- | --- | --- | --- |
| Codex | `.agents/skills/` under relevant repository directories | `$HOME/.agents/skills/` | [Where Codex loads local skills](https://developers.openai.com/codex/skills/) |
| Claude Code | `.claude/skills/` | `~/.claude/skills/` | [Where skills live](https://code.claude.com/docs/en/skills) |
| OpenCode | `.opencode/skills/` | `~/.config/opencode/skills/` | [Place files](https://opencode.ai/docs/skills/) |

Each skills root contains `<skill-name>/SKILL.md`; supporting files belong inside that skill directory. This seed does not claim the listed path is the only readable location.

Codex documents scanning repository ancestors. Claude Code documents additional/nested and managed/plugin locations, with target-specific restrictions. OpenCode documents reading `.claude/skills` and `.agents/skills` locations as well as its own. Capture only the MVP-supported authoring scopes; do not write into managed/bundled stores or assume selection restricts other readers.

The Codex URL resolved to `https://learn.chatgpt.com/docs/build-skills` during review. Preserve the canonical resolved documentation URL/section in the eventual evidence record. The existence of a redirect is not itself evidence of changed installation behavior.

## Development sources

- [Proto Rust plugin](https://github.com/moonrepo/rust-plugin) — delegates to rustup and recommends its toolchain file.
- [Rustup overrides](https://rust-lang.github.io/rustup/overrides.html) — repository-local toolchain selection.
- [Cargo workspaces](https://doc.rust-lang.org/cargo/reference/workspaces.html) — library/CLI workspace configuration.
- [Agent Skills specification](https://agentskills.io/specification) — source format, not host install paths.

## Research limitations

The planning pass did not install or execute any host, test any OS-specific adapter, establish a complete market census, reserve crate names, or verify an exact Rust/Action release pin. Those are tracked tasks. Earlier discussion is not a verified registry and must not be copied as such.
