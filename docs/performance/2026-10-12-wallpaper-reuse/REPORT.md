# Wallpaper resource reuse — 2026-10-12

## Implementation

`Background.qml` now decides static texture/blur retention from feature and
wallpaper eligibility, not workspace occupancy. Switching to an empty workspace
fades the existing blur to zero without clearing the crossfader's image source
or destroying the blur. Each output retains its own current resources.

The small `RetainedWallpaperBlur` component preserves the existing Gaussian
blur radius and 33-sample cap. It caches the output for a settled static source,
and bypasses caching during wallpaper transitions, parallax/pan and awww reveal
animations. Changes to source pixels, dimensions and effect settings refresh
Qt's cache. [Qt GaussianBlur caching documentation](https://doc.qt.io/qt-6/qml-qt5compat-graphicaleffects-gaussianblur.html#cached-prop).

Low-power mode, disabled effects/blur, zero radius, non-static wallpapers,
backdrop replacement, safety blocking and an inactive panel family release the
retained blur. A wallpaper still needed by the internal renderer or lock blur
remains available to that consumer. Lock blur hides the retained desktop blur
without destroying it. Actual wallpaper changes retain the crossfader's existing
bounded two-slot transition behavior; old images are still released and
`Image.cache` remains false. No global wallpaper-history cache was introduced.

## Measurement method

The before and after series each use four fresh shell launches in Rust/QML/QML/
Rust order, two per backend, with a 15-second warmup and 20-second workspace
captures. The workload alternates between populated and empty workspaces at two
focus commands per second. The after series also measures a transient
`no-wallpaper-blur` control; its order is reversed on alternating launches.

The native plugin, unmigrated services, wallpaper, animations and user settings
are unchanged between phases. Backend ownership is checked before/after each
capture, and Niri focus is observed during sampling. The controller restores the
managed shell, original workspace and window focus. Raw captures are in
`before/results.json` and `after/results.json`; weighted summaries are alongside
them. CPU 100% means one logical core. GPU is the busiest per-shell DRM engine,
not whole-device use. PSS is process proportional memory, not total GPU memory.

This is a short before/after experiment, not a randomized or statistically
robust population estimate. The phases are sequential, so changing background
activity can affect the numerical difference. Do not compare these percentages
directly to the earlier media-playing investigation.

## Repeated live results

All eight unprofiled launches had five desktop windows and reported media
inactive before and after each workspace capture. Each table entry averages two
fresh launches; samples within a launch are duration-weighted.

| Backend / phase | Shell CPU | Shell GPU | Process PSS |
| --- | ---: | ---: | ---: |
| QML / before | 29.67% | 30.23% | 573.4 MiB |
| QML / retained | 23.96% | 19.27% | 615.5 MiB |
| Rust / before | 31.57% | 34.20% | 579.1 MiB |
| Rust / retained | 24.57% | 20.59% | 608.1 MiB |

Observed total CPU fell **19.2% with QML** and **22.2% with Rust**. Every retained
baseline launch used less total CPU than every pre-change baseline launch. Main
thread CPU did not improve consistently: QML changed 8.27% → 8.67%, Rust
8.47% → 8.07%. This is primarily a whole-shell/rendering optimization, not a
demonstrated main-thread or isolated Rust improvement.

Mean shell GPU activity fell **36.2% / 39.8%**, but post-change run ranges were
wide: QML 12.03–26.52%, Rust 12.89–28.29%. Do not treat those mean reductions as
a stable guaranteed GPU saving or a frame-rate measurement. The after-phase
blur-disabled control still used approximately 8% shell GPU; retention/caching
does not eliminate all fullscreen blur/rendering work.

The memory tradeoff is explicit: process PSS increased approximately **42.1 MiB
with QML** and **29.1 MiB with Rust**. Driver-reported resident VRAM/GTT varied
considerably between launches and is not added to PSS. Retaining resources is
bounded, but it uses more memory than releasing them on empty workspaces.

## Functional checks

`python3 scripts/test-wallpaper-reuse.py` passes **13 functional cases** (15
including test setup/teardown). These exercise actual QML objects and rendered
pixels, not a reimplementation of the component's behavior:

- The same effect survives repeated zero/partial/full focus fades and hiding.
- Disabling retention, removing its source or setting zero radius releases it.
- Wallpaper/source replacement and changes while hidden refresh cached pixels.
- Source animation bypasses caching without replacing the effect.
- Radius and output size update in place; rendered blur pixels update with radius.
- Cached and uncached blur pixels match, including the opacity-zero layer-texture
  source used by the external awww renderer.

The new component also passes `qmllint`; repository QML pitfalls and Qt
compatibility checks pass. These tests cover the retained blur and its pixel
behavior, not every interactive lock/video/multi-monitor transition.

## Resource-churn verification

A separate instrumented launch per backend recorded ten focus commands over
five seconds. Both accepted baseline traces contain **zero wallpaper pixmap
load starts and zero GaussianBlur creation spans during switching**. Recording
is active: each trace contains more than 67,000 event spans, and startup traces
record the wallpaper load. The earlier investigation showed repeated image
loads and blur creation in this same focus path.

The losslessly compressed traces and profiler logs are in `traces/`, with an
event summary in `traces/trace-summary.json`. Their instrumented CPU/GPU values
are not included in the unprofiled before/after performance table. Together with
the object-identity and rendered-pixel tests, they confirm that this change
removes the measured resource churn without removing the wallpaper blur.

## Reproduction

```sh
python3 scripts/test-wallpaper-reuse.py
python3 scripts/compare-backends.py --duration 20 --warmup 15 --rounds 2 \
  --workspace-only --workspace-variants baseline,no-wallpaper-blur \
  --output /tmp/inir-wallpaper-reuse-new-capture
```

The live benchmark temporarily restarts the desktop shell and switches
workspaces. The functional tests run headlessly using OpenGL/RHI with Mesa
software rendering: a pure software Qt scene graph cannot render GaussianBlur.
