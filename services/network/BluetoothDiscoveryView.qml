pragma ComponentBehavior: Bound
import QtQuick
import QtQml.Models
import Quickshell.Bluetooth
import qs.services
import qs

// Presentation work and scan demand are scoped to a visible discovery view.
Item {
    id: root
    property bool active: false
    readonly property bool presentationActive: root.active && !GlobalStates.screenLocked
    property bool autoScan: true
    property var adapter: Bluetooth.defaultAdapter
    // Allocate once, outside a binding: the allocator's sequence is observable
    // and a binding would change every existing view's identity on allocation.
    property string owner: ""
    readonly property bool scanning: BluetoothStatus.scanRequested(root.owner)
    readonly property bool externalDiscovery: !root.scanning && (root.adapter?.discovering ?? false)
    property var devices: []
    readonly property var source: root.adapter?.devices?.values ?? []

    function rescan(): void { BluetoothStatus.requestScan(root.owner, root.adapter) }
    function stopScan(): void { BluetoothStatus.releaseScan(root.owner) }
    function sync(): void {
        root.stopScan()
        if (root.presentationActive) {
            root.refresh()
            if (root.autoScan) root.rescan()
        } else rebuild.stop()
    }
    function schedule(): void {
        // Fixed-window batching: restarting on every event would starve updates
        // during sustained advertisement traffic.
        if (root.presentationActive && !rebuild.running) rebuild.start()
    }
    function refresh(): void {
        if (!root.presentationActive) return
        const next = Array.from(root.source).sort((a, b) => {
            const meaningful = d => !!d.name && !/^([0-9A-Fa-f]{2}[:-]){5}[0-9A-Fa-f]{2}$/.test(d.name)
            const rank = d => d.connected ? 0 : d.paired ? 1 : meaningful(d) ? 2 : 3
            return rank(a) - rank(b) || String(a.name ?? "").localeCompare(String(b.name ?? ""))
                || String(a.address ?? "").localeCompare(String(b.address ?? ""))
        })
        if (next.length !== root.devices.length || next.some((d, i) => d !== root.devices[i])) root.devices = next
    }
    onPresentationActiveChanged: sync()
    onAdapterChanged: sync()
    onSourceChanged: schedule()
    Component.onCompleted: { owner = BluetoothStatus.newScanOwner(); sync() }
    Component.onDestruction: stopScan()
    Timer { id: rebuild; interval: 200; onTriggered: root.refresh() }
    Connections {
        target: root.adapter ?? null
        function onEnabledChanged(): void { root.sync() }
    }
    Instantiator {
        active: root.presentationActive
        model: root.presentationActive ? root.devices : []
        delegate: QtObject {
            required property var modelData
            property Connections fields: Connections {
                target: modelData
                function onNameChanged(): void { root.schedule() }
                function onPairedChanged(): void { root.schedule() }
                function onConnectedChanged(): void { root.schedule() }
            }
        }
    }
}
