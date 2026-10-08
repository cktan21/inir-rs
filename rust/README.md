# Native backend

The mandatory migration is defined in
[RUST_BACKEND_MIGRATION.md](../docs/plans/RUST_BACKEND_MIGRATION.md). This workspace
now builds a dynamically loaded QML plugin, alongside the existing QML services.
[Migration status](../docs/plans/RUST_BACKEND_MIGRATION_STATUS.md) records the
remaining acceptance gates; the complete service rewrite is not finished.

| Crate | Responsibility |
| --- | --- |
| `inir-types` | Qt-independent state, configuration types and settings metadata. |
| `inir-core` | Shared Tokio runtime, typed domain reducers, NetworkManager/BlueZ/UPower/power-profile/MPRIS D-Bus services, Niri sockets, PipeWire, sysfs/procfs/NSS and configuration persistence. |
| `inir-qt` | CXX-Qt SystemInfo, ConfigService, DesktopServices and incremental collection models, registered in qs.services.native. |
| `inir-backend` | Earlier Audio/Battery/Brightness interface contracts. Not included in the runtime plugin. |

## Build and verify

Requires a Rust toolchain, CMake 3.24+, a C++ compiler, and Qt 6.5+ Core/Qml
development packages, PipeWire development headers, pkg-config and libclang
(for SPA bindings). Native tests also use Qt Test. The frontend's existing Qt/Quickshell requirements still
apply. CMake helpers are vendored at pinned upstream releases; Cargo dependencies
are pinned in Cargo.lock.

```bash
make build-native
make test-native
```

CMake builds `rust/build/libinir_qt.so` and stages the importable module at
`rust/build/qml/qs/services/native/`, including qmldir, plugin.qmltypes and
libqs_services_native.so. The staged filename follows the plugin name generated
by CXX-Qt. A Cargo build alone does not assemble the import root. See the
[CXX-Qt build documentation](https://kdab.github.io/cxx-qt/book/concepts/build_systems.html).

Make install, both Arch shell PKGBUILDs, and the Nix derivation install the
compiled module under `lib/qt-6/qml/qs/services/native/`. Rust source and build
directories remain excluded from the shell runtime payload. The launcher adds
the native import root to QML_IMPORT_PATH; an explicit root can be supplied
with INIR_NATIVE_QML_DIR.

Native logs use tracing with a warning-level default; set INIR_LOG to a tracing
filter (for example, inir_core=debug) for worker diagnostics.

## Run the parallel bridge

```bash
INIR_RUNTIME_DIR="$PWD" INIR_RUST_BACKEND=1 ./scripts/inir run --foreground
./scripts/inir services
```

`services/system/SystemInfo.qml` preserves its public API and delegates identity
reads to the plugin when enabled. A failed plugin import retains the QML service.
The opt-in facades also delegate Network observations/radio/scan/disconnect,
battery readings, Bluetooth summary state, backlight I/O, selected audio
volume/mute controls and Niri actions to Rust. Richer frontend paths remain on
their existing implementations until their own parity gates pass. The contract-only Audio,
Battery and Brightness types are never exported by the runtime plugin.

SystemInfo.setResourceMonitoring(true) acquires a resource consumer; false
releases it. Resource sizes use **bytes** in the native API; legacy ResourceUsage
uses kB. CPU usage is a fraction and becomes available after two samples. Without
consumers, the core takes one initial sample and then suspends polling entirely.

QObjects share one Tokio runtime through a weak cache. QObject-owned tasks abort
on destruction, consumers release automatically, and the final owner cancels the
core. Worker updates use CxxQtThread::queue; a destroyed QObject rejects them.
The smoke test imports the plugin dynamically without linking it into the host,
checks periodic updates, and destroys/recreates 25 QML engines while checking
worker thread counts.

## Desktop service migration

DesktopServices.setServicesActive(true) acquires a shared desktop consumer;
false releases it. Loading the singleton by itself does not connect to desktop
services. The optional Provider acquires this lease. Workers stop at zero leases:
system/session D-Bus signal subscribers, the reconnecting Niri stream, backlight
reads every two seconds, and a dedicated native PipeWire loop thread. Discovery
and controls invoke no CLI tools. Domain readiness/errors are exposed separately.

Collections are QAbstractListModels with stable string IDs, named roles, a
`record` role, `count` and `get(index)`. Properties: accessPoints,
bluetoothAdapters, bluetoothDevices, backlights, audioNodes, mediaPlayers,
niriWindows and niriWorkspaces. The small C++ adapter only implements Qt model
notifications; all device/state policy lives in Rust. Updates preserve delegates
and persistent indexes and emit insert/remove/move/dataChanged without resets.
Each QObject permits at most one pending desktop update callback. Core and
PipeWire channels retain the latest snapshot when updates arrive in bursts.

DesktopServices.execute(requestId, json) validates a typed command and reports
commandFinished(requestId, success, error). Each QObject uses a bounded FIFO
command queue, preserving slider/request order. Battery fields commit as a
single snapshot before property notifications and readiness changes. Examples:

```js
DesktopServices.execute("radio", JSON.stringify({type: "wifiEnabled", enabled: true}))
DesktopServices.execute("scan", JSON.stringify({type: "wifiScan"}))
DesktopServices.execute("volume", JSON.stringify({type: "audioVolume", node: "42", value: 0.5}))
DesktopServices.execute("light", JSON.stringify({type: "brightness", device: "intel_backlight", value: 0.5}))
DesktopServices.execute("window", JSON.stringify({type: "niri", action: {FocusWindow: {id: 42}}}))
```

Other initial commands cover Wi-Fi activation/disconnection, BlueZ power,
discovery, pairing, connect/disconnect/remove, power profiles, audio mute and
MPRIS playback verbs. Credentials never enter state snapshots or logs. Initial
Wi-Fi activation supports compatible saved profiles, open and WPA-PSK networks;
full secret-agent/enterprise/hotspot/VPN parity remains pending. Interactive
BlueZ pairing currently requires an existing agent. Brightness falls back from
direct sysfs to logind SetBrightness when access is denied; DDC remains legacy.
PipeWire volume uses the cubic UI scale and retains channel balance. Success
means a request was accepted, not necessarily that activation/application has
finished. See the status document for each frontend boundary and parity gap.

The following probe performs reads only:

```bash
cargo run --manifest-path rust/Cargo.toml -p inir-core --example services_probe --locked
# Also verifies model/property access through the dynamically loaded Qt plugin:
INIR_TEST_LIVE_SERVICES=1 rust/build/inir_bridge_smoke "$PWD/rust/build/qml"
```

## Configuration foundation

ConfigService.open(path) explicitly opens a configuration file; loading the
plugin alone does not read or rewrite the user's configuration. Its JSON document
and scalar schema metadata are exported to QML. setValue(key, jsonValue) patches
a dotted key, validates it, writes under a separate advisory lock, fsyncs the
temporary file, atomically renames it and fsyncs the directory. Patches from each
QObject execute in FIFO order. Independent native processes reload inside the
lock, preserving each other's edits.

Linux directory watching uses inotify via notify, including atomic editor
replacements. Invalid external contents retain the last valid QML snapshot and
set errorString. Unversioned configurations migrate to version 1 with a private
config.json.bak; unknown settings survive. Future versions and invalid data are
rejected without rewriting them.

Only the initial performance fields are strongly typed; other sections retain
extension maps. modules/common/Config.qml still owns frontend configuration.
It has **not** switched to the native writer, so the cross-process guarantee
currently applies to native writers only. M5 requires completing the typed schema
and migrating all writers together before removing legacy JSON persistence.

## Profiling

```bash
INIR_RUNTIME_DIR="$PWD" ./scripts/inir profile --layout ii --duration 30 --output /tmp/ii-baseline.json
```

Capture ii, iris and waffle separately in a live Niri session, with the same
wallpaper, monitors and interaction scenarios. --pid selects an explicit shell;
--frame-times imports a JSON array of millisecond frame durations from a Qt
profiler capture. Context switches and observed child starts are reported with
their own names: they are not substituted for scheduler wakeups or complete fork
tracing. Unsupported measurements remain explicitly unavailable.
