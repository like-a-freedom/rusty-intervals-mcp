---
name: rust-repo-structure
description: "Guide to idiomatic Rust repository layout for small crates, multi-binary projects, and Cargo workspaces."
metadata:
  version: "1.0"
  author: "rust-repo-structure-skill"
  tags: ["rust", "cargo", "repository", "layout", "workspace", "module-structure"]
user-invocable: true
---

# Rust Repository Structure Guide

Use this skill when you need a clear, idiomatic layout for Rust codebases, from a simple CLI crate to a multi-crate workspace.

---

## Core Principles

- `main.rs` should be a thin wrapper: argument parsing, configuration, application startup.
- `lib.rs` should be the root of the public API and contain the main logic.
- Extract shared types, traits, and errors into separate modules (`config.rs`, `error.rs`, `models.rs`).
- Follow Cargo conventions: `src/`, `tests/`, `examples/`, `benches/`.
- Prefer the 2018+ module style: `src/utils.rs` + `src/utils/` instead of `src/utils/mod.rs`.
- Feature flags should be additive and explicit; avoid hidden dependencies through `default`.
- Workspaces are useful for separating core, CLI, server, and shared types.
- Build profiles should be configured only in the workspace root.

---

## Level 1: Simple crate (CLI / utility)

text
my-tool/
├── Cargo.toml
├── Cargo.lock
├── src/
│   ├── main.rs        ← entry point only, argument parsing
│   ├── lib.rs         ← pub mod ...; public API root
│   ├── config.rs
│   ├── error.rs       ← single error type (thiserror / anyhow)
│   └── utils/
│       ├── parser.rs
│       └── formatter.rs
├── tests/
│   └── integration.rs
└── examples/
    └── basic.rs

- `main.rs` should do the minimum: read `std::env::args`, build `Config`, call `run(config)`, and handle errors.
- `lib.rs` exports `run`, modules, and public structures.
- `config.rs` defines configuration shape and argument parsing (`clap`, `argh`, `structopt`).
- `error.rs` contains a single error enum and conversion to `anyhow::Error` or `miette::Report`.
- Integration tests live in `tests/`, while unit tests stay next to the code.
- `examples/` demonstrate user scenarios without duplicating core logic.

---

## Level 2: Library plus multiple binaries

text
src/
├── lib.rs
└── bin/
    ├── server.rs   ← cargo run --bin server
    └── client.rs

- `lib.rs` should contain shared types, errors, and functions used by all binaries.
- `bin/*.rs` should only contain the entry point for each specific application.
- This is ideal when one codebase serves multiple user-facing executables.
- Use `cargo run --bin <name>` and `cargo build --bins`.

---

## Level 3: Virtual Workspace

text
# Cargo.toml
[workspace]
members = ["crates/*"]
resolver = "2"

text
crates/
├── core/    ← shared types, traits, errors
├── server/
├── cli/
└── proto/

- A workspace provides a single `Cargo.lock`, a shared `target/`, and clear separation of concerns.
- `core` contains shared models, traits, errors, and helper functions.
- `cli` and `server` depend on `core`, but do not need to depend on each other unless necessary.
- `proto` can hold generated Protobuf/GRPC types and codegen artifacts.
- Use `cargo check --workspace`, `cargo test --workspace`, `cargo build --all-features`.

---

## Feature flags

text
[features]
default = []
async   = ["dep:tokio"]
tls     = ["dep:rustls", "async"]
full    = ["async", "tls"]

- Features should be modular: each one adds behavior without disabling other code.
- `default = []` helps avoid implicit dependencies in a library.
- For CI, use `cargo hack check --feature-powerset` or `cargo test --all-features`.
- In workspace crates, verify feature compatibility with `cargo test -p <crate> --all-features`.

---

## Build profiles (workspace root only)

text
[profile.dev.package.serde]
opt-level = 3

[profile.release]
lto = "thin"
codegen-units = 1
strip = true

- Profile optimizations should be declared in the workspace root `Cargo.toml`.
- Do not duplicate profiles in every crate — that makes maintenance harder.

---

## `mod.rs` vs 2018+ style

| Style | File |
|-------|------|
| Old | `utils/mod.rs` |
| New (2018+) | `utils.rs` + folder `utils/` |

- In Rust 2018+, prefer a flat structure where the module is declared in `src/utils.rs` and child files are stored under `src/utils/`.
- This makes navigation easier and simplifies refactoring.

---

## Reference repositories

Look at these projects for mature structure patterns:

- `ripgrep` — multi-crate layout, feature flags, performance.
- `tokio` — virtual workspace, separate `core` and `util` components.
- `axum` — separation of `axum-core` / `axum-extra`.
- `clap` — tidy feature gates and re-exports.
- `serde` — separate proc-macro crate.

---

## Quick checklist

- [ ] `main.rs` is thin, `lib.rs` contains the logic
- [ ] `Cargo.toml` contains no logic; only dependencies, features, profile
- [ ] `tests/`, `examples/`, `benches/` are used for their intended purpose
- [ ] `workspace` is used if the project contains more than one related crate
- [ ] All public API entry points are exported from `lib.rs`
- [ ] Features are additive, `default = []`
- [ ] Build profiles are configured at the workspace root
- [ ] Modern 2018+ module style is used
- [ ] Shared logic lives in `core` / `lib`, and entry points remain thin
