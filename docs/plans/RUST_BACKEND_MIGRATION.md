# iNiR Rust Backend & Settings Consolidation Plan

## Executive Mandate

This plan defines the architectural migration of iNiR's backend to compiled Rust and the overhaul of its settings system.

Implementation progress and outstanding acceptance checks are recorded in
[RUST_BACKEND_MIGRATION_STATUS.md](RUST_BACKEND_MIGRATION_STATUS.md).

**The Rust backend rewrite is mandatory.** iNiR's current architecture relies heavily on shell subprocesses (`nmcli`, `cat /sys`, CLI utilities), ad-hoc JavaScript state reducers, and loose QML singletons that generate unnecessary wakeups, CPU churn, and process forks.

The core mission is:

1. **Mandatory Rust Core (`inir-core` / `inir-qt`):** Rebuild state management, system integrations, IPC, configuration, and data models in compiled Rust.
2. **Decommission & Delete Legacy QML Services:** As each domain in Rust achieves feature parity and validation, its corresponding legacy QML service implementation (`services/network/Network.qml`, shell-out monitors, JS reducers) is permanently deleted.
3. **Retain & Optimize the QML Frontend:** Keep QML for what it does best—declarative layout, shader effects, fluid animations, and visual presentation—while aggressively eliminating dormant allocations, expensive bindings, and unvirtualized views.
4. **Settings 2.0:** Revamp the fragmented settings taxonomy into a unified, desktop-environment control center backed by typed Rust schemas and metadata.

```text
                                    iNiR Architecture

                         QML Frontend Layer (Quickshell)
         ┌─────────────────────────────────────────────────────────────┐
         │ Visual Layouts: ii / iRiS / Waffle                          │
         │ Overlays: Lock / Polkit / OSD / Session / Welcome           │
         │ Settings 2.0: Desktop Control Center & Custom Domain Pages  │
         └──────────────────────────────┬──────────────────────────────┘
                                        │
                          Qt / QML Dynamic Plugin
                             via CXX-Qt (cdylib)
                                        │
         ┌──────────────────────────────▼──────────────────────────────┐
         │                          inir-qt                            │
         │   • CXX-Qt QObjects (SystemInfo, Network, Power, Niri)      │
         │   • QAbstractItemModel wrappers (incremental updates)       │
         │   • Thread-safe signal marshaling to Qt Main Event Loop     │
         └──────────────────────────────┬──────────────────────────────┘
                                        │
         ┌──────────────────────────────▼──────────────────────────────┐
         │                         inir-core                           │
         │   • Tokio Async Runtime & Channel Event Loop                │
         │   • Unified AppState & Granular Event Dispatcher            │
         │   • Typed Serde Configuration & Multi-Process Writer        │
         │   • Consumer-Aware Service Controllers                      │
         └───────┬──────────────┬──────────────┬──────────────┬────────┘
                 │              │              │              │
                 ▼              ▼              ▼              ▼
           NetworkManager     BlueZ          UPower         Niri
               (zbus)         (zbus)     (zbus + sysfs)     (IPC)
                 │              │              │              │
                 ├──── systemd  ├──── MPRIS    ├──── PipeWire └──── procfs/sysinfo
```

### Final Responsibility Boundary

| Domain                 | Rust (`inir-core` + `inir-qt`)                                       | QML (`modules/`, `services/`)                                 |
| :--------------------- | :------------------------------------------------------------------- | :------------------------------------------------------------ |
| **System Integration** | D-Bus (zbus), sysfs, Niri Unix socket, PipeWire, systemd, procfs     | Passive consumer of exported Qt properties and models         |
| **State Management**   | Authoritative `AppState`, diffing, caching, MRU order                | Transient presentation state only (focus, active drawer)      |
| **Configuration**      | Serde structs, migrations, validation, disk I/O, cross-process locks | Schema-driven controls, property bindings                     |
| **Data Models**        | `QAbstractItemModel` implementations with granular row diffs         | `ListView` / `GridView` delegate presentation                 |
| **Concurrency**        | Tokio multi-threaded worker pool, cancellation, rate limiting        | Main UI thread rendering                                      |
| **Presentation**       | None                                                                 | Layout, themes, animations, shaders, gestures, audio playback |

---

## Phase 1 — Performance Baseline & Measurement (M0)

Before modifying existing services, establish reproducible baselines using `scripts/inir doctor --perf`, `pidstat`, `strace`, and `perf`.

### Measurement Targets

| Metric                    | Baseline Assessment                                  | Rust Target                               |
| :------------------------ | :--------------------------------------------------- | :---------------------------------------- |
| **Idle CPU**              | 1.5% – 5.0% (polling / subprocess churn)             | **< 0.5%**                                |
| **Idle GPU**              | Continuous rendering on dormant shaders              | **0% (no frame wakeups)**                 |
| **RSS Memory**            | Variable, bloated by duplicated JS trees             | **-35% to -50% reduction**                |
| **Wakeups / sec**         | High (500ms JS timers, `nmcli monitor`)              | **< 5 / sec idle**                        |
| **Processes Spawned**     | 10–30 forks/min (sysfs `cat`, `nmcli`, `hyprsunset`) | **0 forks/min idle**                      |
| **Shell Cold Startup**    | Measured via `shell.qml` boot markers                | **30–50% faster**                         |
| **Settings Open Latency** | High (entire monolith parsed on open)                | **< 80 ms perceived**                     |
| **Frame-time p95 / p99**  | Frame drops during network scans or window switches  | **< 8.3 ms (120 Hz) / < 16.6 ms (60 Hz)** |

### Baseline Profiling Commands

```bash
# Monitor subprocess fork rates
strace -f -e trace=clone,clone3,execve -p $(pgrep -n quickshell)

# Process CPU and wakeup rates
pidstat -p $(pgrep -n quickshell) -u -r 1

# Profile CPU hotspots across threads
perf record -g -p $(pgrep -n quickshell) -- sleep 10
perf report
```

---

## Phase 2 — The Rust Core & CXX-Qt Dynamic Bridge (M1)

### Workspace Layout

Create the native workspace under `rust/`:

```text
rust/
├── Cargo.toml
├── CMakeLists.txt
├── inir-core/
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs
│       ├── state.rs
│       ├── events.rs
│       ├── config/
│       └── services/
│           ├── network/
│           ├── bluetooth/
│           ├── compositor/
│           ├── power/
│           └── system/
├── inir-qt/
│   ├── Cargo.toml
│   ├── build.rs
│   └── src/
│       ├── lib.rs
│       └── qobjects/
│           ├── system_info.rs
│           ├── network.rs
│           ├── niri.rs
│           └── config.rs
└── inir-types/
    ├── Cargo.toml
    └── src/
        ├── lib.rs
        └── schema.rs
```

### Technology Stack

- **CXX-Qt (0.7+)**: Compiles a Qt dynamic C++ library (`cdylib`) with QML type registrations (`qmlRegisterSingletonType`, `qmlRegisterType`).
- **Tokio**: Multi-threaded async runtime managing background I/O without blocking Qt's main thread.
- **zbus (4.x)**: Pure Rust asynchronous D-Bus client for NetworkManager, BlueZ, UPower, and systemd.
- **serde / serde_json**: Serialization, typed schemas, and versioned migrations.
- **tracing / tracing-subscriber**: Structured logging matching `inir doctor` levels.

### Build and Packaging Contract

The transition to Rust requires updating build scripts and distributions:

1. **Arch Linux (`PKGBUILD`)**:
   - Update `arch=(any)` to compiled targets: `arch=(x86_64 aarch64)`.
   - Build phase runs `cargo build --release` / `cmake --build` to produce `libinir_qt.so`.
   - Never ship raw `target/` build directories.
2. **Nix Packaging (`nix/package.nix`)**:
   - Replace `stdenvNoCC` with `rustPlatform.buildRustPackage` or standard native derivations.
   - Update `nix/runtime-source-filter.nix` to compile and install the dynamic library into `lib/qt-6/qml/qs/services/native/`.
3. **QML Module Registration**:
   - Supply the QML import root via `scripts/inir` using `QML_IMPORT_PATH`.
   - Provide proper `qmldir` and `.qmltypes` metadata so tooling, LSP, and Quickshell recognize the native singleton exports.
4. **Thread Safety & Teardown**:
   - Background tasks must use `cxx_qt::CxxQtThread::queue` to marshal updates to the Qt main thread.
   - Implement clean worker cancellation on shell reload or process termination; prevent dangling pointers to invalidated QObjects.

### Bridge Verification Spike

The bridge gate completes when a minimal `SystemInfo` QObject (`hostname`, `uptime`, `memoryTotal`) loads dynamically inside Quickshell, updates over time without memory leaks, survives repeated shell reloads, and renders in QML.

---

## Phase 3 — Unified Application State & Event Architecture

Avoid fragmented singletons that communicate through ad-hoc signals. Implement an authoritative, centralized state in `inir-core`:

```rust
pub struct AppState {
    pub network: NetworkState,
    pub bluetooth: BluetoothState,
    pub audio: AudioState,
    pub power: PowerState,
    pub niri: NiriState,
    pub media: MediaState,
    pub resources: ResourceState,
    pub config: Config,
}

pub enum Event {
    Network(NetworkEvent),
    Bluetooth(BluetoothEvent),
    Audio(AudioEvent),
    Power(PowerEvent),
    Niri(NiriEvent),
    Media(MediaEvent),
    Resource(ResourceEvent),
    Config(ConfigEvent),
}
```

### Event Dispatch Loop

```text
System Signal (D-Bus / Socket)
              │
              ▼
   Tokio Worker Task
              │ (mpsc channel)
              ▼
   Core Event Reducer
              │
              ▼
   Updates AppState Field
              │
              ▼
   CxxQtThread::queue
              │
              ▼
   Emits Target Qt Signal (e.g. wifiSignalChanged, activeWindowChanged)
              │
              ▼
   QML updates only bound delegate (NO generic stateChanged storm)
```

---

## Phase 4 — Incremental Service Migration & Legacy Deletion

### Mandatory Decommissioning Protocol

For each service migrated to Rust:

1. **Implement Rust Service** in `inir-core` with full zbus/socket integration.
2. **Expose CXX-Qt Facade** matching the existing public API in `services/qmldir`.
3. **Run Parallel Validation** behind an environment flag (`INIR_RUST_BACKEND=1`).
4. **Benchmark & Verify Parity** against all edge cases (reconnects, secrets, errors).
5. **Switch Default to Rust**.
6. **Permanently Delete Legacy QML Implementation** and remove subprocess dependencies.

---

### 4.1 Network (M2)

- **Legacy Problem:** `services/network/Network.qml` and `Vpn.qml` spawn `nmcli monitor` and shell sub-processes, parsing stdout strings. This causes constant CPU wakeups and high memory allocations.
- **Rust Architecture:**
  - Connect to NetworkManager via `zbus`.
  - Listen for D-Bus property change signals on `/org/freedesktop/NetworkManager`.
  - Expose typed access point models with RSSI, security protocols, frequency, and connection state.
  - Implement full feature parity: saved profiles, Wi-Fi scanning, enterprise authentication, hotspot creation, VPN management.
- **Legacy Removal Target:**
  - Delete `services/network/Network.qml`.
  - Delete `services/network/Vpn.qml`.
  - Eliminate `nmcli` subprocess calls across the codebase.

---

### 4.2 Bluetooth & Power Profiles (M3)

- **Legacy Problem:** Polling and disjointed UPower / BlueZ handling across QML helpers. Shell scripts spawn `cat /sys/class/power_supply/...` for charge limits.
- **Rust Architecture:**
  - Direct BlueZ D-Bus integration for adapter discovery, device pairing, and connection states.
  - UPower D-Bus integration for battery percentages, time-to-empty, and charging status.
  - Direct kernel `sysfs` reading in Rust for battery charge thresholds (`/sys/class/power_supply/BAT*/charge_control_end_threshold`).
  - Polkit integration for privileged threshold writes.
- **Legacy Removal Target:**
  - Delete `services/network/BluetoothStatus.qml`.
  - Delete `services/power/Battery.qml`, `services/power/Idle.qml`, `services/power/idlePolicy.js`.
  - Delete shell scripts for reading/writing sysfs charge limits.

---

### 4.3 Niri Compositor State Engine (M4)

- **Legacy Problem:** `services/compositor/NiriService.qml` reads the Niri event stream, parsing JSON in JavaScript and managing workspace tracking, window MRU lists, and focus history in QML objects.
- **Rust Architecture:**
  - Maintain a persistent Unix domain socket connection to Niri's event socket.
  - High-performance deserialization via `serde_json` directly into typed structs.
  - Maintain workspace state, focused window ID, window titles, and MRU ordering in Rust memory.
  - Expose Qt `QAbstractListModel` for windows and workspaces with proper `rowsInserted`, `rowsRemoved`, and `dataChanged` signals instead of recreating dynamic JS arrays.
- **Legacy Removal Target:**
  - Delete `services/compositor/NiriService.qml`.
  - Delete `services/compositor/DankSocket.qml`.
  - Delete `services/compositor/HyprlandData.qml`.

---

### 4.4 System Resource Monitoring (M7)

- **Legacy Problem:** `services/system/ResourceUsage.qml` runs continuous timers parsing `/proc/stat` and `/proc/meminfo` via JavaScript or subprocesses even when panels are hidden.
- **Rust Architecture:**
  - High-performance async sampling of CPU, RAM, GPU, and disk throughput using `sysinfo` or direct `/proc` reads.
  - **Consumer-Aware Polling:** Rust suspends resource sampling entirely when no UI panel (dashboard, bar graph) is visible.
- **Legacy Removal Target:**
  - Delete `services/system/ResourceUsage.qml`.
  - Delete `services/system/SystemInfo.qml`.
  - Delete `services/system/MemoryPressureService.qml`.

---

### 4.5 Display, Brightness & Media (M7)

- **Rust Architecture:**
  - Native sysfs backlight controller (`/sys/class/backlight`) with DDC fallback.
  - MPRIS D-Bus client for player control, metadata, and position tracking.
  - High-performance PipeWire client stream monitoring.
- **Legacy Removal Target:**
  - Delete `services/display/Brightness.qml`, `Hyprsunset.qml`, `brightnessPolicy.js`.
  - Delete `services/media/Audio.qml`, `MprisController.qml`.

---

## Phase 5 — Typed Configuration & Cross-Process Management (M5)

### Rust Configuration Schema

Replace unstructured JSON manipulation with versioned, typed Serde structs:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub schema_version: u32,
    pub appearance: AppearanceConfig,
    pub desktop: DesktopConfig,
    pub network: NetworkConfig,
    pub sound: SoundConfig,
    pub power: PowerConfig,
    pub niri: NiriConfig,
    pub iris: IrisConfig,
    pub waffle: WaffleConfig,
    pub performance: PerformanceConfig,
}
```

### Cross-Process Synchronization

iNiR runs the main desktop shell and an optional standalone Settings window as separate processes.

- **Single Writer / IPC Locking:** Use file advisory locking (`flock`) combined with atomic write-and-rename (`tempfile` + `rename`) to guarantee zero file corruption.
- **Inotify Live Reload:** `inir-core` monitors the configuration file via `inotify`; external writes (e.g. user editing in an editor or standalone Settings saving changes) trigger an instant reload and Qt signal emission.
- **Automated Version Migrations:** Config files carry `schema_version`. On startup, migrations execute sequentially (`v1 -> v2 -> v3`), creating an automatic backup (`config.json.bak`) before saving the migrated schema.
- **Compatibility Facade:** During migration, `modules/common/Config.qml` continues to expose `options`, `ready`, and `configChanged` to preserve existing frontend bindings.

---

## Phase 6 & 7 — Settings 2.0: Revamped Desktop Control Center (M6)

### Desktop-Class Taxonomy

Retire the fragmented, module-centric settings pages in favor of a coherent desktop-environment control center:

```text
Settings 2.0
│
├── Network & Connectivity
│   ├── Wi-Fi (Scanned APs, saved networks, hidden SSID, security)
│   ├── Ethernet (IP address, DHCP/Static, DNS, speed)
│   ├── Bluetooth (Paired devices, discoverable toggle, battery levels)
│   ├── VPN (WireGuard, OpenVPN profiles)
│   ├── Hotspot (SSID configuration, passphrase, band selection)
│   └── Proxy & DNS
│
├── Sound
│   ├── Output (Device selection, volume slider, balance, test sound)
│   ├── Input (Microphone selection, input level, noise suppression)
│   ├── Applications (Per-application stream mixer)
│   └── Sound Effects (System alerts, feedback)
│
├── Displays & Input
│   ├── Displays (Arrangement, resolution, refresh rate, scaling, VRR)
│   ├── Night Light / Color temperature
│   ├── Keyboard (Layouts, repeat delay/rate, shortcuts)
│   ├── Mouse & Touchpad (Acceleration, natural scrolling, gestures)
│   └── Drawing Tablet / Stylus
│
├── Power
│   ├── Battery & Health (State, discharge rate, capacity)
│   ├── Power Profiles (Performance, Balanced, Power Saver)
│   ├── Sleep & Inactivity timeouts
│   └── Charge Limits (Battery preservation threshold sliders)
│
├── Personalization
│   ├── Wallpaper (Image picker, multi-monitor assignment, slideshow)
│   ├── Colors & Accents (Material 3 palettes, dark/light mode)
│   ├── Themes (iNiR themes, icon sets, cursor theme)
│   ├── Typography (System fonts, monospace fonts, sizes)
│   ├── Window Decoration & Borders
│   └── Animation presets
│
├── Desktop & Layouts
│   ├── Family Selection (ii / iRiS / Waffle)
│   ├── Bar & Dock configuration
│   ├── Workspaces (Dynamic/fixed, layout policies)
│   ├── Notifications (DND, history, position, priority apps)
│   ├── App Launcher & Search providers
│   └── Lock Screen & Security
│
├── Apps & Services
│   ├── Default Applications (Browser, terminal, file manager, editor)
│   ├── Autostart entries
│   ├── Weather service configuration
│   └── Media playback integration
│
└── System
    ├── Performance & Rendering (Blur tiers, power-saving mode)
    ├── Diagnostics & Telemetry (Real-time CPU/GPU, frame-times, logs)
    ├── Niri Compositor Settings
    ├── System Updates & Package Management
    └── About (Version, system info, hardware specs)
```

### Schema-Driven UI Generation

For scalar configuration fields, Rust generates schema metadata:

```rust
pub struct SettingField {
    pub key: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    pub kind: SettingKind,
}

pub enum SettingKind {
    Boolean,
    Slider { min: f64, max: f64, step: f64 },
    Choice { options: Vec<&'static str> },
    Color,
    String,
}
```

A reusable QML component set (`SettingToggle.qml`, `SettingSlider.qml`, `SettingChoice.qml`) renders standard fields automatically from the schema, eliminating thousands of lines of boilerplate QML. Rich domain views (Display layout canvas, Wi-Fi network selector, Audio mixer) use dedicated QML components bound directly to Rust models.

---

## Phase 8 & 9 — QML Frontend & Rendering Optimizations (M8, M9)

### Frontend Performance Rules

1. **Lazy Loading via Asynchronous Loaders:**
   - **Rule:** Closed heavy components must not exist in memory.
   - Replace `visible: false` on major panels (Settings, Launcher, Wallpaper selector, Dashboard, Audio mixer) with:
     ```qml
     Loader {
         active: PanelController.isOpen
         asynchronous: true
         sourceComponent: HeavyPanelComponent {}
     }
     ```
2. **Virtualization of Large Collections:**
   - Long lists (installed applications, Wi-Fi networks, clipboard history, wallpaper thumbnails) must instantiate only visible items plus an overscan buffer.
   - Forbid creating hundreds of QML item delegates upfront.
3. **Binding Reduction & Flattening:**
   - Eliminate JavaScript array transformations (`.map()`, `.filter()`, `.sort()`) in property bindings.
   - Perform data manipulation in Rust and expose clean, filtered `QAbstractListModel` instances.
   - Avoid deep property chains (`Config.options?.appearance?.theme?.colors?.primary`).
4. **Animation Discipline:**
   - Animations must be active only during user interaction or transitions.
   - Idle state must feature zero running timers, zero continuous frame callbacks, and zero continuous canvas repaints.
5. **Blur & Effect Performance Tiers:**
   - Implement rendering tiers in `modules/common/`:
     - **Performance:** No live wallpaper blur, static pre-rendered shadows, reduced transitions, disabled background effects. Automatically activated during GameMode or on battery.
     - **Balanced:** Standard blur on active popups, optimized shadows.
     - **Quality:** Full real-time blur, fluid multi-layer shadows, complex shader transitions.

---

## Phase 10 — Built-In Diagnostics & Telemetry

Integrate first-class observability into iNiR:

- **CLI Tools:**
  - `inir doctor`: Health check verifying dependencies, D-Bus services, and permissions.
  - `inir profile`: Dumps live CPU/GPU utilization, active QML object counts, and event throughput.
  - `inir services`: Lists active Rust background tasks, Tokio thread pool stats, and D-Bus listeners.
- **Settings Diagnostics Page:**
  - Located at **Settings → System → Diagnostics**.
  - Real-time performance monitors: CPU %, GPU %, RSS Memory, Frame-time p50/p95/p99, active sub-processes, and loaded QML modules.

---

## Phase 11 — Decommissioning & Legacy File Removal

Once a Rust service and its corresponding Settings 2.0 integration are validated, the legacy implementations must be permanently deleted:

| Milestone | Target to Delete                                                                                                                                                                                                               | Replacement                                                                                     |
| :-------- | :----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | :---------------------------------------------------------------------------------------------- |
| **M1**    | Redundant helper scripts                                                                                                                                                                                                       | `inir-qt` dynamic library                                                                       |
| **M2**    | `services/network/Network.qml`<br>`services/network/Vpn.qml`                                                                                                                                                                   | `inir-core::services::network` (zbus)                                                           |
| **M3**    | `services/network/BluetoothStatus.qml`<br>`services/power/Battery.qml`<br>`services/power/Idle.qml`<br>`services/power/idlePolicy.js`                                                                                          | `inir-core::services::bluetooth`<br>`inir-core::services::power`                                |
| **M4**    | `services/compositor/NiriService.qml`<br>`services/compositor/DankSocket.qml`<br>`services/compositor/HyprlandData.qml`                                                                                                        | `inir-core::services::niri` (IPC + Serde)                                                       |
| **M5**    | Ad-hoc JSON writing scripts                                                                                                                                                                                                    | `inir-core::config` (Typed Serde + inotify)                                                     |
| **M6**    | `modules/waffle/settings/`<br>`waffleSettings.qml`<br>Fragmented settings pages                                                                                                                                                | Unified `Settings 2.0` application with stable IDs                                              |
| **M7**    | `services/system/ResourceUsage.qml`<br>`services/system/SystemInfo.qml`<br>`services/system/MemoryPressureService.qml`<br>`services/display/Brightness.qml`<br>`services/display/Hyprsunset.qml`<br>`services/media/Audio.qml` | `inir-core::services::system`<br>`inir-core::services::display`<br>`inir-core::services::media` |

---

## Migration Roadmap & Acceptance Gates

|  Gate   | Title                                | Deliverables & Exit Criteria                                                                                                                                     |
| :-----: | :----------------------------------- | :--------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **M0**  | **Baseline Profiling**               | Record CPU, GPU, RSS, process spawns, and frame-times across ii/iris/waffle layouts using `pidstat` and `perf`.                                                  |
| **M1**  | **Rust/QML Bridge Spike**            | CMake + CXX-Qt builds `libinir_qt.so`. `SystemInfo` QObject exposed to QML. Dynamic plugin loading verified in Quickshell without crashes on reload.             |
| **M2**  | **Rust Network Service**             | NetworkManager zbus integration complete. Wi-Fi scanning and connections operational in QML UI. `services/network/Network.qml` deleted. Zero `nmcli` spawns.     |
| **M3**  | **Bluetooth & Power**                | BlueZ and UPower zbus clients active. Direct sysfs charge threshold reads. Delete legacy battery and bluetooth QML services.                                     |
| **M4**  | **Niri State Engine**                | Niri IPC event stream parsed in Rust. Windows and workspaces exposed via `QAbstractListModel`. Delete `NiriService.qml` and `DankSocket.qml`.                    |
| **M5**  | **Typed Configuration**              | Strongly typed Rust config structs with Serde. File locking and inotify watching prevent lost updates between shell and standalone settings.                     |
| **M6**  | **Settings 2.0 & Schema Engine**     | Full 8-category desktop control center implemented. Schema-driven rows deployed. Stable string navigation IDs. Retire legacy settings pages.                     |
| **M7**  | **Remaining Native Services**        | Resource monitoring with consumer-aware polling, sysfs brightness, PipeWire audio, and MPRIS media migrated to Rust. Delete remaining legacy `services/*` files. |
| **M8**  | **QML Frontend Optimization**        | Heavy surfaces wrapped in asynchronous `Loader`s. Virtualization implemented for lists. Bindings flattened. Zero running animations when idle.                   |
| **M9**  | **Rendering & Blur Pipeline**        | Performance tiers implemented (Performance / Balanced / Quality). GameMode triggers cheap rendering path.                                                        |
| **M10** | **Performance Release Verification** | Re-run benchmark suite. Verify idle CPU < 0.5%, zero idle forks, 30–50% faster startup, and zero frame drops. Final documentation and release.                   |
