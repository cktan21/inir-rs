# Verification of the five-hotspot and frontend changes — 2026-10-14

Independent check of the 2026-10-12/13 changes (five hotspot fixes, wallpaper
blur retention, media/LiveLayer activity gates). Same harness and workload as
[the 2026-10-09 baseline](../2026-10-09-native-efficiency/results.json): idle,
workspace switching (two focus commands per second), and Wi-Fi rescans.

## Results

`results.json` here: two ABBA rounds on the changed code.
[`2026-10-14-strength-hysteresis`](../2026-10-14-strength-hysteresis/results.json):
one round after the signal-strength fix below. Baseline: five rounds on
2026-10-09. CPU 100% = one logical core. The runs are on different days, so
background conditions differ. Treat the baseline comparison as indicative.

| Workspace switching | 2026-10-09 | 2026-10-14 | Change |
| --- | ---: | ---: | ---: |
| QML shell CPU | 29.2% | 14.2% | −51% |
| Rust shell CPU | 31.1% | 17.0% (13.2% after fix) | −45% |
| QML main thread | 6.06% | 4.98% | −18% |
| Rust main thread | 6.32% | 5.38% | −15% |
| QML PSS | 668.5 MiB | 600.7 MiB | −68 MiB |
| Rust PSS | 682.4 MiB | 610.3 MiB | −72 MiB |

Idle PSS barely moved (QML 604 → 598 MiB, Rust 620 → 609 MiB). Wi-Fi-phase PSS
rose by about 27–35 MiB, because the retained wallpaper blur is no longer
released. Memory now stays at about 600–625 MiB in every phase. It used to swing
between about 565 and 682 MiB as the blur was torn down and rebuilt. Rust mode
runs one helper instead of two: the Python keyboard daemon is replaced by the
native worker. That helper used about 15 MiB PSS in its own process.

## Issue found and fixed: Rust idle GPU bursts

Both Rust runs showed one-second GPU bursts of 13–24% while idle (QML: ~1%).
The cause: NetworkManager reports ±1–2% signal jitter on the active network every
6–18 s. The native mirror published each change, which redrew every strength
indicator (e.g. the pill's Wi-Fi glyph). The QML backend refreshes strength only
on `nmcli monitor` events, so it never saw this jitter.

The mirror now ignores access-point strength moves under 5 points; slow drift
still lands once it accumulates. Over 60 s on this network, publications fell
from 7 (all strength jitter) to 2 real membership changes. Rust idle after the
fix (one run): CPU 0.52%, GPU mean 0.20%, peak 0.8%, no bursts. Before: CPU
0.99%, GPU 0.94% with bursts. QML still showed one isolated burst in this round,
so occasional idle redraws are not Rust-specific.

## Other notes

- Harness: after stopping a shell, helper cleanup waited a fixed 0.5 s and only
  killed helpers recorded before the stop. A helper respawned during shutdown
  failed the run. Cleanup now polls for 3 s, escalates to SIGKILL, and treats
  helpers started after a stopped shell as that shell's own.
- Num Lock shows "off" until toggled when the first reading is "on". This is the
  existing reliability policy, identical in Python and native modes.
- Sample sizes are small (two runs, then one per mode). Rust and QML remain
  within noise of each other. The large reduction applies to both backends and
  comes mainly from the wallpaper blur retention.
