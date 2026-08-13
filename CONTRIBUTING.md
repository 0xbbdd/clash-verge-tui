# Contributing

## Development environment

The minimum supported toolchain is Rust 1.88.0. Install it with the components used by CI:

```bash
rustup toolchain install 1.88.0 --profile minimal --component rustfmt,clippy
```

Run every quality gate before submitting a change:

```bash
cargo +1.88.0 fmt --check
cargo +1.88.0 clippy --locked --all-targets -- -D warnings
cargo +1.88.0 test --locked
cargo +1.88.0 build --locked --release
```

## Design constraints

- Keep Mihomo Controller operations in `api.rs`; do not add an unauthenticated generic file API.
- Keep local Clash Verge access in `local.rs`, use bounded reads and atomic writes, and reject path traversal.
- Tests must not mutate a developer's live controller, network configuration, or Clash Verge data directory.
- Any new bulk deletion, upgrade, restart, or network-wide mutation needs an explicit confirmation flow.
- Preserve the boundary documented in `docs/UPSTREAM.md`: a local file edit is not proof that Clash Verge ran Merge/Script enhance or applied a runtime configuration.
- UI rendering belongs in `ui.rs` and must not perform I/O.

## Testing live controllers

Live testing is optional and must begin with read-only discovery:

```bash
cargo run -- --doctor
```

When reporting compatibility, record the Clash Verge package version, Mihomo version, controller transport, operating system, and whether the evidence is read-only or includes an explicitly authorized mutation.
