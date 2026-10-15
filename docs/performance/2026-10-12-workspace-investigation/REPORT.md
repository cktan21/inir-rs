# Workspace performance investigation — 2026-10-12

Implementation follow-up: [wallpaper resource reuse](../2026-10-12-wallpaper-reuse/REPORT.md)
has now been implemented and tested. The measurements below remain the original
pre-optimization investigation, not results from the new implementation.

The measurements do **not** establish that Rust itself is more computationally
expensive than the QML backend. The earlier 8.1% regression measured the entire
shell, including its Qt rendering, graphics-driver threads and common services.
In this investigation the CPU difference changed sign across workloads. The
largest isolated GPU cost was the shared fullscreen wallpaper blur, which also
repeatedly loads its uncached image and recreates the blur component.

This identifies expensive frontend paths and explains why changing the data
backend has not delivered a large workspace improvement. It does **not** prove
which component caused the exact 2.63 percentage-point difference in the
October 9 capture: that capture had no thread or QML trace, and the desktop
conditions are not identical.

## Backend comparisons

CPU 100% means one occupied logical core. GPU is the shell's busiest DRM client
engine, excluding Niri and other applications. Values below are duration-weighted
means, with each fresh shell launch treated as a repeat. Different series are
not pooled.

| Series / owner | Launches | Windows | Shell CPU | Main CPU | Shell GPU |
| --- | ---: | ---: | ---: | ---: | ---: |
| General ablations / QML | 3 | 4 | 36.03% | 11.25% | 23.73% |
| General ablations / Rust | 3 | 4 | 36.70% | 11.13% | 25.30% |
| Wallpaper control, static title fixture / QML | 2 | 5 | 39.44% | 13.76% | 29.49% |
| Wallpaper control, static title fixture / Rust | 2 | 5 | 35.78% | 11.24% | 26.01% |
| Both workspaces populated / QML | 1 | 5 | 17.99% | 7.99% | 2.91% |
| Both workspaces populated / Rust | 1 | 5 | 17.30% | 6.70% | 4.63% |

The first series shows Rust **1.9% higher** in total CPU and **1.0% lower** in
main-thread CPU. Paired total-CPU differences, Rust minus QML, were
**−0.25, +1.95 and +0.30 percentage points**. The five-window series instead
shows Rust **9.3% lower** in total CPU and **18.3% lower** on the main thread.
These differences do not support a universal faster/slower verdict.

The populated-workspace control keeps a static fixture on the alternate
workspace. Compositor observations verify a non-null active window throughout
every sample. However, its snapshots show **media inactive in both modes**, while
the media-control and trace series show media playing. Its lower CPU/GPU cannot
be attributed solely to populated workspaces: playback state and window
distribution also changed. It remains a valid short Rust/QML comparison within
that series, not an isolated estimate of occupancy savings.

## Where CPU time goes

In the repeated four-window baseline:

| CPU attribution | QML | Rust |
| --- | ---: | ---: |
| Main thread | 11.25% | 11.13% |
| Qt scene-graph render threads | 11.63% | 11.90% |
| Graphics workers named `qs:*` | 4.75% | 4.98% |
| Rust executor, `inir-core` | absent | 0.07% |
| Rust blocking workers | absent | below tick resolution |

The native Niri/workspace apply counters account for about **0.54% of wall
time** during the baseline. Those callbacks include synchronous QML signal and
binding work; they are not isolated Rust CPU. Their live mean times were about
714 µs for Niri and 1,123 µs for workspaces, much higher than a model-only fixture
because real consumers are attached.

The small positive difference in this series is largely outside the main thread;
render and graphics workers explain about 0.50 of the 0.67 percentage-point total
difference. This is thread attribution, not proof of a particular rendering
function or of the historical regression. Thread totals can miss newly observed
or short-lived threads. At 100 Hz over 20 seconds, one tick is 0.05 percentage
points; a recorded zero does not mean no work.

Qt's threaded render loop performs scene-graph work on separate threads, which
remain active with either data backend. [Qt scene-graph documentation](https://doc.qt.io/qt-6/qtquick-visualcanvas-scenegraph.html).

## Controlled frontend changes

These controls affect only the benchmark process; they do not save user settings
or pause actual media playback.

| Presentation, three launches per owner | QML CPU | Rust CPU | QML GPU | Rust GPU |
| --- | ---: | ---: | ---: | ---: |
| Normal | 36.03% | 36.70% | 23.73% | 25.30% |
| Global animations disabled | 24.32% | 25.72% | 5.65% | 5.25% |
| Effects disabled | 28.50% | 26.18% | 5.84% | 5.57% |
| Only workspace-indicator animation stopped | 36.42% | 35.00% | 23.71% | 25.36% |
| Bar unloaded | 25.64% | 25.68% | 22.03% | 24.11% |

Disabling animations cuts CPU by roughly **30–33%**. Disabling effects cuts
shell GPU activity by **75–78%**. Removing the bar cuts CPU substantially, but
leaves the expensive fullscreen GPU work. Stopping just the workspace indicator
does not produce a consistent improvement; it is not the dominant GPU cost.
Its fixed 100/300 ms animations do bypass the global animation switch, a
separate frontend issue.

The more specific wallpaper-blur control, with two launches per owner, gives:

| Wallpaper blur | QML CPU | Rust CPU | QML GPU | Rust GPU |
| --- | ---: | ---: | ---: | ---: |
| Enabled | 39.44% | 35.78% | 29.49% | 26.01% |
| Disabled | 25.55% | 26.36% | 6.21% | 5.55% |

That isolates **26–35% less shell CPU** and **about 79% less shell GPU activity**
when this path is disabled.

A separate one-launch-per-owner media control gives:

| Presentation | QML CPU | Rust CPU | QML GPU | Rust GPU |
| --- | ---: | ---: | ---: | ---: |
| Normal | 35.40% | 32.33% | 25.18% | 25.10% |
| Desktop media widget unloaded | 27.60% | 27.70% | 21.03% | 21.68% |
| Wallpaper blur and media widget disabled | 19.40% | 22.80% | 3.00% | 3.01% |

The media widget is another measurable frontend cost. These short controls are
directional evidence, not a population estimate. The remaining CPU is not
automatically Rust cost: the bar, dock, other widgets, common services and Qt
rendering remain active.

## QML trace evidence and source path

The diagnostic traces are separate five-second captures with profiling enabled;
their resource figures are excluded from the headline comparisons.

Both backends perform **five wallpaper loading-start events and five
`GaussianBlur` creations for ten workspace commands**. Disabling only wallpaper
blur produces **zero wallpaper loading-start events and zero blur creations**.
The source and trace agree:

1. [Background.qml](../../../modules/background/Background.qml) derives window
   presence from the active workspace and animates `focusPresenceProgress`.
2. Its `blurAlwaysLoader` activates when blur progress is positive and destroys
   the blur component when the fade reaches zero.
3. With the external static wallpaper owner active, `needsStaticTexture` follows
   this blur requirement. The QML wallpaper source is cleared when no longer
   needed and set again when blur resumes.
4. [WallpaperCrossfader.qml](../../../modules/common/widgets/WallpaperCrossfader.qml)
   clears both image slots on an empty source, and its images use `cache: false`.
   The next activation therefore requests the fullscreen image again.

The traces also record Cava updates during workspace switching, media progress
updates, and thousands of `LiveLayer.onAfterAnimating` calls. Later ownership
snapshots confirm **media playing**, the desktop media widget enabled, and the
overview/media panels closed. “No injected input” therefore includes ongoing
frontend activity; it is not a quiet media-free baseline. Trace durations are
inclusive instrumented wall times and must not be summed as independent CPU time.

A final ten-second, no-input observation of the restored QML shell, with media
inactive at the state checks, recorded **4.10% CPU**. This is additional context,
not a paired playback or backend experiment. [Restored-shell samples](restored-media-inactive.json).

This is why replacing JSON acquisition alone cannot eliminate the measured
costs. Both paths still drive wallpaper fades, image/texture lifetimes, audio
visualization, bar indicators and dock updates.

## Conclusions and next targets

Keep the native bridge opt-in. This investigation does not establish the 50%
workspace main-thread improvement gate or any frame-time/input-latency gain.

The first optimization target should be the wallpaper blur lifecycle: retain a
bounded prepared texture/effect across workspace occupancy changes and animate
its opacity, then measure the CPU/GPU benefit against retained memory. Evaluate
a smaller blur texture or compositor-supported equivalent after visual parity
checks. This is a recommendation; no such production optimization was applied.

Next, profile media motion and `LiveLayer` eligibility work when the desktop is
covered, and ensure the global reduced-animation setting gates the workspace
indicator. Repeat with an explicitly controlled playback state. A different
backend language or a full UI rewrite is not justified by these measurements.

The exact October 9 regression remains unassigned. Its extra CPU was outside the
main thread, and current GPU/CPU differences depend on the visual workload. The
evidence supports shared frontend/rendering costs and workload sensitivity,
not an intrinsic Rust-versus-QML computational penalty.

## Validation, limitations and artifacts

- **16 accepted fresh shell launches; 1,780 injected workspace commands**,
  including 60 in the separate instrumented traces. Fourteen launches were
  unprofiled. Native/QML ownership, action counts and artifact presence verified.
- One shell at a time; native domains `network,brightness,niri`; QML fallback
  absent in Rust mode and native plugin absent in QML mode. Display power and
  lock state checked, Game Mode off, hot reload disabled, native logs suppressed.
- Workspace commands use the benchmark's integer-ID socket directly to Niri.
  The experiment measures acquisition, publication and UI consequences, not
  native-versus-QML action dispatch.
- Layout ii/material; eDP-1, 2880×1800 at 120 Hz, scale 1.5; Ryzen AI 9 365,
  20 logical CPUs; kernel 7.2.9-1-cachyos; Qt 6.11.2. Release native build.
- Warm-up 15 seconds. Main series: 20-second scenarios; media/populated controls:
  10 seconds; QML trace scenarios: 5 seconds. Alternate backends and counterbalance
  presentation order; duration-weight CPU/GPU samples. Capture the final interval.
- Original checkout was already dirty and is not a bit-identical reconstruction
  of October 9. Live apps, playback and background events are not frozen; small
  differences and single-pair controls cannot establish statistical significance.
- The title-fixture series ends with reported workspace 2 in Rust and 1 in QML.
  No per-sample compositor witness existed in that series. Its idle/title means
  are retained but **excluded from matched idle/title claims**. Later captures
  add compositor observations. This is a measurement-integrity issue to resolve
  before claiming a new title-churn performance gate.
- An incidental native action in the interrupted populated-workspace attempt
  logged a floating-point ID rejected by the unsigned Niri ID parser. Measured
  workspace commands use the external integer-ID socket path. This parity issue
  was not fixed or identified as the performance cause.
- `perf` was unavailable. There are no CPU-stack, energy, scheduler-wakeup,
  frame-time or input-latency claims. DRM engine time is not whole-GPU utilization.
- Native build and two CTest fixtures passed; six Rust/QML contract checks and
  five sampler tests passed. Verification exposed and corrected a sampler tail
  omission, with a deterministic process-scan-overrun regression test.
- The first tracing connection produced no data; startup attachment succeeded.
  Batched console commands dropped the recording toggle, so commands are now
  sent individually. The incomplete attempts are excluded. A mode switch also
  interrupted the first populated-workspace attempt before cleanup; the owned
  temporary shell was stopped, normal service restored, and that comparison rerun.
  The harness now handles SIGTERM through its cleanup path; SIGKILL remains
  uncatchable.
- The managed shell was confirmed active after the final test. Fixtures and
  profiler processes were closed. Acquisition-helper cleanup is limited to
  observed children. Benchmark presentation controls require `INIR_BENCHMARK=1`
  and leave normal presentation defaults and persistent user policy intact.

Accepted results and duration-weighted summaries:

- [General ablations](ablation/results.json), [summary](ablation/summary.json).
- [Specific wallpaper blur](wallpaper-blur/results.json), [summary](wallpaper-blur/summary.json).
- [Media widget control](media-control/results.json), [summary](media-control/summary.json).
- [Populated-workspace control, completed rerun](both-populated-resumed/results.json),
  [summary](both-populated-resumed/summary.json).
- [Separate profiled captures](qml-traces-accepted/results.json),
  [trace event summary](qml-traces-accepted/trace-summary.json).

Raw Qt XML traces are gzip-compressed losslessly; decompress a `.qtd.gz` file
before opening it in Qt Creator. The trace-summary helper reads either form.
Directories `qml-traces`, `qml-traces-startup` and `both-populated` contain
unsuccessful or interrupted attempts, not accepted comparison evidence.

Reproduction from this checkout:

```sh
cmake --build rust/build -j 4
python3 scripts/compare-backends.py --duration 20 --warmup 15 --rounds 3 \
  --workspace-only \
  --workspace-variants baseline,no-animations,no-workspace-animation,no-effects,bar-hidden \
  --output /tmp/inir-workspace-ablation-next
python3 scripts/compare-backends.py --duration 20 --warmup 15 --rounds 2 \
  --title-churn --workspace-variants baseline,no-wallpaper-blur \
  --output /tmp/inir-wallpaper-control-next
python3 scripts/compare-backends.py --duration 10 --warmup 15 --rounds 1 \
  --workspace-only \
  --workspace-variants baseline,no-wallpaper-blur,no-media-widget,no-blur-or-media-widget \
  --output /tmp/inir-media-control-next
python3 scripts/compare-backends.py --duration 10 --warmup 15 --rounds 1 \
  --workspace-only --fixture-only --fixture-workspace alternate \
  --output /tmp/inir-both-populated-next
python3 scripts/compare-backends.py --duration 5 --warmup 15 --rounds 1 \
  --qml-profile --workspace-variants baseline,no-wallpaper-blur,bar-hidden \
  --output /tmp/inir-qml-trace-next
python3 scripts/analyze-workspace-profile.py /tmp/inir-workspace-ablation-next/results.json
```

Use fresh output directories. The harness temporarily restarts the shell and
changes workspace focus; it restores the managed service on normal completion,
exceptions, SIGINT and SIGTERM. Avoid interrupting it with SIGKILL.
