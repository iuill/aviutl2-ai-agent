# Development

## Required checks

The canonical build runs in Docker:

```bash
docker build --output type=local,dest=dist .
```

For a local Rust installation:

```bash
cargo fmt --all --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
```

The Windows artifacts use Rust 1.88.0, `cargo-xwin` 0.19.2, and statically
linked MSVC CRT. `aviutl2` is pinned exactly to 0.41.0; update it only with the
compatibility checks recorded in `docs/phase0.md`.

## Phase 0 boundaries

The fixed port 7890 and unauthenticated `/healthz` endpoint exist only for the
single-instance bootstrap probe. Do not add the provisional write API before
Q1–Q7 have been measured on Windows and Design v0.5 has replaced the unverified
branches.
