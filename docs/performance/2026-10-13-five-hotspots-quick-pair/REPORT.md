# Five hotspot fixes: short live check

The five scoped fixes are implemented and the functional checks pass. Native
keyboard ownership removed the Python daemon during the native run. In this
short pair, idle shell-plus-helper PSS was 614.7 MiB native versus 626.0 MiB
QML, about 11.3 MiB lower. Workspace CPU was higher for native. This is one
short pair, not a statistically established performance verdict or a passed
Phase 3 rollout gate. Rust remains opt-in.

## Captured results

Captured 2026-10-13. Rust then QML, one fresh start each; 10-second warm-up and
10 seconds each of idle, workspace switching and Wi-Fi rescans; three real
windows and one output. CPU 100% means one logical core; memory is MiB.

| Metric | Native | QML |
| --- | ---: | ---: |
| Idle shell CPU | 4.29% | 4.49% |
| Idle main-thread CPU | 1.80% | 1.80% |
| Idle shell PSS | 613.6 | 606.2 |
| Idle helper PSS | 1.1 | 19.8 |
| Idle shell + helper PSS | 614.7 | 626.0 |
| Workspace shell CPU | 25.09% | 19.17% |
| Workspace main-thread CPU | 7.80% | 6.89% |
| Workspace shell + helper PSS | 613.5 | 630.5 |
| Wi-Fi rescan shell CPU | 4.98% | 3.98% |

Both modes passed in-process ownership checks. The native run had a ready
keyboard worker with one device, no Python probe/monitor, no Niri fallback and
no QML network acquisition. QML had no native plugin and used its own Niri,
network and Python keyboard paths. All four requested scans returned success.
The original QML service and workspace were restored. No VPN or hotspot
activation, secret modification, or physical keyboard toggle was performed.

## What this verifies

- 34 Rust tests, two CTest targets, six contract checks, JS matching tests and
  five Qt behavior tests passed. Isolated actual-QML VPN/hotspot probes covered
  native/fallback readiness, WireGuard, removals and visibility leases.
- The 100-row model test proves title publications do not increment workspace
  apply calls. Single-row apply was 11.94 µs, bulk title update 109.04 µs.
- Native mode removes the persistent Python keyboard helper, and both keyboard
  acquisition paths remain demand-gated. This pair measures helper memory too;
  shell-only PSS would miss that saving.

## Limits

Ten seconds of warm-up is a quick acceptance check, not a settled idle-energy
measurement. Startup jobs, network conditions and other desktop work remain
variable. No p-values or broad speedup claim are made from one pair. Taskbar and
overview QML fixes are common to both modes, so this pair cannot isolate their
before/after benefit; the overview was closed. No frame-time, input-latency or
temperature inference is made.

The [earlier title-workload attempt](../2026-10-13-five-hotspots-quick/REPORT.md)
was rejected after the compositor changed focus outside the expected workload;
its cause is unestablished, and it contributes no performance numbers here.
The completed pair omitted title churn and took under two minutes; both live
attempts together stayed under five minutes. Functional title-only behavior is
covered by the compiled model test.

[Raw metrics and ownership snapshots](results.json), [native shell log](1-rust.log),
[QML shell log](1-qml.log), [implementation and tradeoffs](../../plans/FIVE_HOTSPOT_EFFICIENCY_FIXES.md).

Reproduction:

```sh
python3 scripts/compare-backends.py --duration 10 --warmup 10 --rounds 1 \
  --native-domains network,brightness,niri,keyboard \
  --output /tmp/inir-five-hotspots-quick
```
