import QtQuick
import QtTest
import qs
import qs.services

TestCase {
    id: test
    name: "BluetoothDiscoveryLifecycle"
    when: windowShown
    property var view: null
    property var other: null
    QtObject {
        id: testAdapter
        property bool enabled: true
        property bool discovering: false
        property string adapterId: "hci0"
        property QtObject devices: QtObject { property var values: [] }
    }
    QtObject { id: alpha; property string name: "Alpha"; property string address: "A"; property bool paired: false; property bool connected: false; property int rssi: 0 }
    QtObject { id: beta; property string name: "Beta"; property string address: "B"; property bool paired: false; property bool connected: false }
    Component { id: factory; BluetoothDiscoveryView { adapter: testAdapter } }
    SignalSpy { id: changed; target: test.view; signalName: "devicesChanged" }

    function init() {
        GlobalStates.screenLocked = false
        testAdapter.enabled = true
        testAdapter.discovering = false
        alpha.connected = false
        alpha.paired = false
        alpha.name = "Alpha"
        beta.connected = false
        testAdapter.devices.values = [beta, alpha]
        BluetoothStatus.leases = ({})
        BluetoothStatus.starts = 0
        BluetoothStatus.releases = 0
        view = factory.createObject(test)
        changed.clear()
    }
    function cleanup() {
        if (other) { other.destroy(); other = null }
        if (view) { view.destroy(); view = null }
        wait(0)
        compare(Object.keys(BluetoothStatus.leases).length, 0)
    }
    function test_open_close_retained_view() {
        view.active = true
        compare(BluetoothStatus.starts, 1)
        compare(view.devices[0], alpha)
        view.active = false
        compare(BluetoothStatus.releases, 1)
        beta.connected = true
        wait(230)
        compare(view.devices[0], alpha, "hidden pages must not sort")
        view.active = true
        compare(view.devices[0], beta, "resume rebuilds immediately")
        compare(BluetoothStatus.starts, 2)
    }
    function test_distinct_owners() {
        view.active = true
        const owner = view.owner
        other = factory.createObject(test, {active: true})
        compare(view.owner, owner, "allocating another view must not change this view's identity")
        compare(Object.keys(BluetoothStatus.leases).length, 2)
        verify(view.owner !== other.owner)
        view.active = false
        verify(other.scanning)
        compare(Object.keys(BluetoothStatus.leases).length, 1)
    }
    function test_advertisements_do_not_republish_or_sort() {
        view.active = true
        changed.clear()
        for (let i = 0; i < 100; ++i) alpha.rssi = i
        wait(230)
        compare(changed.count, 0)
        const saved = view.devices
        view.refresh()
        compare(view.devices, saved)
    }
    function test_relevant_changes_are_batched() {
        view.active = true
        changed.clear()
        beta.connected = true
        beta.connected = false
        beta.connected = true
        compare(changed.count, 0)
        wait(230)
        compare(changed.count, 1)
        compare(view.devices[0], beta)
    }
    function test_scan_not_restarted_by_device_updates() {
        view.active = true
        view.stopScan()
        beta.connected = true
        testAdapter.devices.values = [alpha, beta]
        wait(230)
        compare(BluetoothStatus.starts, 1)
        verify(!view.scanning)
        view.rescan()
        compare(BluetoothStatus.starts, 2)
    }
    function test_lock_releases_and_quiets_presentation() {
        view.active = true
        GlobalStates.screenLocked = true
        compare(Object.keys(BluetoothStatus.leases).length, 0)
        beta.connected = true
        wait(230)
        compare(view.devices[0], alpha)
        GlobalStates.screenLocked = false
        compare(view.devices[0], beta)
    }
    function test_known_devices_view_does_not_auto_scan() {
        view.autoScan = false
        view.active = true
        compare(BluetoothStatus.starts, 0)
        compare(view.devices.length, 2)
        view.rescan()
        compare(BluetoothStatus.starts, 1)
    }
    function test_external_scan_is_not_our_spinner() {
        view.autoScan = false
        view.active = true
        testAdapter.discovering = true
        verify(view.externalDiscovery)
        verify(!view.scanning)
        view.active = false
        verify(testAdapter.discovering)
    }
}
