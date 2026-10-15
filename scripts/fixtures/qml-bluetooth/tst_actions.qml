import QtQuick
import QtTest
import "bluetoothActions.js" as Actions

TestCase {
    name: "BluetoothActionOutcomes"
    readonly property var device: ({adapter: "hci0", address: "AA:BB:CC:DD:EE:FF"})

    function test_pending_disconnect_is_not_shown_as_connected() {
        const actions = Actions.begin({}, device, "1", "disconnect")
        const action = Actions.current(actions, device)
        compare(action.operation, "disconnect")
        compare(Actions.presentation(action, true, true), "pending")
    }
    function test_failed_disconnect_shows_error_without_faking_disconnection() {
        const started = Actions.begin({}, device, "1", "disconnect")
        const actions = Actions.finish(started, device, "1", false, "Missing adapter")
        const action = Actions.current(actions, device)
        compare(action.error, "Missing adapter")
        compare(Actions.presentation(action, true, true), "error")
        compare(Actions.presentation(action, false, true), "error")
    }
    function test_success_returns_to_observed_bluez_state() {
        const started = Actions.begin({}, device, "1", "disconnect")
        const actions = Actions.finish(started, device, "1", true, "")
        const action = Actions.current(actions, device)
        compare(action.error, "")
        compare(Actions.presentation(action, false, true), "paired")
        compare(Actions.presentation(action, true, true), "connected")
    }
    function test_retry_clears_previous_error() {
        const started = Actions.begin({}, device, "1", "connect")
        const failed = Actions.finish(started, device, "1", false, "Device unavailable")
        const retry = Actions.begin(failed, device, "2", "connect")
        compare(Actions.current(retry, device).error, "")
        compare(Actions.presentation(Actions.current(retry, device), false, true), "pending")
    }
    function test_stale_completion_cannot_overwrite_a_new_action() {
        const first = Actions.begin({}, device, "1", "connect")
        const second = Actions.begin(first, device, "2", "disconnect")
        const stale = Actions.finish(second, device, "1", false, "old error")
        compare(stale, second)
        compare(Actions.current(stale, device).operation, "disconnect")
        compare(Actions.current(stale, device).phase, "pending")
    }
    function test_other_devices_and_adapters_keep_their_own_state() {
        const other = {adapter: "hci1", address: device.address}
        const first = Actions.begin({}, device, "1", "disconnect")
        const second = Actions.begin(first, other, "2", "connect")
        const finished = Actions.finish(second, device, "1", false, "Failed")
        compare(Actions.current(finished, other).phase, "pending")
        compare(Actions.current(finished, device).phase, "error")
        compare(Actions.key({adapter: "hci0", address: device.address.toLowerCase()}), Actions.key(device))
    }
    function test_adapter_actions_do_not_create_device_errors() {
        const actions = ({})
        compare(Actions.begin(actions, {adapter: "hci0", enabled: true}, "1", "power"), actions)
        compare(Actions.finish(actions, device, "missing", false, "old error"), actions)
    }
}
