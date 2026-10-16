# Standalone public API consumer

This unpublished package deliberately has its own workspace and Cargo.lock. It depends only on the library; it uses no CLI internals or unsafe code. From the repository root:

```sh
cargo run --manifest-path examples/external-consumer/Cargo.toml --locked
cargo test --manifest-path examples/external-consumer/Cargo.toml --locked
```

All filesystem writes use an exclusively created temporary fixture with explicit home/project roots. The fixture is removed afterward. No real skill directories, environment mutation, payload execution, credentials or host/model services are used. The example explicitly approves only its own reviewed fixture plans. Tests cover both scopes, all three core targets, aliases, custom registration, typed negative cases and versioned receipts.

See [the API guide](../../docs/api.md) for compatibility, approval, security and package-content boundaries. Native tests must run on the actual OS/filesystem; a local macOS run does not prove Windows behavior.
