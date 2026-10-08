//! CXX-Qt contracts for the `qs.services` backend boundary.
//!
//! Step 2 of `docs/plans/UI_CONSOLIDATION_AND_BACKEND_BOUNDARY.md` makes
//! `qs.services.*` the only thing the QML layouts talk to. These bridges state,
//! in a form the compiler checks, what each of those singletons must expose for
//! a native implementation to drop in underneath unchanged layouts.
//!
//! They register into `qs.services.native`, not `qs.services`: the companion
//! plan (`RUST_BACKEND_MIGRATION.md`, Gate 1) requires the native path to load
//! beside the QML one so the two can be compared, and a shadowing module name
//! would make that impossible.
//!
//! These historical contracts are not the runtime plugin. `inir-qt` provides
//! the implemented native services; each remaining domain must pass the
//! mandatory migration plan's parity gate before these bodies are replaced.

pub mod audio;
pub mod battery;
pub mod brightness;
