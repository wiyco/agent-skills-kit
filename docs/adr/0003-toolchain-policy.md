# ADR 0003: Rustup toolchain file as the Rust version source of truth

Status: proposed implementation direction.

Use an exact dev toolchain pin in `rust-toolchain.toml`. Proto remains compatible and can manage other tools, but Cargo execution should rely on rustup's repository-local override. The proto Rust plugin itself recommends this file and does not create Cargo shims. [Source](https://github.com/moonrepo/rust-plugin).

Do not require a duplicate Rust pin in `.prototools`. A future duplicate must be generated or checked automatically. GitHub Actions installs the same dev pin and separately tests the measured MSRV and latest stable compatibility.

The dev compiler version and the library MSRV serve different purposes. Verify an actual stable release when bootstrapping; do not claim an older compiler is supported until tests prove it. [Rustup](https://rust-lang.github.io/rustup/overrides.html) and [Cargo MSRV](https://doc.rust-lang.org/cargo/reference/rust-version.html).

No Docker, Node package manager, moon, or Turbo dependency is introduced into the Rust build without a concrete need.
