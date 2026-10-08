Build-only upstream CMake helpers, unmodified:

- CXX-Qt CMake 0.10.0, commit `64e63f560737b3b7f85ef8267c5918e4b8176d5a`,
  https://github.com/KDAB/cxx-qt-cmake (MIT OR Apache-2.0).
- Corrosion 0.5.2, commit `a1a1aaa057a5da656c06c3d8505b767a4e941709`,
  https://github.com/corrosion-rs/corrosion (MIT).

Only CMake sources and licenses are included. These do not ship in the runtime
payload. Keeping them here makes Arch/Nix builds independent of FetchContent
network access. Rust dependencies remain pinned in `rust/Cargo.lock`.
