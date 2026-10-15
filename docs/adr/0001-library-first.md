# ADR 0001: Library-first, local-copy MVP

Status: proposed implementation direction; maintainers may amend before implementation.

The product's value is reusable evidence-backed destination resolution and safe installation, not only a terminal UI. Start with one library crate and one thin CLI in a Cargo workspace. Keep catalog, resolver, validation, planning, and execution as library modules.

Use a local skill directory as the initial input. Copy is the only MVP install method. Keep remote acquisition and lifecycle management out of the first release. This gives other Rust applications a small useful dependency and reduces network/credential/trust complexity.

Target selection is preferred placement, not exclusive host visibility. The library records shared-reader effects and does not silently consolidate native placements.

Consequences: the CLI and external examples exercise the same API; custom target definitions can be supported without writing a plugin system. Additional crates are introduced only when independently reusable APIs or dependencies justify them.
