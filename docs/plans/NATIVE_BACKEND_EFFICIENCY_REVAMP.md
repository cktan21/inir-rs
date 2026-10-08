# Native backend efficiency revamp

Status: proposed (2026-10-08). Supersedes the performance assumptions in
[RUST_BACKEND_MIGRATION.md](RUST_BACKEND_MIGRATION.md); its domain inventory
and decommissioning gates still apply.

## Why the Rust backend is not faster

Turning on `INIR_RUST_BACKEND=1` makes the shell do **more** work, not less.
The language is not the cause. There are three reasons:

1. **The work is added on top of the QML services.** Every QML service keeps
   running beside the Rust worker.
2. **Rust copies the QML design's "refetch everything, then diff" pattern.**
   It also adds a JSON serialization step at the Qt boundary.
3. **The expensive part stays on the GUI thread.** The shell's cost is QML/JS
   work on the main thread. A backend that computes the same snapshots faster
   in another thread, then makes the main thread parse, diff and loop over
   them, cannot help.

### 1. Duplicate work (largest cost)

| Domain | QML still running with the native backend on | Rust adds |
| --- | --- | --- |
| Niri | Its own `EventStream` socket and `JSON.parse` for each event ([NiriService.qml:72](../../services/compositor/NiriService.qml)), sorting, and window list batching | A second event stream, the full window and workspace snapshot on every event, and model updates |
| Audio | `Quickshell.Services.Pipewire` (C++) bindings for sink, source and nodes | A second PipeWire client that binds **every** audio node and subscribes to its Props |
| Battery | `UPower.displayDevice` fallback branches keep Quickshell's UPower service alive ([Battery.qml:21](../../services/power/Battery.qml)) | A second UPower subscription |
| Media / Bluetooth | `Quickshell.Services.Mpris` and `Quickshell.Bluetooth` across 60+ files | A second MPRIS and BlueZ subscription |
| Brightness | Detection, timers and `monitor.refresh()` on each revision | A sysfs poll every 2 s |

Quickshell's Pipewire, UPower, Mpris and Bluetooth modules are already native
C++ that is event-driven. Rewriting them in Rust gains nothing, and running
both costs twice as much.

### 2. Rust repeats the snapshot-and-diff design

- **The whole state is cloned on every event.** The reducer clones the entire
  `DesktopState` (all APs, windows, nodes and players) for any change in any
  domain ([runtime.rs:109](../../rust/inir-core/src/runtime.rs)). The Qt task
  clones it again ([desktop.rs:152](../../rust/inir-qt/src/qobjects/desktop.rs)),
  and `apply` clones `last` a third time
  ([desktop.rs:218](../../rust/inir-qt/src/qobjects/desktop.rs)).
- **Any signal triggers a full re-read.** Any D-Bus signal under a path
  namespace marks the domain dirty. 50 ms later the whole domain is re-read
  ([workers.rs:18](../../rust/inir-core/src/services/workers.rs),
  [:113](../../rust/inir-core/src/services/workers.rs)). The signal body,
  which already contains the changed properties, is thrown away. In practice
  this means:
  - **Network.** One AP `Strength` change costs 3 + one per device + one per
    AP **sequential** round trips: manager `GetAll`, then per device `GetAll`,
    `GetAllAccessPoints`, then per-AP `GetAll`
    ([network.rs:41](../../rust/inir-core/src/services/network.rs)).
    NetworkManager emits these changes continuously while scanning.
  - **Media.** Any MPRIS signal costs `ListNames` plus 2 × N `GetAll`
    ([media.rs:13](../../rust/inir-core/src/services/media.rs)).
  - **Name changes.** The `NameOwnerChanged` rule has no argument filter
    ([workers.rs:80](../../rust/inir-core/src/services/workers.rs)), so every
    bus client that connects or disconnects wakes the worker.
- **Commands open a new bus connection each time.** Every command opens a new
  D-Bus connection (socket, SASL handshake and `Hello`)
  ([workers.rs:203,210](../../rust/inir-core/src/services/workers.rs)). This
  includes each brightness slider step that only writes sysfs. The logind
  fallback also repeats the session lookup on every write.
- **Niri rebuilds everything per event.** Each Niri event rebuilds every window
  and workspace record with new strings
  ([niri.rs:202](../../rust/inir-core/src/services/niri.rs)), including the
  constant title-change events.
- **Two worker threads idle on top of the shell.** A two-worker multi-threaded
  Tokio runtime ([runtime.rs:75](../../rust/inir-core/src/runtime.rs)) adds
  work-stealing threads and wakeups to a workload that is almost entirely idle
  I/O.

### 3. The boundary moves work back onto the GUI thread

- **Each changed collection is re-encoded and re-parsed on the main thread.**
  The whole collection goes through `serde_json`, then a UTF-16 `QString`, then
  `QJsonDocument::fromJson` and a `QVariantMap` per row
  ([service_models.cpp:24](../../rust/inir-qt/src/service_models.cpp)). All of
  this runs on the GUI thread.
- **The row diff is O(n²)**, using `QVariant` comparisons
  ([service_models.cpp:46](../../rust/inir-qt/src/service_models.cpp)).
- **Every `data()` call allocates.** Each call creates a `QString` from the
  role name and does a hash lookup in a `QVariantMap`
  ([service_models.cpp:17](../../rust/inir-qt/src/service_models.cpp)). Every
  delegate binding read pays this cost.
- **Revision counters make QML pull the whole list again.**
  `Network._applyNativeNetwork` calls `get(i)`, which converts each row into a
  new JS object. It then runs O(n²) `some`/`find` lookups and reassigns
  `lastIpcObject` on **every** AP, even when nothing changed
  ([Network.qml:38,44](../../services/network/Network.qml)). That re-evaluates
  every AP delegate. `Audio._nativeNode` scans the model linearly with `get(i)`
  on each volume tick ([Audio.qml:120](../../services/media/Audio.qml)).
- **One lease starts every domain.** `Provider.qml` starts all D-Bus,
  PipeWire, Niri and backlight workers at startup, whether or not any panel
  needs them ([Provider.qml:9](../../services/native/Provider.qml)).

### Measurement gap

The baseline recorded in
[baseline-environment.json](../performance/baseline-environment.json) captured
nothing (`available: false`). No live A/B comparison exists, so phase 0 is
mandatory.

## Target architecture

The principle: **events in, deltas out, typed rows, zero JSON, one owner per
domain.**

```text
 D-Bus / PipeWire / Niri socket / sysfs
          │  signal bodies applied in place (no refetch)
          ▼
 Domain actor (one per domain, owns its state, started by its own lease)
          │  Delta { inserted, removed, moved, changed(row, roles) }
          ▼
 Per-domain coalescer (≤1 queued Qt callback, merges deltas)
          │  CxxQtThread::queue
          ▼
 Typed Rust-owned QAbstractListModel (Vec<Row>, role enum)
          │  beginInsertRows / dataChanged(roles) only
          ▼
 QML delegates bind to roles directly. No get(i) loops, no JS reducers.
```

Rules:

1. **One owner per domain.** When a domain is native, its QML acquisition code
   is unloaded, not just bypassed. Fallbacks run in a `Loader` that is inactive
   while native is ready, so they keep no subscriptions or bindings alive.
2. **Leave existing native C++ in place.** Audio, battery, MPRIS and Bluetooth
   use Quickshell's C++ services. Rust domains for them are removed unless a
   measured gap justifies one. Policy (sink protection, charge limits,
   notifications) can move to Rust later as pure logic over Quickshell's
   objects, or stay in QML if it is cheap.
3. **Rust targets real QML hot spots.** These are process spawning (`nmcli`,
   `ddcutil`, `sh -c`), JSON parsing in JS, large JS reducers and sorts, and
   polling. Niri, Network, Brightness/DDC, config, app catalog and search,
   wallpapers, and ScreenTime are the candidates.
4. **No JSON at the Qt boundary.** Rows are `#[repr]` Rust structs that the
   model owns. `data(role)` is a `match` that returns a cheap `QVariant`.
   Commands are typed `#[qinvokable]` methods, for example
   `setVolume(node: u32, value: f64)` instead of `execute(id, json)`.
5. **Deltas, not snapshots.** Each actor keeps an indexed map
   (`IndexMap<Key, Row>`) and emits row-level changes with a changed-role mask.
   Full snapshots happen only at (re)connect.
6. **Only state that has a consumer is live.** Each domain and model has its
   own lease, taken by the panel or bar widget that shows it. The bar's
   network icon leases `network.summary`; the Wi-Fi list leases
   `network.accessPoints`, which is the only thing that starts AP tracking.

## Phases

Each phase ends with a measured A/B result against the previous phase. A
change that does not improve a metric, or that regresses one, is reverted.

### Phase 0: Measure (blocking)

**Goal:** Create an A/B baseline showing where the native backend loses time
vs. QML-only, and rank hot spots for phases 2–5.

#### Rust instrumentation

Add to `rust/inir-core/src/services/workers.rs`:
- `tracing::info!` on domain snapshot start/end with elapsed time
- Count D-Bus method calls per signal (track `GetAll`, `GetAllAccessPoints`,
  `GetManagedObjects`, `ListNames` per domain)
- Track "pending" signal coalescing delay and count (the 50 ms bucket)
- Expose a `/metrics` or `backend.metrics()` QML invokable that returns JSON

Add to `rust/inir-core/src/runtime.rs`:
- Track `reduce` duration and `DesktopState` clone size (bytes) per event
- Count events/s per domain

Add to `rust/inir-qt/src/qobjects/desktop.rs`:
- Time `apply()` on main thread (µs)
- Count JSON bytes serialized per collection

#### QML instrumentation

Add to `services/network/Network.qml` (after line 30):
```js
property int _debugApplyStart: 0
onNativeNetworkReadyChanged: {
  if (QS_DEBUG === "1") console.time("Network._applyNativeNetwork")
  // ...existing code...
  if (QS_DEBUG === "1") console.timeEnd("Network._applyNativeNetwork")
}
```

Add to `services/compositor/NiriService.qml` (EventStream socket message handler):
```js
if (QS_DEBUG === "1") console.time("Niri.event")
// ...apply event...
if (QS_DEBUG === "1") console.timeEnd("Niri.event")
```

#### Testing phases

**Phase 0a: Idle (baseline noise)**
```bash
INIR_RUST_BACKEND=0 QS_DEBUG=1 timeout 300 ./scripts/inir run --foreground \
  | grep -E 'console\.time|snapshot|D-Bus|apply' > /tmp/qml-only.log

INIR_RUST_BACKEND=1 QS_DEBUG=1 timeout 300 ./scripts/inir run --foreground \
  | grep -E 'console\.time|snapshot|D-Bus|apply' > /tmp/rust-on.log

diff <(sort /tmp/qml-only.log | uniq -c) <(sort /tmp/rust-on.log | uniq -c)
```

**Phase 0b: Interactive (Wi-Fi panel, volume, workspaces)**

For each backend setting:
```bash
# Wi-Fi scan for 30s
INIR_RUST_BACKEND=X ./scripts/inir run --foreground &
sleep 5 && ./scripts/inir request panel network
sleep 25 && ./scripts/inir request panel close
killall qs
# Collect logs into $BACKEND-wifi.log

# Volume slider (5 steps/sec for 30s = 150 events)
# Niri workspace switch ×50
# Play media ×2 players for 20s
```

#### Capture metrics

Output to `docs/performance/phase0-<date>.json`:
```json
{
  "date": "2026-10-08",
  "layout": "ii",
  "scenarios": {
    "idle": {
      "qml_only": {
        "wakeups_per_sec": 2.3,
        "apply_calls_per_sec": 0.5,
        "dbus_calls_per_sec": 1.2,
        "memory_mb": 185
      },
      "rust_on": {
        "wakeups_per_sec": 6.1,  // expect higher
        "apply_calls_per_sec": 3.2,  // duplicate work
        "dbus_calls_per_sec": 4.8,   // reread cost
        "memory_mb": 215
      }
    },
    "wifi_scan": { ... },
    "volume_slider": { ... },
    "workspace_switch": { ... }
  },
  "analysis": {
    "largest_gaps": [
      { "scenario": "wifi_scan", "metric": "dbus_calls", "ratio": 8.2, "reason": "full re-read on each strength change" },
      { "scenario": "idle", "metric": "wakeups", "ratio": 2.6, "reason": "Tokio + audio + Niri streams" },
      { "scenario": "volume_slider", "metric": "apply_time_us", "ratio": 3.1, "reason": "JSON encode/decode on main thread" }
    ]
  }
}
```

#### Decision gate

If `rust_on` on **every** metric is within 10% of `qml_only`, proceed to Phase 1.
If any metric regresses >20%, revert the entire native provider and pivot to Phase 6 (frontend).
If results are mixed, proceed to Phase 1 to stop duplicate work and remeasure.

### Phase 1: Stop paying twice (largest expected win, smallest change)

- Remove the Rust Audio, Battery, Media and Bluetooth domains from the
  runtime. Delete their workers and `DesktopServices` properties, and keep the
  code on a branch. Facades go back to Quickshell's services only.
- Niri: either fully cut over (Phase 3) or turn the Rust stream off. Do not
  run both.
- Replace the single `setServicesActive` lease with per-domain leases
  (`acquire("network")` returns a handle object, and destroying it releases
  the lease).
- Move each QML fallback path into a `Loader { active: !nativeReady }` so it
  holds no processes, sockets or Quickshell service bindings while native is
  ready.
- Gate: native-on is less than or equal to native-off on every Phase 0 metric.

### Phase 2: Rebuild the bridge

- Replace `RecordModel`/JSON with a generic CXX-Qt
  `QAbstractListModel` over `Vec<T: Row>`. A `Row` trait provides `key()`,
  `role_names()`, `data(role) -> QVariant` and
  `diff(&old) -> RoleMask`. Delete `service_models.cpp`.
- Use a `ModelDelta<T>` protocol (insert, remove, move, change with role mask)
  that the core computes off-thread. The main thread only applies it and emits
  notifications.
- Use per-domain `watch` channels with `Arc<DomainState>` (cheap clone), or
  delta queues. Delete the global `AppState` clone and the `last` clone in
  `DesktopServices`.
- Split `DesktopServices` into per-domain QObjects (`NativeNetwork`,
  `NativeNiri`, …) with typed invokables and per-property change signals. Drop
  the `*_revision` counters.
- Make the Tokio runtime `current_thread` on a single dedicated thread.
  PipeWire, if any remains, stays on its own loop.
- Gate: no `serde_json` or `QJsonDocument` on the GUI-thread path. The
  `apply` cost per event is under 50 µs for 100 rows, measured.

### Phase 3: Niri as the flagship domain

Niri is where QML is most expensive: JSON parsed in JS on every event, JS
sorting and layout reduction.

- Rust owns the event stream, layout-aware ordering (port
  `sortWindowsByLayout`), outputs, MRU and focus history, overview and
  keyboard layout.
- Title-only events update one row with the `title` role mask. Layout events
  update only geometry roles.
- Port `NiriService.qml`'s public API as a thin facade over the native models,
  then delete its socket and reducers.
- Keep one persistent action socket instead of connecting per action.
- Gate: workspace switches and title churn show at least a 50% reduction in
  main-thread time compared with the Phase 0 QML-only baseline.

### Phase 4: Network without refetching

- Bootstrap with one `GetManagedObjects` call (NetworkManager exposes
  ObjectManager at `/org/freedesktop`). After that, apply
  `PropertiesChanged`, `InterfacesAdded` and `InterfacesRemoved` bodies
  directly to the indexed state.
- Do SSID grouping and sorting in Rust, either as a derived model or a sort
  key role, and delete `_applyNativeNetwork`.
- Keep one long-lived system and session `Connection` and use typed `zbus`
  proxies built once. Filter `NameOwnerChanged` with `arg0` per service.
- Move the remaining `nmcli` paths (connect with password, profiles, VPN,
  hotspot) to D-Bus in the same actor so that `Network.qml`, `Vpn.qml` and
  `Hotspot.qml` stop spawning processes.

### Phase 5: Other process-heavy services

Ordered by Phase 0 ranking. Likely candidates:

- Brightness with native DDC/CI (`ddc-hi` or i2c-dev) instead of `ddcutil`
  processes.
- Config, using the existing native store. `Config.qml` JSON parsing and
  writes are likely large on startup and in settings.
- App catalog and search indexing.
- Wallpaper scanning and thumbnailing.
- ScreenTime and RecorderStatus polling.
- YtMusic and ShellUpdates process fan-out.

### Phase 6: Frontend

The backend cannot fix binding storms. Using the QML profiler output:

- Virtualize long lists.
- Remove `property var` arrays that are reassigned wholesale.
- Stop running animations when nothing is visible.
- Unload hidden panels.

This is most likely where the remaining frame-time cost lives.

## Architecture and language alternatives

| Option | Verdict |
| --- | --- |
| **Keep Rust + CXX-Qt, restructured as above** | **Recommended.** The current problems are design problems; Rust and CXX-Qt are not the bottleneck. |
| Hand-written C++ Quickshell plugin / upstream contributions | Worth it for narrow pieces. A native Niri module in Quickshell, like its Hyprland module, would benefit every consumer and remove the plugin. Consider it after Phase 3 proves the model. |
| Separate Rust daemon + IPC (D-Bus or socket) to the shell | Not recommended for performance: it adds serialization and wakeups. Only useful for crash isolation or sharing state with other clients. |
| Zig / C / Go for the backend | No benefit. Go adds GC pauses and a runtime; Zig and C add risk without removing the boundary cost. |
| Leave QML: Rust-native UI (Slint, iced + layer-shell, GTK4 + Relm4 + gtk4-layer-shell) | A full rewrite that gives up the existing shaders, animations and three layouts. Evaluate only if Phase 0 and Phase 6 show that the QML engine or scene graph itself is the bound **after** the backend fixes. A small spike (one bar plus one panel in Slint) could test that before committing. |

## Decommissioning

The existing migration gates still apply. In addition, no native domain ships
by default unless its Phase 0 A/B shows it is no slower than the QML path on
every metric, and its QML acquisition code is removed in the same release.
