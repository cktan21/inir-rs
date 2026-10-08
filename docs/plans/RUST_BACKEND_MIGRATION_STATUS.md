# Rust migration implementation status

This tracks the mandatory [migration plan](RUST_BACKEND_MIGRATION.md). Completed
code is distinguished from full acceptance: live Quickshell/Niri validation and
cross-layout benchmarks cannot be inferred from a headless Qt smoke test.

| Gate | Implemented | Remaining acceptance work |
| --- | --- | --- |
| M0 | Repeatable `inir profile` procfs capture, explicit unavailable metrics, frame percentile import. [Environment record](../performance/baseline-environment.json). | The current environment has no Quickshell, Niri, pidstat, perf or strace. Capture real CPU/GPU/RSS, fork/wakeup traces, startup, settings and frame metrics across all three layouts before service cutover. |
| M1 | Native workspace, pinned CMake/CXX-Qt dynamic plugin, import metadata, shared Tokio core, identity and resource properties, main-thread marshaling, teardown, opt-in SystemInfo facade, Make/Arch/Nix packaging changes. | Verify actual Quickshell import/render/reloads in a Niri session. Test Nix builds and aarch64 packaging. Qt engine reload tests pass locally; M1's live gate remains open. |
| M2 | Direct NetworkManager/zbus discovery and signal subscriptions, typed AP state with raw SSIDs and stable object-path keys, incremental Qt AP model, radio/scan/disconnect commands, initial saved-profile/open/WPA-PSK activation API. Opt-in Network facade uses native observations and radio/scan/disconnect. | Complete connection outcome/secret-agent handling, password/profile management, enterprise/WPA3/hidden/hotspot/VPN parity, scan completion UI and device/error details. Connection/password/profile/VPN frontend paths still use legacy services. Live control/parity and benchmarks precede deletion. |
| M3 | BlueZ ObjectManager/device/battery observations and incremental adapter/device models; native power/discovery/pair/connect/disconnect/remove commands. Direct UPower display-device and AC state feed the opt-in Battery facade; Bluetooth summary reads also delegate. Modern and legacy power-profile D-Bus endpoints are supported with native profile selection. | Migrate Bluetooth panels and implement Agent1 prompts, trust and pairing policy. Move power-profile UI/persistence, charge thresholds, privileged battery writes, suspend notifications/policy and idle management. Battery/charge hardware and interactive controls remain unvalidated; existing policies remain in QML. |
| M4 | Async Niri Unix event stream using pinned official IPC types, reconnect/cancellation, window/workspace/layout/focus/urgency reduction, initial focus history, incremental Qt window/workspace models and separate action sockets. Existing facade sends supported actions through Rust when ready. | Adopt native models in all consumers, preserve spatial/layout data in exported roles, outputs, MRU behavior, keyboard/config/hot-corner and single-window policies. Frontend state reducers still run for parity; test a real Niri session before replacing them. |
| M5 | Locked atomic native config store, version-1 migration/backup, inotify reload, FIFO patches, concurrent process tests, native ConfigService; performance fields are typed and extension data survives. | Complete all typed sections and migrations; replace every legacy writer; integrate `Config.options/ready/configChanged`. Legacy QML writers do not participate in native locking yet. |
| M6 | Stable eight-category metadata and initial scalar performance schema. | Implement the unified desktop control center, generated rows and rich domain views; retire fragmented settings only after parity. |
| M7 | Native identity, uptime, RAM/swap and CPU sampling. Native PipeWire registry/default metadata/SPA volume-mute subscriptions and node model, volume/mute commands; selected existing audio controls delegate. Native backlight discovery/sysfs reads/writes with logind permission fallback and existing monitor facade integration. Native MPRIS player metadata/model and basic playback commands. Desktop leases suspend these workers when unused; backlight reads run every two seconds while active. | Complete GPU/disk/temperature and memory-pressure parity; native DDC/output mapping, night-light/display controls; audio default/route/profile switching, stream mixer, microphone/virtual-device policy and protection; MPRIS position/seek/rich frontend behavior. Validate real backlight and controls, migrate remaining frontend paths, and benchmark before removal. |
| M8 | Optional native provider is loaded asynchronously. | Audit and optimize the full heavy frontend, collection virtualization, bindings and idle animation behavior. |
| M9 | Existing rendering policy retained. | Implement and validate Performance/Balanced/Quality tiers and automatic activation. |
| M10 | Baseline tooling and automated native/distribution checks. | Re-run real benchmarks, establish all performance targets, complete diagnostics UI, decommissioning, documentation and release verification. |

## Verification

Local Qt 6.11.2 / Rust 1.97.1 validation:

- Core regression tests cover procfs units and CPU deltas, domain diffing,
  versioned backups, preservation of legacy fields, rejection of invalid/future
  configuration, four concurrent native writer processes, atomic editor
  replacements, and suspension with zero resource consumers.
- Dynamic QML smoke tests cover camelCase API names, periodic CPU/uptime updates,
  configuration writes and FIFO ordering, inotify reload, 25 engine reloads and
  a bound on worker thread accumulation. They also check native service model
  access, inactive startup, explicit command parsing errors and FIFO completion
  of 32 requests rejected while services are inactive.
- Seven additional core tests cover NetworkManager's actual D-Bus wire format
  against a private mock peer, scan dispatch, AP security/identity, sysfs bounds
  and device-name validation, PipeWire SPA/cubic volume decoding, Niri focus,
  close and urgency events, unknown-event tolerance and separate action sockets.
- Qt model tests use QAbstractItemModelTester and persistent indexes to verify
  insert/remove/move/data-change notifications, duplicate-key rejection and
  updates without model resets. Desktop updates coalesce to one pending Qt
  callback per QObject; PipeWire snapshots coalesce without blocking its loop.
- A [read-only native service probe](../performance/native-services-probe.json)
  observed NetworkManager, BlueZ, UPower, power profiles, PipeWire and MPRIS on
  this machine. It found five audio nodes with volume data, one Bluetooth
  adapter and two media players. UPower and backlight discovery correctly
  reported no battery/backlight hardware. NIRI_SOCKET is unset.
- Baseline tests cover procfs process names containing spaces/parentheses,
  percentile math, real process metrics and explicit missing-process reports.
- Payload fixtures exercise native module installation in both Arch packages
  and exclusion of source/build artifacts from the shell payload.

Run from the checkout:

```bash
make test-native
INIR_RUNTIME_DIR="$PWD" INIR_BUILD_RUST=1 bash scripts/test-local-distribution.sh
python3 scripts/test-runtime-payload.py
# Read-only; requires local desktop daemons, never changes device settings:
cargo run --manifest-path rust/Cargo.toml -p inir-core --example services_probe --locked
INIR_TEST_LIVE_SERVICES=1 rust/build/inir_bridge_smoke "$PWD/rust/build/qml"
```

No service has met its full decommissioning gate. Legacy implementations are
retained for comparison and compatibility. Enable the bridge for live validation
with `INIR_RUST_BACKEND=1`; it is not the default backend yet. The optional provider
acquires one desktop consumer, starting the shared D-Bus, Niri, backlight and
PipeWire workers. Releasing the last consumer stops them; each domain exposes
readiness and errors for fallback. There are two shared Tokio workers plus a
dedicated native PipeWire loop thread while desktop services are active.
Domain consumers are currently grouped under one desktop lease; individual
domain/panel leases still need frontend integration. Battery fields commit
together before notifying QML, with readiness published after initialization.

DesktopServices exports accessPoints, bluetoothAdapters, bluetoothDevices,
backlights, audioNodes, mediaPlayers, niriWindows and niriWorkspaces as incremental
Qt models. Existing QML APIs and richer policies remain available during adoption.
Power-profile and MPRIS controllers currently expose native APIs/models without
replacing their frontend controls. A successful command completion means the
daemon accepted the request (or the backlight write completed); Wi-Fi activation
and PipeWire parameter application still require observing the resulting state.

No live control tests changed radios, connections, pairing, volume, brightness,
power profiles or playback. The available environment permits native read and
headless Qt validation, but cannot establish Quickshell layout parity, Niri
behavior, battery/backlight hardware support or the plan's performance gates.
