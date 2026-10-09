# Frontend memory optimization

Status: proposed (2026-10-09). Companion to
[NATIVE_BACKEND_EFFICIENCY_REVAMP.md](NATIVE_BACKEND_EFFICIENCY_REVAMP.md),
which covered CPU. This plan covers **resident memory (RSS/PSS) of the
`qs` shell process**, and is a concrete realization of that plan's Phase 6
(frontend), scoped to RAM.

## Why this is a frontend plan, not a backend one

The Rust backend holds only kilobytes of state (a handful of access points,
power profiles, backlights). Phase 1 already trimmed shell RSS slightly
(~700→685 MB range). **The backend cannot move the shell's memory.** The
shell's resident memory lives almost entirely in the QML/Qt frontend: the JS
heap, the scene graph, GPU staging buffers, blur framebuffers, wallpaper
pixmaps, and panels kept instantiated while closed.

## Measured baseline (2026-10-09)

Profiled on the live Niri session, single output, panel family `ii`.

**Process accounting (why "12.5 GB used" is mostly not the shell):**

| Layer | Memory |
| --- | --- |
| True app memory, all processes (PSS) | 8.5 GB |
| Kernel (slab, page tables, kernel stacks) | ~0.5 GB |
| Shared memory / GPU buffers (Shmem) | ~0.2 GB |
| `used` as reported (= total − free − cache) | 12.5 GB |

Summing raw RSS across all processes gives 11.3 GB, but RSS double-counts
shared libraries; **PSS (8.5 GB) is the honest per-app total.** `MemAvailable`
was 10.5 GB — the machine was not memory-starved; the 3.9 GB of swap in use was
pressure from concurrent Rust compilation, not steady state.

**Top consumers (RSS):** zed-editor 1.5 GB, rust-analyzer 1.0 GB (dev-only,
exits with the editor), **qs 0.9 GB**, browser (zen + web content) ~3.2 GB.

**The `qs` process itself:**

| Metric | Value |
| --- | --- |
| RSS | 952 MB |
| **PSS** | **849 MB** |
| **Private_Dirty (anonymous heap — the reducible target)** | **759 MB** |
| Private_Clean | 61 MB |
| Shared_Clean (Qt/system libs, shared with other apps) | 132 MB |
| Swap | 0 |

The 759 MB of private anonymous memory is the goal. Largest single anonymous
mapping was 91 MB (JS heap or a texture atlas); the rest is a long tail of
15–25 MB mappings consistent with per-surface framebuffers, blur buffers and
decoded pixmaps.

## Profiled offenders (ranked by expected impact)

Ranking is by expected MB recovered against implementation risk. Each must be
confirmed with a before/after measurement during implementation — a change that
does not move Private_Dirty is reverted.

### 1. Blur / wallpaper framebuffers (largest expected win, medium risk)

- `modules/background/Background.qml` is 183 KB and owns animated blur, video
  wallpaper, GIF wallpaper and per-output rendering. The many 15–25 MB
  anonymous mappings are consistent with its blur/wallpaper framebuffers.
- `Variants { model: Quickshell.screens }` ([Background.qml:464](../../modules/background/Background.qml))
  replicates the background per monitor; every output multiplies the blur and
  wallpaper buffer cost. One output today, but it scales badly.
- `enableAnimatedBlur` keeps a live blur pipeline rendering; blur backbuffers
  are full-surface RGBA (width×height×4 bytes, often doubled for ping-pong).
- Levers to evaluate (measure each): downscale the blur backbuffer (blur at
  half/quarter resolution then upscale — visually near-identical), drop the blur
  pipeline to a single buffer when static, release wallpaper/blur buffers for
  occluded or non-focused outputs, and ensure video/GIF wallpaper decoders are
  torn down when the wallpaper is static.

### 2. Panels resident from boot (cheapest win, low risk)

Five `LazyLoader`s are gated on `active: Config.ready`, i.e. instantiated at
startup and **never released**, though they are rarely-used surfaces
([shell.qml:726-729,900](../../shell.qml)):

- `modules/regionSelector/RegionSelectorRouter.qml`
- `modules/japaneseLookup/JapaneseLookup.qml`
- `modules/tilingOverlay/TilingOverlayRouter.qml`
- `modules/wallpaperSelector/WallpaperSelectorRouter.qml`
- `modules/closeConfirm/CloseConfirm.qml`

Lever: gate each on its actual open/trigger state (an IPC/GlobalStates flag)
instead of `Config.ready`, so a closed panel holds zero scene graph. Keep a
warm-cache path only where first-open latency is proven to matter. The
`wallpaperSelector` in particular also loads thumbnails (see #3).

### 3. List delegates not reused (medium win, low-medium risk)

81 files use `ListView`/`GridView`; **only 1 sets `reuseItems`**. Large models
instantiate and retain a delegate per row:

- Wallpaper selector thumbnails, overview (`modules/overview`, 17 files),
  clipboard history, emoji picker, app launcher.
- Levers: set `reuseItems: true`, bound `cacheBuffer`, and cap thumbnail
  decode via `sourceSize` (most Images already set it — 128/135 — so decode is
  mostly fine; the gap is delegate retention, not decode).

### 4. Always-resident large QML singletons (smaller win, low risk)

Singletons loaded for the whole session carry their full object tree:
`modules/common/Config.qml` (267 KB), `modules/common/ThemePresets.qml`
(150 KB). Settings trees are large too — `DesktopWidgetsConfig.qml` (335 KB),
`iris/settings/IrisOptions.qml` (242 KB), `BarConfig.qml` (148 KB) — these are
only a problem if kept warm after the settings window closes; confirm they are
released, not pooled.

### 5. Image decode — already handled (no action)

128 of 135 Image-using files set `sourceSize`, so full-resolution decode into
RAM is largely avoided. Wallpaper source assets are 1–3 MB on disk but decode
to surface resolution regardless; covered under #1, not here.

## Phases

Each phase ends with a before/after of `qs` `Private_Dirty` (from
`/proc/<pid>/smaps_rollup`) under a fixed scenario (idle, then open+close each
major panel once). A change that does not reduce resident memory, or regresses
first-open latency unacceptably, is reverted.

### Phase 0: Instrument and attribute (blocking)

- Record the baseline above reproducibly: a script that samples
  `smaps_rollup` (`Pss`, `Private_Dirty`) for `qs` at idle and after a scripted
  panel open/close cycle.
- Attribute the anonymous heap to QML where possible: enable
  `QSG_RHI_PROFILE`/scene-graph stats or `QML_DISK_CACHE`/GC logging, and use
  Qt's `QtQuick` memory hints to separate scene-graph/GPU buffers from the JS
  heap. Goal: know how much of the 759 MB is blur/wallpaper vs JS objects vs
  delegates before touching code.
- Gate: a repeatable number per target, so each later phase can prove its win.

### Phase 1: Release boot-resident panels (offender #2)

- Convert the five `active: Config.ready` LazyLoaders to open-gated.
- Gate: idle `Private_Dirty` drops; no regression in panel open behavior.

### Phase 2: Blur / wallpaper buffers (offender #1)

- Measure blur backbuffer size and count; downscale and/or single-buffer when
  static; release buffers for non-focused outputs.
- Gate: idle `Private_Dirty` drops materially with no visible blur-quality loss
  at normal viewing distance.

### Phase 3: List reuse (offender #3)

- Add `reuseItems` + bounded `cacheBuffer` to the high-row-count views first
  (wallpaper thumbnails, overview, clipboard, emoji, launcher).
- Gate: resident memory after opening those panels drops; scroll stays smooth.

### Phase 4: Singleton / settings audit (offender #4)

- Confirm settings trees are released on close; trim or lazy-split the largest
  always-resident singletons if they prove to hold significant heap.

## Non-goals

- The Rust backend. Its footprint is kilobytes; it is not the lever.
- rust-analyzer / editor / browser memory. Not part of the shell.
- Reducing shared library (`Shared_Clean`) memory — it is shared with every
  other Qt app and counted once in PSS already.

## Decommissioning / safety

This plan touches UI files with wide reach (background, panels, list views).
Every change is measured and individually revertible, validated live on the
running shell, and must not regress visual quality or first-open latency beyond
an agreed threshold. No phase ships without a before/after memory number.
