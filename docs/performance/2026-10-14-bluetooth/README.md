# Bluetooth implementation verification — 2026-10-14

Arch/CachyOS, Qt 6.11.2, Quickshell 0.3.1, BlueZ 5.87,
NetworkManager 1.58.1. Measurements used the running shell on this checkout.
All percentages below are one-core percentages, not whole-machine utilization.

## Short controlled handoff

`scripts/profile-bluetooth.py --duration 5` sampled process/thread CPU counters.
The shell was locked, no iNiR view owned a scan, and LibrePods stayed running.
The native controller was enabled in both samples and used the shared runtime.

| Process | External/Blueman, 5.016 s | Integrated preview, 5.018 s | After rollback, 5.019 s |
| --- | ---: | ---: | ---: |
| Blueman applet | 52.632% | stopped for preview | 60.575% |
| Blueman tray | 4.984% | stopped for preview | 5.779% |
| bluetoothd | 33.493% | 1.993% | 43.040% |
| Quickshell | 2.990% | 3.587% | 2.391% |
| LibrePods | 0.000% | 0.000% | 0.000% |
| Native executor thread | 0.000% | 0.000% | 0.000% |

The shell had 52 threads in both samples; the controller did not add a dedicated
OS thread. Global BlueZ `Discovering` stayed true in both samples, while iNiR's
owned scan/session count was zero. Global discovery alone does not establish
which client causes the work. This short handoff isolates a substantial Blueman-
associated CPU cost, not a temperature reduction or a sustained benchmark.

Integrated preview registered the pairing and OBEX agents successfully and
reported NetworkManager ready. No real pairing, transfer or tethering was
performed. Preview was rolled back: external mode, original launcher and
Blueman applet/tray restored. The Blueman autostart file was not changed;
bluetoothd and LibrePods were never stopped.

## Lifecycle and protocol checks

- The real manager loaded and acquired an acknowledged native scan on hci0.
  Screen lock removed its owners and released the session. The other client's
  global discovery remained active.
- Eight native Bluetooth tests passed on private D-Bus buses, including
  overlapping/expiring scans, close-before-start acknowledgment, typed pairing
  replies/cancellation/lock, OBEX send/receive/progress/cancel/session cleanup,
  receive collision protection and PANU/NAP profile defaults.
- Nineteen QML test results passed (eight discovery cases, seven command-outcome
  cases and suite setup/cleanup).
  The production discovery helper is used unchanged; peers/singletons are mocks.
- Five reversible migration tests passed. Native bridge/model CTest tests passed.
  Rust core regression suite and service-layout/native-contract checks passed.
- All nine affected Bluetooth surfaces compiled with the real Quickshell/Wayland
  backend, including the final changes, without instantiating their windows or
  restarting the locked session.

The full real-device acceptance checklist remains in [Bluetooth documentation](../../BLUETOOTH.md).
Do not disable Blueman autostart until those hardware checks pass.

## Real-device follow-up — 2026-10-15

The final controller and UI fixes were loaded and tested with AirPods. The
QML-to-Rust command-object conversion fix resolved the incorrect “Missing
adapter” error. Connection failures now appear as per-device errors rather than
being hidden by an observed Connected label.

Audio investigation found missing Bluetooth audio endpoints in the existing
WirePlumber session. Restarting WirePlumber restored the endpoints. LibrePods was
temporarily stopped with permission for an isolated connection check and then
restored; its ear-detection behavior also affected when the audio profile became
available. The user subsequently confirmed audio was working. No pairing was
removed, and bluetoothd was not restarted.

This verifies the headphone connection/audio path, not keyboard pairing, real
OBEX transfers or either tethering direction. Blueman autostart remains unchanged.
