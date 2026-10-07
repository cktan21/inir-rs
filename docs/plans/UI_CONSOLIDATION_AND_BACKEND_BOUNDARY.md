# UI De-Duplication and Backend Boundary Plan

## Status

| Milestone | State |
| :--- | :--- |
| 1.1 Unify system overlays | **Done** — polkit, OSD and session screen route by `panelFamily`; lock and region selector relocated into `modules/lock/{iris,waffle}/` and `modules/regionSelector/`. |
| 1.2 One settings application | **Done** — `waffleSettings.qml` and `modules/waffle/settings/` retired; navigation resolves string IDs through `SettingsPageRegistry.indexForKey` with numeric and alias fallbacks. |
| 1.3 Clean the root directory | **Done** — all eight moves landed. |
| 2.1 Audit & enforce single source of truth | **Done** — no QML under `modules/` runs a system command; enforced by `scripts/test-backend-boundary.py`. |
| 2.2 Define the Rust interface contract | **Contracts landed** for audio, brightness and battery in `rust/`, checked by `scripts/test-rust-contract.py`. Remaining domains are deliberately uncontracted pending the gates below. |
| 2.3 Unify configuration schema | **Done** — single `waffles` sub-object; 512 schema keys in sync. |

Two decisions are left to make rather than assume, both recorded under
[Open questions](#open-questions).

Sequencing note: `RUST_BACKEND_MIGRATION.md` treats a native backend as
**acceptance gates, not a mandatory rewrite** — a bridge spike (Gate 1) and
profiling that justifies a domain (Gate 3) come before any port. Milestone 2.2
is therefore scoped to the interface contract, not an implementation.

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
| **Lock Screen** | `modules/lock/` | `modules/iris/lock/` → `modules/lock/iris/`<br>`modules/waffle/lock/` → `modules/lock/waffle/` | Lock surfaces handle sensitive PAM authentication. PAM was already unified — only `modules/lock/LockContext.qml` touches it, and the family surfaces take it as a `required property LockContext context` — so this move was about ownership, not three bespoke authenticators. |
| **Polkit Auth** | `modules/polkit/` | `modules/iris/polkit/`<br>`modules/waffle/polkit/` | Privilege escalation dialogs should have one audited, reliable presentation layer. |
| **On-Screen Display** | `modules/onScreenDisplay/` | `modules/iris/onScreenDisplay/`<br>`modules/waffle/onScreenDisplay/` | Volume, brightness, and lock indicator popups should route through one unified controller. |
| **Region Selector** | `modules/regionSelector/` | `modules/iris/regionSelector/`<br>`modules/waffle/regionSelector/` | Screenshot and screen-crop tools should share identical geometry math and toolbars. |
| **Session Screen** | `modules/sessionScreen/` | `modules/iris/session/`<br>`modules/waffle/sessionScreen/` | Power operations (reboot, poweroff, lock, suspend) belong in one shared core dialog. |

#### Implementation Steps:
1. Re-wire `modules/iris/ShellIrisPanelsImpl.qml` and `modules/waffle/ShellWafflePanelsImpl.qml` to reference the shared core modules.
2. Remove the duplicated directories under `modules/iris/` and `modules/waffle/`.
3. Verify that IPC signals for OSD and lock invocation remain responsive across all 3 family configurations.

The shape this settled into: the canonical module owns a small `Scope` that
switches on `Config.options.panelFamily`, and each family's presentation moves in
beside it — flat when it is a single file (`IrisOSD.qml`, `IrisPolkitContent.qml`,
`IrisSessionScreen.qml`, `IrisOptionsToolbar.qml`, `WOptionsToolbar.qml`) and in a
subdirectory when it is a set (`onScreenDisplay/waffle/`, `lock/iris/`,
`lock/waffle/`). Imports become `qs.modules.lock.iris` rather than
`qs.modules.iris.lock`; same files, ownership inverted.

Three scripts hard-coded the old waffle lock paths and were updated with it:
`scripts/test-lock-wake-policy.js`, `scripts/test-idle-policy.js` and
`scripts/test-local-distribution.sh`.

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

This is enforced, not just stated: `scripts/test-backend-boundary.py` fails the
build on any such call, ignoring mentions inside comments. Clearing it moved
three concerns out of the UI layer:

| Was | Now |
| :--- | :--- |
| Three near-identical nmcli hotspot implementations in `modules/common/models/quickToggles/HotspotToggle.qml` and both sidebar toggle styles | One `services/network/Hotspot.qml`; the toggles are presentation only |
| `modules/pill/LinkWifi.qml` ran profile metadata, secret reveal, forget and password-connect itself | `Network.refreshProfileMetadata`, `forgetProfile`, `revealProfilePassword`, `connectWithPassword` |
| `modules/pill/LinkBt.qml` ran the bluetoothctl pair-trust-connect flow | `BluetoothStatus.pairDevice` with a `pairFinished` signal |

---

### Milestone 2.2: Define the Rust Interface Contract (CXX-Qt)
Formalize the property and method contracts for each service singleton so they can be replaced by compiled Rust types without changing QML consumers:

Contracts live in `rust/inir-backend/src/`, one bridge per service. The shape
below is the real CXX-Qt 0.10 syntax — properties are attributes on the QObject
type, not pseudo-`type` declarations, and the block is a safe `extern "RustQt"`:

```rust
#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qml_singleton]
        #[qproperty(bool, ready)]
        #[qproperty(f64, value)]
        #[qproperty(bool, mic_muted)]
        type AudioService = super::AudioServiceRust;

        #[qsignal]
        fn sink_protection_triggered(self: Pin<&mut AudioService>, reason: QString);

        #[qinvokable]
        fn toggle_mute(self: Pin<&mut AudioService>);
        #[qinvokable]
        fn set_sink_volume(self: Pin<&mut AudioService>, target: f64);
    }
}
```

Member names are the snake_case spelling of the QML ones; CXX-Qt emits the
camelCase Qt names layouts already bind to. Note that the contract follows the
**actual** service: `services/media/Audio.qml` exposes `value`, not `volume`, and
has no top-level `isMuted` or `sinkName` (mute state lives on `sink.audio`). The
earlier illustrative snippet named members that do not exist — exactly the drift
`scripts/test-rust-contract.py` now prevents.

See [`rust/README.md`](../../rust/README.md) for why the module registers as
`qs.services.native`, why registration is static for now, and which domains are
contracted.

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

# 1b. UI layers call services instead of running system commands
python3 -B scripts/test-backend-boundary.py

# 1c. CXX-Qt contracts still match the QML services they describe
python3 -B scripts/test-rust-contract.py

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

All 18 gate scripts under `scripts/test-*` pass. Compiling the Rust contract is
opt-in, since a shell-only checkout has no toolchain:

```bash
INIR_BUILD_RUST=1 bash scripts/test-local-distribution.sh
```

---

## Open questions

Two items are deliberately unresolved, because each changes behaviour users can
see and neither is settled by the plan.

### 1. Two hotspot profiles, or one?

The shell maintains two different access points, and they were left that way:

- **`Hotspot`** — created by `nmcli dev wifi hotspot` from `hotspot.ssid` /
  `hotspot.password` / `hotspot.band` in config. Drives the quick toggles.
- **`RicelinHotspot`** — a persistent AP-mode profile with `ipv4.method shared`,
  whose name and password are edited in place in the link pill and read back out
  of NetworkManager. Needs to survive being brought down, so it cannot be the
  one-shot form.

Both now live in `services/network/Hotspot.qml`, so nothing in `modules/`
executes `nmcli` — but one service owning two profiles is still the duplication
this plan set out to remove. Converging them means choosing which semantics
survive and migrating anyone with a saved `RicelinHotspot`, so it is a product
decision rather than a refactor.

### 2. How far should the Rust contracts go?

Audio, brightness and battery are contracted. The remaining services in the
Milestone 2.1 table — compositor, network, MPRIS, idle, VPN, bluetooth, resource
usage, config — are not. Extending coverage is cheap; what is not cheap is
deciding which members are a stable public contract versus incidental surface,
and `Config` in particular carries the cross-process writer problem documented in
`RUST_BACKEND_MIGRATION.md`. Worth doing per-domain, when that domain is actually
a candidate under Gate 3.

---

## Next Action
Resolve the two open questions above. No milestone work is outstanding.
