# UI De-Duplication and Backend Boundary Plan

## Executive Summary

iNiR currently bundles three competing desktop shell families (`ii`, `iris`, `waffle`) that each duplicate core system plumbing—including lock screens, authentication agents, on-screen displays, and settings applications.

This plan outlines a 2-step architectural refactoring roadmap:
1. **Step 1: UI De-Duplication & Root Cleanup** — Collapse duplicate system plumbing into a shared core, merge settings apps, and convert `ii`, `iris`, and `waffle` into purely visual layout presets (Bar, Launcher, Control Center).
2. **Step 2: Strict Backend Boundary (`qs.services`)** — Enforce a single source of truth across all QML layouts and lock down service interfaces to prepare for a drop-in compiled Rust backend via CXX-Qt.

---

## Target Architecture

```
                       ┌────────────────────────────────────────────────────────┐
                       │               3 Visual Layouts (Frontend)              │
                       │  - ii: Floating Pill + Android Quick Settings + Grid   │
                       │  - iris: Island Bar + Iris Orbit + Dock                │
                       │  - waffle: Bottom Taskbar + Start Menu + Action Center │
                       └───────────────────────────┬────────────────────────────┘
                                                   │
                                                   ▼
┌───────────────────────────────────────────────────────────────────────────────────────────────────────┐
│                                 SHARED SYSTEM UI & OVERLAYS (Core)                                    │
│   • Lock Screen (modules/lock)               • OSD (modules/onScreenDisplay)                          │
│   • Polkit Auth (modules/polkit)             • Screenshot Crop (modules/regionSelector)               │
│   • Session/Power (modules/sessionScreen)    • Single Settings App (modules/settings)                 │
└──────────────────────────────────────────────────┬────────────────────────────────────────────────────┘
                                                   │ (Property bindings & method calls ONLY)
                                                   ▼
┌───────────────────────────────────────────────────────────────────────────────────────────────────────┐
│                                   STRICT BACKEND BOUNDARY (qs.services)                               │
│        (Currently QML/Shell ──▶ Future drop-in replacement with compiled Rust CXX-Qt plugin)          │
│   • compositor/NiriService     • display/Brightness         • media/Audio & Mpris                     │
│   • power/Battery & Idle       • network/Network & Vpn      • system/ResourceUsage                    │
│   • config/Config (Single Serde authority)                                                            │
└───────────────────────────────────────────────────────────────────────────────────────────────────────┘
```

---

## Step 1: UI De-Duplication & Root Cleanup

### Milestone 1.1: Unify System Overlays into Shared Core
Currently, `lock`, `polkit`, `onScreenDisplay`, `regionSelector`, and `session` are written 3 separate times across the root and family directories.

| System Tool | Canonical Target | Redundant Copies to Remove | Justification |
| :--- | :--- | :--- | :--- |
| **Lock Screen** | `modules/lock/` | `modules/iris/lock/`<br>`modules/waffle/lock/` | Lock surfaces handle sensitive PAM authentication; running 3 bespoke implementations triples security risk and maintenance overhead (~220 KB of duplicated code). |
| **Polkit Auth** | `modules/polkit/` | `modules/iris/polkit/`<br>`modules/waffle/polkit/` | Privilege escalation dialogs should have one audited, reliable presentation layer. |
| **On-Screen Display** | `modules/onScreenDisplay/` | `modules/iris/onScreenDisplay/`<br>`modules/waffle/onScreenDisplay/` | Volume, brightness, and lock indicator popups should route through one unified controller. |
| **Region Selector** | `modules/regionSelector/` | `modules/iris/regionSelector/`<br>`modules/waffle/regionSelector/` | Screenshot and screen-crop tools should share identical geometry math and toolbars. |
| **Session Screen** | `modules/sessionScreen/` | `modules/iris/session/`<br>`modules/waffle/sessionScreen/` | Power operations (reboot, poweroff, lock, suspend) belong in one shared core dialog. |

#### Implementation Steps:
1. Re-wire `modules/iris/ShellIrisPanelsImpl.qml` and `modules/waffle/ShellWafflePanelsImpl.qml` to reference the shared core modules.
2. Remove the duplicated directories under `modules/iris/` and `modules/waffle/`.
3. Verify that IPC signals for OSD and lock invocation remain responsive across all 3 family configurations.

---

### Milestone 1.2: Consolidate Settings into ONE Application
Currently, Waffle launches an independent settings client (`waffleSettings.qml`) backed by `modules/waffle/settings/`, causing split schema authority.

1. **Fold Waffle Settings into Main Application**:
   * Migrate Waffle configuration toggles into `modules/settings/WaffleConfig.qml`.
   * Retire `waffleSettings.qml` and delete `modules/waffle/settings/`.
2. **Normalize Settings Navigation to Stable IDs**:
   * Replace fragile numeric index navigation (e.g. `page: 4`) in `modules/settings/SettingsPageRegistry.qml` with stable string IDs (e.g. `"appearance"`, `"niri"`, `"sound"`, `"waffle"`).
   * Update IPC routes in `shell.qml` to accept string IDs while preserving backward-compatible numeric aliases.

---

### Milestone 1.3: Clean the Root Directory
Evict massive UI monoliths and misplaced tooling from the repository root:

| Source File | Destination | Description |
| :--- | :--- | :--- |
| `welcome.qml` | `modules/welcome/WelcomeApp.qml` | 3,700-line first-run onboarding wizard. |
| `settings.qml` | `modules/settings/SettingsApp.qml` | Main settings application window. |
| `FamilyTransitionOverlay.qml` | `modules/common/FamilyTransitionOverlay.qml` | Runtime visual transition shader overlay. |
| `GlobalStates.qml` | `modules/common/GlobalStates.qml` | Global shell states singleton. |
| `ShellIiPanels.qml` | `modules/ii/ShellIiPanels.qml` | II family panel loader wrapper. |
| `ShellIrisPanels.qml` | `modules/iris/ShellIrisPanels.qml` | Iris family panel loader wrapper. |
| `ShellWafflePanels.qml` | `modules/waffle/ShellWafflePanels.qml` | Waffle family panel loader wrapper. |
| `go.mod` | `scripts/colors/go.mod` | Belongs in the theme generator subdirectory, not the repo root. |

---

## Step 2: Establish the Strict Backend Boundary (`qs.services`)

The goal of Step 2 is to eliminate direct shell/Python subprocess execution from the UI layer, enforcing a strict boundary where all layouts communicate exclusively through `qs.services.*`.

### Milestone 2.1: Audit & Enforce Single Source of Truth
Every visual component across all three families must bind only to the domain singletons organized in `services/`:

| Domain | Singleton | Exposed State & Capabilities |
| :--- | :--- | :--- |
| **Compositor** | `services/compositor/NiriService.qml` | Workspaces, active window titles, monitor geometries, IPC requests. |
| **Display** | `services/display/Brightness.qml` | Device backlights, brightness percentage, step increment/decrement. |
| **Media** | `services/media/Audio.qml`<br>`services/media/MprisController.qml` | PipeWire sinks/sources, volume, mute, MPRIS metadata and playback controls. |
| **Power** | `services/power/Battery.qml`<br>`services/power/Idle.qml` | UPower percentage, charging state, time remaining, idle inhibitor status. |
| **Network** | `services/network/Network.qml`<br>`services/network/BluetoothStatus.qml` | NetworkManager Wi-Fi scan list, active SSID, VPN state, Bluetooth devices. |
| **System** | `services/system/ResourceUsage.qml`<br>`services/system/SystemInfo.qml` | CPU load, memory pressure, temperatures, storage usage. |
| **Config** | `modules/common/Config.qml` | Reactive typed options, nested getter/setter facade, atomic flushes. |

**Strict Rule:** No QML file under `modules/` may execute `Quickshell.Io.Process` for system commands (`wpctl`, `nmcli`, `brightnessctl`, etc.). All operations must invoke methods on `qs.services.*`.

---

### Milestone 2.2: Define the Rust Interface Contract (CXX-Qt)
Formalize the property and method contracts for each service singleton so they can be replaced by compiled Rust types without changing QML consumers:

```rust
// Example CXX-Qt contract preview for Audio
#[cxx_qt::bridge]
mod qobject_audio {
    unsafe extern "RustQt" {
        #[qobject]
        type AudioService = super::AudioServiceRust;

        #[qproperty]
        type volume: f64;
        #[qproperty]
        type is_muted: bool;
        #[qproperty]
        type sink_name: QString;

        #[qinvokable]
        fn set_volume(self: Pin<&mut AudioService>, volume: f64);
        #[qinvokable]
        fn toggle_mute(self: Pin<&mut AudioService>);
    }
}
```

---

### Milestone 2.3: Unify Configuration Schema for Serde
1. Audit all keys stored in `~/.config/inir/config.json`.
2. Consolidate Waffle-specific keys under a clean `"waffle"` sub-object in the main schema.
3. Validate defaults against `scripts/test-iris-defaults.py`.
4. Ensure the schema maps 1:1 to a Rust `#[derive(Serialize, Deserialize)]` struct for Gate 4 of the native backend rollout.

---

## Validation Gates

After each phase, execute the verification suite:

```bash
# 1. Service layout and dependency encapsulation
python3 -B scripts/test-service-layout.py

# 2. JavaScript policy evaluation
node scripts/test-brightness-policy.js
node scripts/test-idle-policy.js

# 3. Static QML compatibility and syntax checks
python3 -B scripts/test-qml-pitfalls.py
python3 -B scripts/test-qml-qt-compat.py

# 4. Schema consistency
python3 -B scripts/test-iris-defaults.py

# 5. Full packaging and distribution test suite
bash scripts/test-local-distribution.sh
```

---

## Next Action
Begin execution with **Milestone 1.1: Unify System Overlays into Shared Core**.
