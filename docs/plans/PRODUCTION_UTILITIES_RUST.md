# Production utilities consolidation

Approved scope: all first-party production Python helpers, excluding the YouTube
Music stack. Preserve functionality, files and visual behavior; tests, profiling,
translation and installer-only tooling may remain Python. Existing Go theme
generators remain compiled Go, without Python fallbacks after cutover.

The machine-readable inventory is `sdata/runtime-helpers.json`. `ported` means a
Rust implementation exists, **not** that every caller has migrated. `legacy`
means migration remains required. A subsystem is `migrated` only after parity,
caller, packaging and lifecycle tests pass. Never claim completion with legacy
production callers left outside the explicit exception list.

## Architecture

- `inir-utils`: Qt-independent shared implementation and bounded job context.
- `inir-helper`: command frontend; raw stdin/stdout for clipboard, legacy-shaped
  direct commands, JSON envelopes for `job OPERATION`.
- `NativeUtilityJobs`: shared-backend asynchronous request/cancel/completion bridge.
- Heavy OpenCV/thumbnail/theme work belongs in an isolated on-demand imaging
  worker, never a mandatory dependency of the Qt plugin.
- Frequent monitors acquire leases; canceled/replaced jobs cannot publish stale
  results. No per-feature executor or always-on utility broker.

## Stages

1. Inventory, synthetic compatibility fixtures and binary delivery.
2. Clipboard, USB/sensors, system/file reads and input/development workers.
3. Surgical configuration operations, theme file updates and Bluetooth migration.
4. Shared HTTP, lyrics, voice/catalog/plugin adapters and dictionary/Anki storage.
5. Images, thumbnails, audio-source selection, recording and theme generation.
6. Managed startup migrations, runtime Python caller guard and decommissioning.

Existing scripts stay until their replacement passes acceptance; there is no
silent Python fallback after a subsystem is cut over. Do not delete user Python
environments or uninstall Python required by excluded features. Keep existing
data formats, config comments/unknown keys, clipboard bytes and caches compatible.

## Safety and verification

Use synthetic clipboard/sysfs/HTTP/config/archive fixtures. Live USB, keyboard,
clipboard, audio, lock/suspend and transfer checks require user approval. Never
restart the shell or replace a loaded plugin as part of an unattended build.
Keep CPU-heavy concurrency bounded, test process/thread teardown, and compare
helper launches, latency and PSS without promising a temperature improvement.

## Implemented batch (2026-10-15)

This is **not completion of the entire approved migration**. Implemented:

- Qt-free library and CLI, native asynchronous utility bridge, lazy QML facade,
  request IDs, deadlines, cancellation, a 64-request cap, two blocking workers
  and four concurrent network jobs. The native utility path works with the
  desktop-backend toggle disabled; fallback uses the same Rust implementation.
- Clipboard HTML5 cleanup and byte-preserving plain/binary filtering; history
  decode caller and new-login startup use Rust. Migration 043 only rewrites the
  managed watcher and does not restart it or alter existing history.
- Native USB/block event subscription on the existing Tokio reactor; lease
  release closes the socket. USB snapshots and sensor-path discovery use Rust.
- Native disk, explicit-shell-PID memory mappings and VPN byte-counter reads.
- Shared evdev Caps/Num Lock worker for backend/CLI, with no dedicated udev
  watcher thread; existing sysfs fallback retained. OSK and lock-keyboard labels
  use xkbcommon directly in bounded blocking jobs with stale-result guards.
- Cancellable lyrics requests with retry/search behavior and stale suppression.
- Development-only watcher port, identical-content suppression, debounce,
  parent lifetime, Quickshell reload-log guard, and scanner-reach refresh.
- CMake/Arch/Nix helper delivery and explicit source-installer build-before-copy.
  No runtime compilation and no silent Python fallback for these callers.

Original Python sources are deliberately retained as parity references and for
old sessions/startup entries until decommissioning gates pass. The installed
running shell/plugin have not been replaced or restarted by this work.

### Verification

Synthetic checks cover all HTML5 named entities, invalid numeric entities,
ordinary clipboard bytes, malformed nested markup, USB naming/classification,
multi-GPU sensor selection without temperature reads, seven xkb descriptions,
managed startup migration, and a development watcher with a stubbed IPC command.
HTTP fixtures cover rate-limit retry, fallback matching and connection teardown.
Native/QML tests check result conversion, duplicate/invalid requests,
cancellation, deadlines, disabled-desktop-backend operation and engine teardown.
Changed real components are compiled without constructing their windows/services.

Verified: 14 utility unit tests, 31 existing core/integration tests, 10 synthetic
compatibility/lifecycle checks, both isolated QML transports, 11 changed real QML
components, 10 runtime-payload tests, service/bridge contract checks and all four
CMake native tests in a release build. Utility/CLI Clippy passes with warnings
denied. A temporary-prefix CMake install delivers the standalone executable and
plugin; the real installation remains untouched. Live device/paste/lock behavior
and before/after process/temperature measurements remain untested.

The broad `test-local-distribution.sh` suite currently stops at its pre-existing
Orbit hot-corner string guard: it expects the old inline `liveWindows` expression,
but the unchanged Niri facade delegates it to the fallback. Do not change that
unrelated implementation just to satisfy this utility migration.

### Still pending

Super-key daemon/unit migration; surgical Niri/Hyprland configuration and keybind
parsers; Bluetooth migration CLI; AI catalog, voice, plugin metadata, dictionary,
Anki/deck adapters; image/region/thumbnail/Cava/recording helpers; theme generation,
Go binary delivery and the remaining inline Python callers. The production
Python removal guard cannot be enabled until these are ported.

The imaging stage cannot be built/tested here yet: `pkg-config opencv4` fails and
OpenCV development headers (including contrib/ximgproc) are absent. Native build
dependency installation needs explicit approval; image behavior must not be
silently replaced with a different algorithm to bypass that requirement.
