# Bluetooth manager and discovery

Discovery views now request a 25-second scan. Closing a panel, changing its
adapter, switching away from the discovery page, locking the screen or powering
off an adapter releases that view's request. Search again starts a new window.
Two visible views share one native discovery session per adapter; closing one
does not stop the other. A late StartDiscovery reply is followed by cleanup.
Another client's scan (including LibrePods) is not stopped by iNiR. The UI
distinguishes that scan from its own search spinner.

Device rows show pending actions and per-device failures before the normal
connection label. A failed disconnect is not treated as a successful disconnect:
the underlying connection state remains whatever BlueZ reports. Retrying clears
the old row error, and buttons are disabled while a device action is pending.

Quickshell remains the only Bluetooth device-list observer. The Rust controller
only handles commands, adapter discovery state, agents, transfers and Bluetooth
network connections. It uses the shared Rust runtime, independently of
`INIR_RUST_BACKEND`. Hidden views stop sorting and disconnect presentation
subscriptions. Visible lists batch relevant changes at 200 ms and retain QObject
identity; RSSI/manufacturer advertisement updates do not reorder the list.

The night-light page no longer starts discovery. Retained action-center pages
stop scanning when hidden, without waiting for their destruction.

## Open and choose a mode

Run `inir bluetooth` to open Devices, Transfers and Internet. Quick panels still
provide their existing controls. `inir bluetooth status` prints readiness,
own scan leases, global discovery, active transfers and errors.

The default configuration is:

```json
"bluetooth": { "managerMode": "external", "receiveDirectory": "" }
```

External mode keeps Blueman as the pairing/transfer agent while applying the
discovery fixes. Integrated mode registers a KeyboardDisplay pairing agent and
an OBEX receiving agent. Prompts open even if the manager is closed; lock,
rejection, daemon cancellation and timeouts dismiss them. A receive directory
left empty means `Downloads/Bluetooth`.

The integrated switch only selects the agent mode; it does not disable Blueman
autostart. For a controlled handoff, use the commands below. Custom Bluetooth
launcher commands are preserved; the legacy `blueman-manager` launcher is
redirected in integrated mode.

## Dependencies and features

The native QML module must be built and on `QML_IMPORT_PATH`. BlueZ's
`bluetoothd` stays running. LibrePods is not stopped or removed.

File transfers require the BlueZ OBEX session service (`bluez-obex` on Arch).
Incoming files require explicit approval, use sanitized names and private
`.incoming-*` staging directories, and are finalized without overwriting existing
files. Interrupted partial files remain in their staging directory for recovery;
the transfer row shows the path/error. Outgoing transfers validate readable local
files, publish progress in a stable Qt list model and support cancellation.

Internet features require NetworkManager with its Bluetooth plugin. Phone-to-
laptop tethering uses a PANU profile; enable tethering on the phone first.
Laptop-to-device sharing uses a NAP bridge with IPv4 shared mode and requires
`dnsmasq`. New profiles have autoconnect disabled. Compatible PANU profiles and
iNiR-tagged profiles are reused. Internet connections persist when the window
closes and can be disconnected from the Internet tab. Only Bluetooth active
connections may be disconnected through this controller.

Missing services expose readiness/errors instead of silently invoking competing
command-line agents. Daemon restarts close the old client, cancel prompts and
reconnect with backoff. A pending scan resumes only for its original remaining
time, not another 25 seconds.

## Reversible Blueman handoff

First finish file transfers and stop any existing Blueman internet sharing.

```sh
inir bluetooth preview
# Test your actual devices; autostart is still unchanged.
inir bluetooth rollback
```

Preview temporarily stops Blueman's applet and selects integrated mode after
checking pairing, OBEX and NetworkManager readiness. It records the original
mode/launcher. A failed handoff restores the previous selection and applet.

Before disabling autostart, verify:

1. Headphone connection/reconnection and keyboard PIN/passkey pairing.
2. A confirmation prompt arriving while the manager is closed; reject/cancel
   and screen-lock handling.
3. Both sending and receiving a real file, cancellation, and filename collisions.
4. Phone-to-laptop internet and laptop-to-peer internet, including disconnect.
5. Panel close, page switch, adapter switch and the 25-second timeout; another
   client's discovery should remain unaffected.

Then explicitly confirm:

```sh
inir bluetooth enable --hardware-verified
inir bluetooth rollback
```

Enable writes only the current user's `autostart/blueman.desktop` Hidden override,
with a backup in the XDG state directory (`inir/bluetooth-migration.json`). Blueman
remains installed. Rollback restores the original file and settings; subsequent
manual edits and symlinks are protected rather than overwritten. Without real
hardware verification, keep the external default and Blueman autostart.

## Verification

```sh
cargo test --manifest-path rust/Cargo.toml -p inir-core --test bluetooth_control --locked
python scripts/test-bluetooth-discovery.py
python scripts/test-bluetooth-manager.py
python scripts/test-bluetooth-components.py # needs a Wayland session, compiles only
python scripts/test-rust-contract.py
ctest --test-dir rust/build -R bluetooth_commands --output-on-failure
python scripts/profile-bluetooth.py --duration 5
```

Core tests use a private D-Bus daemon, not the desktop's Bluetooth service. They
exercise acknowledged scan ownership/expiry, the exported pairing agent, lock
and cancellation, safe receive finalization, and network profile defaults. QML
tests run the production discovery helper with deterministic peers and cover
retained-page visibility, independent owners, batching and lock cleanup.
The component smoke check compiles all nine affected Bluetooth surfaces without
instantiating their windows or singleton consumers.
The native command test uses a private bus and the real QML-to-Rust bridge to
verify object arguments survive conversion. It also checks nested file arrays
and booleans; missing-field validation must not replace supplied adapter IDs.

Measure idle, scanning and post-timeout process/thread CPU separately. Global
`Discovering=true` can legitimately persist when another client owns a session;
the iNiR lease/owned fields identify its own demand. CPU percentage, thread count
and temperature are different measurements: these changes remove unnecessary
Bluetooth work but do not promise a particular temperature reduction.

Protocol references: [BlueZ Adapter API](https://github.com/bluez/bluez/blob/master/doc/org.bluez.Adapter.rst),
[pairing agent example](https://github.com/bluez/bluez/blob/master/test/simple-agent),
[OBEX agent example](https://github.com/bluez/bluez/blob/master/test/simple-obex-agent),
and [NetworkManager Bluetooth examples](https://networkmanager.pages.freedesktop.org/NetworkManager/NetworkManager/nmcli-examples.html).
