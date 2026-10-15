# Five hotspot efficiency fixes

Implemented 2026-10-13 as the next scoped step in
[the backend revamp](NATIVE_BACKEND_EFFICIENCY_REVAMP.md). The chosen approach
combines local QML fixes with a native keyboard worker and an extension of the
existing NetworkManager mirror. Rust remains opt-in.

| Area | Choice implemented | Benefit | Tradeoff / remaining limit |
| --- | --- | --- | --- |
| Native publications | Shared-collection identity checks, same-key-order row diff, skip empty Qt applies | Title-only events do not scan or apply workspace rows; a pending change reverted before Qt runs is dropped | A real structural reorder still uses the general vector diff. Status/metadata changes must be preserved. |
| Taskbar matching | Cache the one-to-one Niri-ID/Wayland-handle association, index candidates by app ID, keep reactive QML wrappers | Focus/layout changes keep associations and record identity. Workspace filtering uses the same global join | Wayland does not supply the Niri ID, so ambiguous same-app matching remains heuristic and can be quadratic when invalidated. Unmatched handles remain excluded to prevent ghosts. |
| Overview | One bounded 16 ms rebuild bucket and QML-backed item records keyed by window ID | Structural signals and deferred facade publication share a rebuild; surviving rows retain reactive bindings | Adds at most one bucket of delay. Workspace counts are still recomputed while the overview is visible; preview requests retain their existing caching. |
| Keyboard lock state | Leased Rust evdev worker, udev hotplug, eventfd cancellation; demand-gated Python/sysfs fallbacks | Native mode removes the persistent Python interpreter and periodic device enumeration. No hotplug timer when udev works | Input permissions still apply; no-readable-device errors fall back. Physical unplug/replug and every external keyboard were not exercised. Num Lock reliability policy remains in QML. |
| VPN / hotspot state | Extend the shared NetworkManager mirror; native activated-only summaries; actual visible-control leases | VPN/WireGuard counts/names/flags follow native events; both hotspot variants stop native-mode status polling; unrelated network changes stop spawning Tailscale queries | VPN/hotspot actions, import and detailed profile reads still use CLI. Tailscale retains its own visibility-gated CLI status refresh; hidden external Tailscale changes catch up on open. QML fallback keeps NetworkManager summary refreshes for hidden indicators. |

Alternatives deliberately deferred:

- Separate Niri windows/workspaces channels would eliminate even the workspace
  subscriber wake. The current pointer check and no-op guard achieve the smaller
  change first, without changing the publication protocol.
- Direct dirty-ID deltas and a different reorder algorithm could remove more
  scans. They need reconnect and coalescing semantics across skipped frames.
- Dropping the taskbar join entirely would resurrect stale foreign-toplevel
  handles. Caching preserves its correctness purpose.
- Plain mutable JS overview records would retain identity but fail to notify
  bindings. QML-backed properties preserve both identity and notifications.
- A standalone native keyboard helper could also remove Python RAM, but would
  introduce another process/protocol. The existing plugin and lease system were
  chosen; Python remains available when the plugin or permissions are absent.
- Rewriting Tailscale, VPN import, or all hotspot actions is a separate change.
  The state mirror must be correct before removing those acquisition paths.

## Verification

- 34 Rust tests across core and Qt, including readiness/removal, cancellation,
  signal decoding, no-op/revert coalescing and row-diff behavior.
- Two CTest targets and live NetworkManager bridge smoke pass. The model test
  explicitly verifies title publications do not increment workspace applies.
- With 100 QML role views attached, single-row apply measured 11.94 µs and an
  all-row title change 109.04 µs. Only the narrow-event interpretation of the
  50 µs gate passes; this does not meet a 50 µs bound on every bulk update.
- JS matching tests and five Qt behavior tests cover one-to-one mapping,
  invalidation, ghost exclusion, fallback snapshots, reactive overview metadata
  and rebuild coalescing. Six Rust/QML contract checks pass.
- Isolated probes of the actual VPN/hotspot QML cover activation/removal,
  WireGuard, Tailscale separation, summary consistency and balanced leases.
- A short live comparison is recorded in
  [the verification A/B](../performance/2026-10-14-chatgpt-verify/REPORT.md) (the earlier one-run quick pair is raw data only: `2026-10-13-five-hotspots-quick-pair/results.json`).
  It is a smoke comparison, not a statistically established rollout gate.

The shell unit now specifies a 60-second start timeout, instead of inheriting
the user's 15-second default. This avoids killing a valid cold boot before it
claims the StatusNotifier bus name; it does not claim to speed up startup.
