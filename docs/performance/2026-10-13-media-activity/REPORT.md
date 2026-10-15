# Media presentation and LiveLayer activity — 2026-10-13

Implemented plan items 3 and 4: media presentation gates and the conservative
LiveLayer frame-callback gate. Playback, persistent settings, backend ownership
policy and the existing output coverage policy are not changed.

## Media changes

- All nine media presets pass visibility/motion eligibility to their position
  ticker. Lyrics and expanding-lyrics presets now expose the motion property
  that the desktop loader already attempted to assign.
- The ticker keeps the existing 250 ms cadence, stops while inactive/paused,
  and publishes immediately on resume or replacement of an active player. It
  emits a position notification, never a seek or a playback command.
- Visualizer overlays forward motion eligibility to the shared renderer.
  Samples and normalization scale are held together when motion is suspended.
  A Qt sequence is copied on freezing, not on every live audio frame.
- The retained home/away Canvas copies use drawing ownership to gate paint
  requests and opacity animation. The new owner queues a current-frame repaint.
  Inactive copies prime once on creation/resize for surface handoff; live sample
  and scale updates cannot keep repainting them.
- The desktop's Cava lease still controls acquisition. Presentation reads the
  shared producer to avoid clearing its last frame when the lease is released.
  Another visible consumer can keep Cava running without animating a frozen one.
  The existing shared-process subscription lifecycle and 800 ms shutdown debounce
  remain unchanged.
- Desktop presentation respects existing output-local motion/power eligibility
  and stops at zero widget opacity. A partially visible desktop is not treated
  as covered merely because any window exists.

Metadata/artwork services and shared controller synchronization remain active.
This is not a claim that every media-related operation in the shell is now
visibility-gated. In particular, shared lyrics/metadata work was not rewritten.

## LiveLayer changes

`trackingHostFrames` requires live content, permission to detach, a calm host,
visibility, non-null content and positive dimensions. The host-frame connection
is disabled otherwise, and eligibility changes still return content home.

Eligible instances retain the original per-frame ancestor geometry, opacity,
clipping and effect checks. There is no new ancestor-cache/dirty-flag mechanism:
that more complex optimization remains deferred. Surface handoff, the settle
delay, placement invalidation and host recreation behavior remain intact.

## Validation

All of the following pass:

- 12 real QML ticker/snapshot cases: inactive and paused gates, stopped timers,
  immediate resume, player replacement, no seeking, mutable Qt-sequence
  isolation, frozen normalization, initial handoff priming and current-frame recovery.
- Five real Canvas cases using the unmodified production component with theme
  dependencies stubbed: live updates paint, hidden/non-drawing copies do not,
  reactivation paints the current frame, and resizing primes a retained copy
  without restarting dynamic painting.
- 24 checks on the real LiveLayer in transparent, input-inert native Wayland
  surfaces: detach/recovery, ancestor movement and fading, clipping/effects,
  visibility/permission/host/size/content gates, host recreation and no more
  than one drawing owner. This tests the actual component, not a mirror of its
  geometry algorithm. Multi-monitor hardware transitions were not exercised.
- All 13 existing wallpaper-retention functional cases.
- Preset-wiring checks, helper `qmllint`, repository QML pitfalls/Qt compatibility
  checks, and `git diff --check`.

Reproduce with:

```sh
python3 scripts/test-media-activity.py
python3 scripts/test-live-layer-activity.py
```

The first command runs headlessly. The second maps small transparent test
surfaces temporarily; they do not take keyboard focus or intercept input.

## Trace evidence

The pre-gate diagnostic recorded 4,308 QML / 4,542 Rust
`LiveLayer.onAfterAnimating` callbacks over ten workspace commands. The verified
final traces recorded zero such callbacks in both backends; `evaluate()` still ran for 20 live-state
notifications in each baseline trace. The native fixture separately proves that
eligible moving content still receives frame checks and detaches correctly.

These are targeted lifecycle observations, not a percentage claim about total
CPU, GPU, frame rate or input latency. Qt trace durations are inclusive
instrumented wall time and must not be added as independent CPU time.

## Performance experiment limitations

The benchmark gained an owned silent MPRIS fixture. It reports a progressing
synthetic track so position/playing paths can be exercised without controlling
the user's players. It produces no audio and is removed on controller cleanup;
this is not a representative real-audio visualizer benchmark.

`before/` and `media-only/` each contain four unprofiled launches, 15-second
warmups and 20-second baseline/media-widget-disabled captures. Their raw CPU
means rose in the first implementation series rather than demonstrating a win.
They are retained for transparency, not accepted as an optimization success.

During these sequential captures, unrelated Niri facade/sorting/record code,
keyboard/network integrations, test scripts and the systemd unit changed in the
shared checkout. The controller's default native-domain set also expanded from
three domains to four, and the live window count changed from four to three.
No bit-identical source snapshot exists for every launch.
Those series therefore cannot establish a causal before/after CPU regression or
improvement from this change. Unprofiled versus profiled values are not pooled.

`media-copy-diagnostic/` and `final-traces/` retain the separate diagnostic
captures. The latter precedes the final lease-local-to-shared sample-source
adjustment; its LiveLayer gate is identical to the final implementation.
`verified-smoke/` checks the shared sample-source change before the final one-shot
texture-priming adjustment. `verified-traces/` records the final implementation.
Both explicitly fix native domains to `network,brightness,niri`. They are
smoke/behavior checks, not a fresh controlled performance comparison against
unchanged old code.

The outcome is verified activity gating with no claimed whole-shell percentage
saving. A stable-checkout, controlled real-audio comparison is still needed to
quantify that saving and the remaining rendering cost.

After the last capture, one managed shell was running, no owned test helpers were
orphaned, the synthetic MPRIS name was released, idle inhibition was restored to
false, and normal media state was inactive. User playback was never commanded.
