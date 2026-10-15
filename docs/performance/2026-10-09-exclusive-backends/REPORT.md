# Exclusive Rust / QML resource comparison — 2026-10-09

Rust did not show a consistent resource advantage in this test. In the two
display-on workspace runs per mode, its mean shell CPU was **25.05%
versus 22.26% for QML**. Rust was cheaper in the first pair and
more expensive in the second, so this is not a statistically established
faster/slower or responsiveness verdict. Main-thread CPU was higher on average,
and there was no clear RAM or GPU saving.

## Activity results

These are means of two 30-second captures per mode. Each capture issued 60
workspace-focus commands at two commands per second. CPU 100% means one full
logical core, not the entire 20-thread CPU. RAM is MiB (1024² bytes).

| Metric during workspace switching | QML | Rust | Rust vs QML |
| --- | ---: | ---: | ---: |
| Shell CPU | 22.26% | 25.05% | 12.6% |
| Main-thread CPU | 6.15% | 7.53% | 22.4% |
| Shell + helper CPU | 22.69% | 25.62% | 12.9% |
| RAM, proportional resident (PSS) | 615.97 MiB | 629.28 MiB | 2.2% |
| RAM, resident (RSS) | 702.06 MiB | 714.31 MiB | 1.7% |
| Shell GPU busiest engine | 20.74% | 21.67% | 4.5% |
| Context switches per second | 4707.18 | 4341.71 | -7.8% |
| Observed threads | 77.10 | 82.00 | 6.4% |

Per-run CPU/GPU values show the variation hidden by the mean:

| Run, in execution order | Shell CPU | Main-thread CPU | GPU engine busy | Focus commands |
| --- | ---: | ---: | ---: | ---: |
| 1-rust | 21.17% | 5.76% | 19.07% | 60 |
| 1-qml | 22.89% | 6.27% | 19.89% | 60 |
| 2-qml | 21.62% | 6.03% | 21.58% | 60 |
| 2-rust | 28.94% | 9.31% | 24.26% | 60 |

The activity PSS run means ranged from 615.6–616.4
MiB for QML and 610.0–648.6 MiB for Rust.
Two repeats per mode are insufficient to attribute these differences solely
to backend implementation. This remains a live desktop with background
application, network and UI activity.

## Matched idle pair

The first Rust idle capture started on workspace 2, whereas the other idle
captures started on workspace 1. Its idle numbers are retained in the raw data
but excluded from this comparison. The second pair has matching workspace,
display-on and game-mode state:

| Idle metric | QML | Rust |
| --- | ---: | ---: |
| Shell CPU | 0.79% | 1.31% |
| Main-thread CPU | 0.31% | 0.48% |
| RAM, PSS | 609.3 MiB | 630.4 MiB |
| RAM, RSS | 697.6 MiB | 716.3 MiB |
| Shell GPU busiest engine | 0.91% | 0.27% |
| Live helper processes | 3 | 2 |

In this matched pair, Rust used 21.2
MiB more PSS and 0.52 percentage
points more of one core. One matched idle pair is not a general memory or
idle-energy benchmark.

## Isolation and setup

- Exactly one Quickshell shell process ran at a time. The managed shell was
  stopped before each temporary test sequence and restored afterward.
- Rust mode: native plugin loaded; native Niri ready; fallback Loader absent;
  QML network monitor and acquisition processes stopped; backlight reads native.
- QML mode: native plugin absent from process mappings; no desktop Rust consumer;
  native Niri revision zero; QML event/action sockets connected and nmcli
  monitor running.
- Rust workers selected with `INIR_NATIVE_DOMAINS=network,brightness,niri`.
  The unused native power-profile worker was excluded because the UI still uses
  Quickshell's power-profile service. Default native mode still includes that
  worker unless this selection is set.
- Audio, battery, Bluetooth, media, power profiles, frontend resource display and
  the QML UI remain common services. Rust's optional resource sampler was inactive.
  This is exclusive ownership of the migrated domains, not a Rust-rendered UI.
- Four fresh starts in Rust → QML → QML → Rust order; 30-second warm-up, then
  30 seconds each of idle and workspace activity; two windows, two workspaces,
  one eDP-1 output; ii layout, Ryzen AI 9 365 / Radeon 880M, 20 logical CPUs,
  kernel 7.2.9-1-cachyos.
- Idle blanking was inhibited only inside each test shell, without persisting
  user policy. Connected DRM displays were checked for power-on state at every
  metric sample. Game mode was false at capture checkpoints.
- Both modes inherited the same original shell environment. Native diagnostic
  tracing was suppressed with `INIR_LOG=error` to avoid per-event logging cost.
- Measurements use procfs CPU ticks, smaps_rollup RSS/PSS/private/swap data,
  thread context switches, and per-client DRM fdinfo engine time and resident
  buffer counters. Duplicated file descriptors for the same DRM client are
  deduplicated.
- Workspace commands were sent by the benchmark directly to Niri. The test
  compares acquisition, state publication and UI updates; it does not isolate
  native versus QML action-dispatch cost.

GPU percentages are this shell's DRM client engine activity, not whole-device
utilization, and exclude the compositor's separate GPU client. Driver-reported
resident VRAM/GTT values can include shared buffers and should not be added to
PSS or treated as dedicated physical VRAM on this integrated GPU. No scheduler
wakeups, frame times, input latency or exact short-lived process-spawn counts
were measured.

## Corrections and artifacts

The first real-shell restart exposed an unregistered `NiriFallback` type.
Its entry in `services/qmldir` was added before accepted runs. Earlier isolated
probes had not exercised that root-module registration.

The earlier 25-second series lacked display-power checks. A subsequent
confirmation failed because the display was off, so those earlier measurements
are exploratory and are not the basis for the figures above. The final series
explicitly woke the display and rejected powered-off samples. Inspection also
found the first idle workspace mismatch described above; the reusable script
now restores and verifies the initial workspace after warm-up.

- [Final per-sample results and ownership snapshots](display-on/results.json)
- [Machine-readable summary](summary.json)
- [Exploratory series, not used for headline results](results.json)
- [Reproduction script](../../../scripts/compare-backends.py)

The script restores the original service and workspace in a finally block.
For another comparison in this live checkout:

```sh
python3 scripts/compare-backends.py --duration 30 --warmup 30 --rounds 2 \
  --output /tmp/inir-exclusive-comparison
```

This does not establish the Phase 3 ≥50% main-thread improvement gate. The
workspace main-thread result did not approach that target; title churn and
frame/input latency remain unmeasured. The Phase 2 snapshot transport and
compatibility array publication still exist.
