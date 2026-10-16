# Catalog schema and bundled evidence

The library's `catalog` module validates versioned target metadata without reading the filesystem, process environment, or network. `Catalog::from_json` parses bounded JSON; `Catalog::new` validates caller-created data; `Catalog::bundled` loads versioned core documentation candidates. Iteration is deterministic. Unknown IDs, fields, enum values and schema versions do not become defaults.

Every definition names a product variant, aliases, scopes and platform candidates. A scope's preferred write path is separate from host-readable discovery paths. A missing write path is unavailable, not a request to guess. Paths use typed context/environment bases and literal components; environment fallbacks are explicit. Application workspace and project context are different bases. See [ADR 0007](adr/0007-catalog-schema-versioning.md) for identity and compatibility policy.

## Current evidence and limitations

Reviewed on 2026-10-16. These are rolling host documentation observations, not pinned host-version or native discovery results. Codex project/user scopes are `Documented` with native lexical resolver tests. Claude Code and OpenCode remain `Unverified` pending their focused adapter changes. No actual host discovery has been run. Evidence records keep those dimensions separate, with adapter resolver IDs separate from empty real-host discovery lists.

| CLI variant | Preferred project / user root in reviewed documentation | Additional discovery / evidence |
| --- | --- | --- |
| Codex (`codex`, alias `codex-cli`) | `.agents/skills` / `$HOME/.agents/skills` | Repository ancestor scanning is described in **Where Codex loads local skills**. [Original URL](https://developers.openai.com/codex/skills/) redirects to [the reviewed canonical page](https://learn.chatgpt.com/docs/build-skills). |
| Claude Code (`claude-code`) | `.claude/skills` / `~/.claude/skills` | **Choose where skills load** also describes other scopes and nested/additional-directory behavior, which are not enabled here. [Official documentation](https://code.claude.com/docs/en/skills). |
| OpenCode (`opencode`) | `.opencode/skills` / `~/.config/opencode/skills` | **Place files** and **Understand discovery** describe Claude/agent-compatible read locations as well as native placement. Preferred writes remain native. [Official documentation](https://opencode.ai/docs/skills/). |

The documentation does not establish a tested version or every native OS combination. Platform arrays identify native lexical resolution candidates, not host discovery or safe installation claims. Admin/system/plugin/cloud stores are outside the modeled write scopes. Selecting a host does not imply exclusive visibility. Host-supported links do not override this project's stricter copy-only/no-descendant-link policy.

The reviewed [OpenCode skills](https://opencode.ai/docs/skills/) and [configuration](https://opencode.ai/docs/config/) pages did not establish an `XDG_CONFIG_HOME` skills override during this review. The bundled rule therefore records only the documented literal home-relative root; it does not silently infer an XDG rule. Other unverified adapters still need override review. Schema validity is not permission to install using unverified facts.

## Context examples

Original `tests/fixtures/catalog-contexts.json` demonstrates an XDG-shaped environment base with an explicit home-relative fallback and a separate application-workspace base. These two synthetic definitions are unverified, have no host-owned evidence, and are excluded from `Catalog::bundled`. They demonstrate schema expressiveness without inventing an installation path for a real host. The examples are inert metadata, not Agent Skills content.

Schema tests validate duplicate ID/alias/variant rejection, unknown scope/version/field handling, unsafe components, environment names and fallbacks, scope consistency, evidence gaps/dates, statuses, parser limits and serialization. Bundled-example tests check deterministic metadata, native versus compatibility placements, explicit unknown support, and empty real-host test evidence. None of these exercises a host or an installation engine; ASK-005/006 and subsequent safety work provide those separate layers.

## Codex CLI adapter review

`core_targets::codex_explicit_scopes_and_alias` and `core_targets::codex_context_and_variant_boundaries` test project/user placement, the canonical alias, explicit missing context, ancestor descriptors, shared-reader warnings, and rejection of unmodeled variants. These run with native paths on the required Linux/macOS/Windows CI matrix; they do not run Codex or prove skill discovery. The source does not establish a minimum CLI version or OS-specific discovery guarantees. No IDE/cloud/admin/system variant is enabled by analogy. No host-specific reserved local skill name is established by the reviewed page; general skill validation remains separate work.

The [environment variable reference, Core locations](https://learn.chatgpt.com/docs/config-file/environment-variables) describes `CODEX_HOME` as state storage including skills. [Advanced configuration, Config and state locations](https://learn.chatgpt.com/docs/config-file/config-advanced) describes that state root but does not establish a replacement for the explicit `$HOME/.agents/skills` authoring root in **Where Codex loads local skills**. All three pages were reviewed on 2026-10-16, against rolling documentation with no pinned host version. This adapter follows the explicit authoring table and does not interpret `CODEX_HOME` as a write override; alternate/legacy state skill locations remain unverified. This limitation is returned in resolution warnings. It must be revisited before claiming configuration-complete or version-specific host coverage.

Codex documents keeping duplicate skill names separate and following skill-folder symlinks. This library's stricter no-descendant-link copy policy still applies. Codex placement can also be visible to OpenCode according to its independently reviewed compatibility discovery table. The pure resolver does not enumerate ancestors or inspect host enable/disable configuration.
