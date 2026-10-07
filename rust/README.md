# `rust/` — CXX-Qt contracts for the `qs.services` boundary

This crate is **a contract, not a port.**

Step 2 of [`docs/plans/UI_CONSOLIDATION_AND_BACKEND_BOUNDARY.md`](../docs/plans/UI_CONSOLIDATION_AND_BACKEND_BOUNDARY.md)
makes `qs.services.*` the only thing the QML layouts talk to, so the backend can
later be replaced without touching a layout. Milestone 2.2 asks for that
replacement surface to be written down somewhere a compiler can check. That is
what lives here: one `#[cxx_qt::bridge]` per contracted service, declaring the
properties, methods and signals a native implementation has to provide.

The bodies are the minimum that makes each declared surface coherent. They are
not an implementation and are not wired into the running shell.

## Why it is not the backend yet

[`docs/plans/RUST_BACKEND_MIGRATION.md`](../docs/plans/RUST_BACKEND_MIGRATION.md)
deliberately sequences this as **acceptance gates, not a mandatory rewrite**:

- **Gate 1** wants a native bridge spike — a packaged dynamic plugin proving
  worker updates, incremental models, teardown and fallback.
- **Gate 3** wants profiling or a functional gap to *choose* the first domain
  that earns a Rust implementation.

Writing contracts first serves both gates without pre-committing to either. The
three domains covered here were picked for small, well-understood surfaces:

| Bridge | Contract for | Why this one |
| --- | --- | --- |
| `src/battery.rs` | `services/power/Battery.qml` | Pure observed state — no root-level invokables, so there is no command surface to preserve. |
| `src/brightness.rs` | `services/display/Brightness.qml` | Shell-wide state plus stepping and sleep entry points. Per-output `monitors` stays in QML: moving it needs Gate 1's incremental-model work. |
| `src/audio.rs` | `services/media/Audio.qml` | The plan's own worked example; covers properties, invokables and a signal. |

## Keeping it honest

`scripts/test-rust-contract.py` checks every contracted member against the QML
singleton it names, mapping `snake_case` to the camelCase Qt spelling that QML
consumers bind to. Members are matched only at the **root** of the singleton, so
a function nested in an `IpcHandler` does not count as part of the QML API.

A contract may cover less than a service exposes, and services may have no
contract yet. What the gate forbids is a contract claiming something the QML side
does not have — the exact drift that would make "drop-in replacement" untrue.

## Building

```bash
cargo build --manifest-path rust/Cargo.toml
```

The contract check runs without a Rust toolchain, since it only parses. The
compile step is opt-in via `INIR_BUILD_RUST=1` in
`scripts/test-local-distribution.sh` so a shell-only checkout stays testable.

Two deliberate choices, both from the migration plan's bridge gate:

- **The module URI is `qs.services.native`, not `qs.services`.** Gate 1 requires
  the native path to load *beside* the QML one so the two can be compared; a
  shadowing name would make that impossible.
- **Registration is static, though the eventual plugin is dynamic.** CXX-Qt
  cannot lay out a dynamic QML plugin from a pure Cargo build — it needs the
  CMake integration — so requesting one here would produce a module that cannot
  be imported. Packaging the dynamic plugin is itself Gate 1 work.

`rust/` is absent from `sdata/runtime-payload-dirs.txt` and excluded in
`sdata/runtime-exclusions.json`, so nothing here ships to an install, and
`rust/target/` is gitignored. `Cargo.lock` **is** tracked: the migration plan
calls for pinned build dependencies.
