# Backend Consolidation and Rust Migration Gates

## Current scope: behavior-preserving file consolidation

This change organizes existing QML implementations; it does **not** introduce Rust,
replace system integrations, or change config, Settings, startup, or service behavior.
`services/qmldir` retains the public `qs.services` names, versions, and singleton/type
registrations. Consumers keep their existing imports and APIs.

| Implementation directory | Relocated files |
|---|---|
| `services/network/` | `Network.qml`, `Vpn.qml`, `BluetoothStatus.qml` |
| `services/compositor/` | `CompositorService.qml`, `NiriService.qml`, `DankSocket.qml`, `HyprlandData.qml`, `NiriAnimationPresets.qml` |
| `services/display/` | `Brightness.qml`, `Hyprsunset.qml`, `brightnessPolicy.js` |
| `services/media/` | `Audio.qml`, `MprisController.qml` |
| `services/power/` | `Battery.qml`, `Idle.qml`, `PowerProfilePersistence.qml`, `idlePolicy.js` |
| `services/system/` | `ResourceUsage.qml`, `SystemInfo.qml`, `MemoryPressureService.qml` |

These domain folders are **not new public modules**. The existing
`qs.services.network` module continues to expose only `WifiAccessPoint` through
`services/network/qmldir`; `qs.services.deferred` remains unchanged.
Relative helper imports, registrations, tests, and path references must follow moves.
Do not combine this cleanup with polling changes, API redesign, config migration,
or removal of authentication, platform, or command fallbacks.

Layout guard:

```bash
python3 -B scripts/test-service-layout.py
```

## Assessment: verified evidence versus proposed work

- **Verified:** iNiR runs an external Quickshell executable (`scripts/inir`), not a
  custom Qt host. `Makefile` currently prepares/copies a QML/script payload.
- **Verified:** Arch shell packages use `arch=(any)`; `nix/package.nix` uses
  `stdenvNoCC`. `nix/runtime-source-filter.nix` admits runtime content rather than
  arbitrary new root build directories. A native backend needs a new build contract.
- **Verified:** Quickshell provides several integrations already used here, including
  native Networking in `modules/pill/LinkWifi.qml` and `modules/pill/PillLink.qml`.
  `services/network/Network.qml` and parts of the pill UI still use `nmcli`.
- **Verified:** config already has typed QML properties, persistence safeguards,
  dynamic data, and multiple process consumers; replacing it is not just adding Serde.
- **Inference:** selective Rust extraction may improve correctness or performance.
  No Rust plugin loading, reload compatibility, or performance gain is proven by
  this consolidation. A static QObject demo alone does not prove integration.

## Keep existing native Quickshell integrations

| Domain | Existing boundary to retain unless a demonstrated gap justifies replacement |
|---|---|
| Bluetooth | `Quickshell.Bluetooth`; `services/network/BluetoothStatus.qml` |
| Battery / power profiles | Quickshell UPower / PowerProfiles; `services/power/` |
| Audio | Quickshell PipeWire; `services/media/Audio.qml` |
| Media transport | Quickshell MPRIS; extract iNiR policy separately if useful |
| Notifications / tray / authentication | Quickshell Notifications, SystemTray, Polkit, and PAM |
| Network | Evaluate Quickshell Networking parity before adding another NM client |

Native network models do not establish full feature parity: test saved profiles,
secrets, enterprise Wi-Fi, hotspot, VPN, and advanced editing. Keep narrow fallbacks
where needed. Charge-limit discovery and privileged writes are separate from UPower;
a Polkit agent does not itself grant the shell permission to write sysfs.

## Revised sequencing: acceptance gates, not a mandatory rewrite

| Gate | Exit condition |
|---|---|
| 0. Baseline + file consolidation | Stable public API/layout; record reproducible measurements before behavior changes |
| 1. Native bridge spike | Installed dynamic plugin, async updates, incremental models, teardown, and fallback work in Quickshell |
| 2. Network consolidation | Compare both existing paths; establish native capability coverage and fallback ownership |
| 3. One justified Rust domain | Profiling or a functional gap selects the domain; preserve adapters and public behavior |
| 4. Typed config reader, then writer | Shadow validation and fixtures pass before persistence ownership changes |
| 5. Settings consolidation | Stable page IDs and compatibility mappings precede taxonomy changes |

Frontend lifetime/rendering improvements and measurements can proceed independently.
Resource sampling or a pure Niri event reducer are candidates, not guaranteed wins.
Preserve Niri event/request sockets, reconnect/resynchronization, ordering, MRU,
throttling, and policy semantics; test fixtures before moving active policy logic.

## Bridge gate: packaging, reload, threading

[CXX-Qt documents dynamic QML plugins](https://kdab.github.io/cxx-qt/book/concepts/build_systems.html),
but defaults to static registration; its documented dynamic route requires CMake
and a Rust `cdylib`. Use public Qt APIs rather than Quickshell internals.

- Package the library, `qmldir`, and `.qmltypes` with explicit singleton registration
  and a non-conflicting module name. Prove a worker update and incremental model.
- Supply the QML import root to shell and standalone entry points; generic
  `QT_PLUGIN_PATH` alone is not the QML module discovery contract.
- Test repo-copy, repo-link, system-prefix, and Nix installs. Split architecture-specific
  artifacts if keeping the shell package architecture-independent. Separate build
  source filtering from installed payloads; never ship Cargo `target/` wholesale.
- Pin build dependencies/lockfiles; test supported Qt versions and architectures.
  Coordinate frontend/backend upgrades, diagnostics, restart, and rollback.
  [Qt plugin compatibility](https://doc.qt.io/qt-6/deployment-plugins.html) differs
  from Quickshell's private-Qt ABI constraints.
- Test repeated QML reloads, family switches, concurrent standalone Settings, exit,
  and missing/incompatible plugins. Isolate optional imports so fallback can load;
  `services/PolkitService.qml` demonstrates a dynamic implementation boundary.
- Keep Qt property/model notifications on the owning thread. Never block Qt with
  async work; cancel tasks and invalidate stale QObject references during teardown.
  Do not assume a rebuilt native library can be replaced through QML hot reload.

## Config compatibility and cross-process writers

Preserve `modules/common/Config.qml`'s facade: `options`, `ready`, `revision`,
`configChanged`, nested getters/setters, batching, and flushing. Start with a
read-only typed subsection and golden fixtures, not an immediate format replacement.

- Resolve the same effective canonical/legacy file as `scripts/lib/config-path.sh`
  and `modules/common/Directories.qml`; retain JSON keys and compatibility aliases.
- Preserve unknown nested fields, custom-widget/mascot maps, intentional deletions,
  defaults, false/zero values, and legacy representations. Keep transient/persistent
  UI state (`modules/common/Persistent.qml`) separate from user configuration.
- Shell and standalone Settings are separate processes: an in-process Rust singleton
  does not create one shared AppState or writer. Choose one writer over IPC, or
  cross-process locking plus read/merge/write transactions. Atomic replacement alone
  prevents partial files, not lost updates; watchers must survive replacement.
- Define unversioned/future-version handling, idempotent migrations, backups, and
  downgrade behavior. Coordinate with `sdata/lib/migrations.sh` and its consent/history
  policy. Test external edits, concurrent writes, failures, and self-write echoes.
- Keep schema, shipped defaults, and reset metadata synchronized; extend
  `scripts/test-iris-defaults.py` when changing the schema authority.

## Settings and comparison safety

`modules/settings/SettingsPageRegistry.qml` stores arrangements by numeric index;
`settings.qml` persists the current page, and `shell.qml` exposes index-based IPC.
Introduce stable page IDs and legacy-index mappings before reorganizing pages.
Preserve search, translations, deep links, and family-specific routes. Reuse the
metadata in `modules/iris/settings/IrisOptions.qml`; generic rows must retain bundled
updates and special actions such as family transitions and Niri config writes.

Compare replacements through passive observations or recorded event replay, **not
parallel active controllers**. Do not duplicate scanners, notification/Polkit owners,
profile restoration, privileged writes, or workspace policies. Preserve tray ownership:
`assets/systemd/inir.service` uses the StatusNotifierWatcher bus name for readiness.
Keep required protocol/model/IPC owners resident; unload expensive presentation trees
without blindly destroying drafts, prewarming, or delayed animation teardown.

## Baseline measurement cautions

Measure ii/Waffle/iRiS, cold/warm startup and reopen, shell plus standalone Settings,
static/live wallpapers, and representative monitors, scaling, and refresh rates.
Select the actual shell instance/PID, not the newest Quickshell process. Include
process-tree memory, GPU costs, wakeups, subprocesses, and input-to-visible latency;
RSS alone is not a rendering or whole-session budget. Account for profiler overhead.
`inir doctor --perf` and `shell.qml` boot markers are starting points, not frame-time
measurements. Startup percentage and latency targets remain aspirations until measured.
