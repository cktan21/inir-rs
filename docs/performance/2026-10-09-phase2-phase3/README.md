# Interrupted exploratory capture

This series was interrupted after the first Rust run. Review found that taskbar
enrichment still copied titles into plain fields; stable native records require
live getters so visible titles do not go stale. The getters were fixed, and
the complete exclusive comparison restarted in `../2026-10-09-phase2-phase3-final/`.

Do not use this incomplete series for Rust-versus-QML performance conclusions.
