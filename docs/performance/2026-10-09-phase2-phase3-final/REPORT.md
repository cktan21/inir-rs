# Phase 2/3 exclusive backend comparison — 2026-10-09

Title churn is substantially cheaper after the typed bridge and cached Niri
projection: mean main-thread CPU falls **81.3%**, from 5.57% QML to 1.04% Rust;
total shell CPU falls **63.6%**, from 6.44% to 2.34%.

Workspace switching does **not** meet Phase 3's 50% main-thread gate:
7.78% QML versus 7.63% Rust, just **1.9% lower**. Total shell CPU is
**8.1% higher** (32.41% versus 35.04%); shell GPU engine activity is
**8.5% higher**. This is not evidence of a universally faster native backend.
Keep the new bridge opt-in; do not promote this to a completed performance gate.

## Measurements

Means of two 30-second captures per owner. CPU 100% means one fully occupied
logical core. Main-thread CPU is reported separately, not subtracted from total.
RAM is shell-only proportional resident memory, not shell-plus-helper RAM.

| Scenario / owner | Shell CPU | Main-thread CPU | Shell PSS | Shell GPU busiest engine |
| --- | ---: | ---: | ---: | ---: |
| Idle / QML | 0.93% | 0.38% | 610.95 MiB | 0.76% |
| Idle / Rust | 0.88% | 0.38% | 613.96 MiB | 0.69% |
| Workspace switching / QML | 32.41% | 7.78% | 679.61 MiB | 27.61% |
| Workspace switching / Rust | 35.04% | 7.63% | 670.43 MiB | 29.95% |
| Wi-Fi rescan / QML | 1.23% | 0.52% | 568.18 MiB | 0.35% |
| Wi-Fi rescan / Rust | 1.27% | 0.51% | 568.28 MiB | 0.36% |
| Title churn / QML | 6.44% | 5.57% | 613.62 MiB | 0.63% |
| Title churn / Rust | 2.34% | 1.04% | 613.38 MiB | 0.77% |

| Run order | Workspace shell CPU | Workspace main CPU | Title shell CPU | Title main CPU |
| --- | ---: | ---: | ---: | ---: |
| 1-rust | 35.67% | 7.72% | 2.59% | 1.11% |
| 1-qml | 33.08% | 8.01% | 6.30% | 5.54% |
| 2-qml | 31.75% | 7.55% | 6.59% | 5.59% |
| 2-rust | 34.42% | 7.55% | 2.10% | 0.97% |

Native idle PSS is 3.02 MiB higher;
workspace PSS is 9.18 MiB lower.
Title PSS differs by only 0.24 MiB.
There is no broad RAM/GPU saving established by these two repeats.
Native has about five more shell threads, but one fewer live acquisition helper
(no `nmcli monitor`). Helper memory was not sampled in this series.

## Isolation and workloads

- Exactly one shell at a time, Rust → QML → QML → Rust. Same original command,
  environment, output, workspace, layout and presentation settings.
- Five real windows, including one static-content title fixture; one output
  (`eDP-1`), two workspaces; layout `ii`; 20 logical CPUs.
- 15-second warm-up plus focus/readiness settling before every capture.
- Idle: no injected actions. Workspace: 60 focus commands, two/second.
  Wi-Fi: six external `nmcli device wifi rescan` calls, one/five seconds,
  identical in both modes. NetworkManager exit codes are retained in the raw data.
- Title churn: the same external Qt window in all four runs, 20 title updates/s.
  The shell's public record was queried before/after to verify delivery.
  Fixture CPU/GPU is excluded from per-shell counters equally in both modes.
- Native: `INIR_RUST_BACKEND=1 INIR_NATIVE_BRIDGE=1`,
  `INIR_NATIVE_DOMAINS=network,brightness,niri`. Models ready, Niri fallback
  Loader absent, QML network subscriber/acquisition stopped, backlight reads native.
- QML: both flags zero, native plugin absent from process mappings,
  native apply count zero, QML Niri event socket and network subscriber active.
- Audio, battery, Bluetooth, media and power profiles remain common Quickshell
  services. The unused Rust power worker is not started.
- Hot reload disabled and native per-event logging suppressed. Lock state and
  ownership checked at scenario boundaries; display power checked every sample.
  Both modes had Game Mode off. Ownership snapshots include model apply counters.
- procfs ticks for CPU; smaps_rollup for RSS/PSS/private/swap; DRM fdinfo for
  per-client engine time and VRAM/GTT; thread context-switch counts.
  GPU means this shell's client, not the compositor or whole GPU; resident buffer
  counters can include shared buffers and must not be added to PSS.
- Original service, workspace and focused window were restored, and the fixture
  closed. The service was confirmed active after the sequence.

Only two repeats per mode; this is a live desktop, not a statistically controlled
latency/frame-time study. Idle noise, background apps, network timing and rendering
can influence results. Workspace CPU includes common UI work; attributing the
remaining cost to a specific frontend component would require profiling.
No scheduler-wakeup, frame-time or input-latency claim is made.

## Bridge and implementation validation

The old JSON/C++ record adapter and global desktop publication are removed.
Rust-owned typed models apply off-thread indexed deltas and exact role masks.
Adjacent equal-mask edits use one Qt range notification. Niri's persistent
projection reconstructs only affected IDs, retaining unchanged row identities;
title-only changes do not sort or publish whole JS collections.

The 100-row Qt fixture keeps QML role views attached and verifies model edits,
title-role-only notifications, collection/row identity, moves and zero
steady-state resets. Observed single-row apply means were **11.7–12.4 µs**;
all-100-title apply means were **90–111 µs**, versus the earlier 1.27 ms.
The normal narrow-event <50 µs gate passes; the stricter interpretation requiring
all 100 changed rows below 50 µs does **not**. The fixture measures bridge apply,
not complete rendered-frame latency.

Niri projection, layout ordering, MRU/null geometry, persistent action socket,
failed-action non-replay, stream/action isolation and independent lease tests pass.
CTest `native_models`/`native_bridge` and the six QML/Rust contract checks pass.
Model fixtures acquire no compositor lease and are rejected outside test mode.

The native startup logged one temporary null keyboard-array binding warning.
Empty metadata is now initialized with array/map shapes and covered by the Qt test.
Overview's imperative title filter also now listens to narrow title/app role changes.
These post-capture fixes affect pre-connect initialization and an open filtered
overview, not the measured warmed-up, closed-overview workloads.

The earlier interrupted series in `../2026-10-09-phase2-phase3/` is not comparison
evidence: it was restarted after correcting stale copied taskbar titles.
The older Phase 0 report used a different window count and bridge; do not treat its
raw CPU numbers as a directly matched pre/post implementation experiment.

## Reproduction

```sh
cmake --build rust/build -j 4
python3 scripts/compare-backends.py --duration 30 --warmup 15 --rounds 2 \
  --title-churn --output /tmp/inir-exclusive-next
```

Requires an unlocked session and exactly one shell managed by `inir.service`.
The harness restores that service in its cleanup path. Subsequent cleanup is
scoped to observed shell-child helper identities; unowned orphan helpers cause
refusal, not a broad process kill.

[Raw samples and ownership snapshots](results.json), [machine-readable means](summary.json).
