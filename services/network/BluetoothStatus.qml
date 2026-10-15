pragma Singleton
pragma ComponentBehavior: Bound

import Quickshell
import Quickshell.Bluetooth
import Quickshell.Io
import QtQuick
import QtQml.Models
import qs
import qs.services
import qs.modules.common
import "bluetoothActions.js" as DeviceActions

/**
 * Bluetooth status service.
 */
Singleton {
    id: root

    readonly property var backendBluetooth: NativeBackend.desktop
    readonly property bool nativeBluetoothReady: backendBluetooth?.bluetoothReady ?? false
    readonly property bool available: nativeBluetoothReady ? backendBluetooth.bluetoothAdapters.count > 0 : Bluetooth.adapters.values.length > 0
    readonly property bool enabled: nativeBluetoothReady ? backendBluetooth.bluetoothEnabled : Bluetooth.defaultAdapter?.enabled ?? false
    readonly property BluetoothDevice firstActiveDevice: Bluetooth.defaultAdapter?.devices.values.find(device => device.connected) ?? null
    readonly property int activeDeviceCount: nativeBluetoothReady ? backendBluetooth.bluetoothConnectedCount : Bluetooth.defaultAdapter?.devices.values.filter(device => device.connected).length ?? 0
    readonly property bool connected: nativeBluetoothReady ? activeDeviceCount > 0 : Bluetooth.devices.values.some(d => d.connected)

    readonly property bool integrated: (Config.options?.bluetooth?.managerMode ?? "external") === "integrated"
    readonly property string receiveDirectory: Config.options?.bluetooth?.receiveDirectory || (Directories.downloadsPath + "/Bluetooth")
    readonly property var controller: provider.status === Loader.Ready ? provider.item.controller : null
    onControllerChanged: Qt.callLater(root._restoreLeases)
    readonly property var controlState: provider.status === Loader.Ready ? provider.item.status : ({})
    readonly property var transfers: controller
    readonly property var prompt: GlobalStates.screenLocked ? null : (controlState.prompts?.[0] ?? null)
    property bool managerOpen: false
    property var managerDetails: ({})
    property string lastError: ""
    property int _sequence: 0
    property var _callbacks: ({})
    property var _requests: ({})
    property var _deviceActions: ({})
    property var _leases: ({})
    property var _fallbackOwned: ({})

    Loader {
        id: provider
        active: Config.ready
        asynchronous: true
        source: "../native/BluetoothProvider.qml"
    }

    function newScanOwner(): string { return "bluetooth-view-" + (++root._sequence) }
    function scanRequested(owner: string): bool {
        return (root._leases[owner]?.deadline ?? 0) > Date.now()
    }
    function requestScan(owner: string, adapter): bool {
        if (!owner || !adapter?.enabled || GlobalStates.screenLocked) return false
        root.releaseScan(owner)
        const next = Object.assign({}, root._leases)
        next[owner] = { adapter: adapter, deadline: Date.now() + 25000 }
        root._leases = next
        if (root.controller) root.controller.scan(owner, adapter.adapterId, 25000)
        else if (provider.status !== Loader.Loading && !adapter.discovering) {
            const owned = Object.assign({}, root._fallbackOwned)
            owned[adapter.adapterId] = adapter
            root._fallbackOwned = owned
            adapter.discovering = true
        }
        root._scheduleExpiry()
        return true
    }
    function releaseScan(owner: string): void {
        if (!(owner in root._leases)) return
        const next = Object.assign({}, root._leases)
        delete next[owner]
        root._leases = next
        if (root.controller) root.controller.releaseScan(owner)
        root._reconcileFallback()
        root._scheduleExpiry()
    }
    function releaseAllScans(): void {
        for (const owner of Object.keys(root._leases)) root.releaseScan(owner)
        root.controller?.releaseAll()
    }
    function _restoreLeases(): void {
        if (!root.controller?.ready) return
        // Retire the legacy observer's own sessions before the native client
        // takes over; never stop an already-discovering external client.
        for (const id of Object.keys(root._fallbackOwned)) {
            if (root._fallbackOwned[id]?.discovering) root._fallbackOwned[id].discovering = false
        }
        root._fallbackOwned = ({})
        for (const owner of Object.keys(root._leases)) {
            const lease = root._leases[owner]
            const remaining = Math.min(25000, lease.deadline - Date.now())
            if (remaining > 0 && lease.adapter?.enabled)
                root.controller.scan(owner, lease.adapter.adapterId, remaining)
        }
    }
    function _reconcileFallback(): void {
        const owned = Object.assign({}, root._fallbackOwned)
        for (const id of Object.keys(owned)) {
            const wanted = Object.values(root._leases).some(lease => lease.adapter?.adapterId === id)
            if (!wanted && owned[id]?.discovering) {
                owned[id].discovering = false
                delete owned[id]
            }
        }
        root._fallbackOwned = owned
    }
    function _scheduleExpiry(): void {
        expiry.stop()
        const deadlines = Object.values(root._leases).map(lease => lease.deadline)
        if (deadlines.length > 0) {
            expiry.interval = Math.max(1, Math.min(...deadlines) - Date.now())
            expiry.start()
        }
    }
    Timer {
        id: expiry
        onTriggered: {
            for (const owner of Object.keys(root._leases))
                if (root._leases[owner].deadline <= Date.now()) root.releaseScan(owner)
            root._scheduleExpiry()
        }
    }
    Instantiator {
        model: Object.values(root._fallbackOwned)
        delegate: Connections {
            required property var modelData
            target: modelData
            function onDiscoveringChanged(): void { root._reconcileFallback() }
        }
    }
    Connections {
        target: GlobalStates
        function onScreenLockedChanged(): void {
            root.controller?.setLocked(GlobalStates.screenLocked)
            if (GlobalStates.screenLocked) { root.managerOpen = false; root.releaseAllScans() }
        }
    }
    function execute(operation: string, parameters, callback = null): bool {
        if (root.deviceActionForArguments(parameters).phase === "pending") {
            root.lastError = Translation.tr("Another action for this device is still in progress")
            if (callback) callback(false, root.lastError)
            return false
        }
        const id = String(++root._sequence)
        root._requests[id] = { operation: operation, parameters: parameters }
        if (callback) root._callbacks[id] = callback
        root._deviceActions = DeviceActions.begin(root._deviceActions, parameters, id, operation)
        if (!root.controller?.ready) {
            root._complete(id, false, Translation.tr("Bluetooth controller is unavailable"))
            return false
        }
        root.lastError = ""
        if (root.controller.execute(id, operation, parameters)) return true
        root._complete(id, false, Translation.tr("Bluetooth request could not be queued"))
        return false
    }
    function _complete(id: string, success: bool, error: string): void {
        const request = root._requests[id]
        if (!request) return
        delete root._requests[id]
        const callback = root._callbacks[id]
        delete root._callbacks[id]
        root._deviceActions = DeviceActions.finish(root._deviceActions, request.parameters, id, success, error)
        if (!success) root.lastError = error
        if (callback) callback(success, error)
    }
    function deviceActionForArguments(parameters): var {
        return DeviceActions.current(root._deviceActions, parameters)
    }
    function deviceAction(device): var { return root.deviceActionForArguments(root.deviceArguments(device)) }
    function devicePresentation(device): string {
        return DeviceActions.presentation(root.deviceAction(device), device?.connected ?? false, device?.paired ?? false)
    }
    function deviceActionLabel(action): string {
        const failed = action.phase === "error"
        switch (action.operation) {
        case "connect": return failed ? Translation.tr("Connection failed") : Translation.tr("Connecting…")
        case "disconnect": return failed ? Translation.tr("Disconnect failed") : Translation.tr("Disconnecting…")
        case "pair": return failed ? Translation.tr("Pairing failed") : Translation.tr("Pairing…")
        case "forget": return failed ? Translation.tr("Forget failed") : Translation.tr("Forgetting…")
        case "trust": return failed ? Translation.tr("Trust update failed") : Translation.tr("Updating trust…")
        case "sendFiles": return failed ? Translation.tr("File send failed") : Translation.tr("Preparing files…")
        case "connectTether": return failed ? Translation.tr("Tethering failed") : Translation.tr("Connecting internet…")
        default: return failed ? Translation.tr("Bluetooth action failed") : Translation.tr("Working…")
        }
    }
    function deviceArguments(device): var {
        return { adapter: device?.adapter?.adapterId ?? Bluetooth.defaultAdapter?.adapterId ?? "", address: device?.address ?? "" }
    }
    function connectDevice(device): void {
        if (root.controller?.ready) root.execute("connect", root.deviceArguments(device))
        else device?.connect()
    }
    function disconnectDevice(device): void {
        if (root.controller?.ready) root.execute("disconnect", root.deviceArguments(device))
        else device?.disconnect()
    }
    function forgetDevice(device): void {
        if (root.controller?.ready) root.execute("forget", root.deviceArguments(device))
        else device?.forget()
    }
    function respond(id: string, accepted: bool, value = ""): void { root.controller?.respond(id, accepted, value) }
    function openManager(): void { if (!GlobalStates.screenLocked) root.managerOpen = true }
    Connections {
        target: root.controller
        function onReadyChanged(): void {
            if (root.controller?.ready) root._restoreLeases()
            else {
                for (const id of Object.keys(root._requests))
                    root._complete(id, false, Translation.tr("Bluetooth service disconnected"))
            }
        }
        function onCommandFinished(id, success, error): void {
            root._complete(id, success, error)
        }
        function onStateUpdated(): void {
            for (const scan of root.controlState.scans ?? []) {
                if (!scan.error) continue
                root.lastError = scan.error
                for (const owner of Object.keys(root._leases))
                    if (root._leases[owner].adapter?.adapterId === scan.adapter) root.releaseScan(owner)
            }
        }
    }
    IpcHandler {
        target: "bluetooth"
        function open(): void { root.openManager() }
        function close(): void { root.managerOpen = false }
        function status(): string {
            return JSON.stringify({ mode: root.integrated ? "integrated" : "external", available: !!root.controller,
                ready: root.controller?.ready ?? false, state: root.controlState,
                launcher: AppLauncher.commandFor("bluetooth"), managerOpen: root.managerOpen,
                locked: GlobalStates.screenLocked,
                view: root.managerOpen && !GlobalStates.screenLocked ? root.managerDetails : ({}),
                leases: Object.keys(root._leases), transfers: root.transfers?.count ?? 0,
                discovering: Bluetooth.defaultAdapter?.discovering ?? false, error: root.lastError })
        }
        function setMode(mode: string): bool {
            if (!["external", "integrated"].includes(mode)) return false
            Config.setNestedValue("bluetooth.managerMode", mode)
            return true
        }
        function migrateLauncher(): void {
            if (AppLauncher.commandFor("bluetooth") === "blueman-manager") Config.setNestedValue("apps.bluetooth", "inir bluetooth")
        }
        function restoreSettings(mode: string, launcher: string): void {
            if (["external", "integrated"].includes(mode)) Config.setNestedValue("bluetooth.managerMode", mode)
            if (AppLauncher.commandFor("bluetooth") === "inir bluetooth") Config.setNestedValue("apps.bluetooth", launcher)
        }
    }

    // Address of the device currently going through the pairing flow, empty when
    // idle. Connecting and disconnecting a known device go through the Quickshell
    // BluetoothDevice object instead and need nothing here.
    property string pairingAddress: ""
    signal pairFinished(string address, bool ok)

    /**
     * Pairs, trusts and connects an unpaired device in one pass.
     *
     * BlueZ exposes no single "pair and use" call, and pairing without trusting
     * leaves a device that disconnects for good on the next power cycle. Both
     * pair and connect get a 30s timeout because an unresponsive device
     * otherwise leaves bluetoothctl waiting indefinitely. The address travels as
     * a positional argument so it cannot break or inject the command.
     */
    function pairDevice(address: string): void {
        if (root.integrated) {
            if (root.pairingAddress || !address.length) return
            root.pairingAddress = address
            root.execute("pair", { adapter: Bluetooth.defaultAdapter?.adapterId ?? "", address: address },
                (ok, error) => { root.pairingAddress = ""; root.pairFinished(address, ok) })
            return
        }
        if (pairProc.running || !address.length)
            return;
        root.pairingAddress = address;
        pairProc.command = ["sh", "-c",
            'timeout 30 bluetoothctl pair "$1" && bluetoothctl trust "$1" && timeout 30 bluetoothctl connect "$1"',
            "sh", address];
        pairProc.running = true;
    }

    Process {
        id: pairProc
        stdout: StdioCollector {}
        stderr: StdioCollector {}
        onExited: exitCode => {
            const address = root.pairingAddress;
            root.pairingAddress = "";
            root.pairFinished(address, exitCode === 0);
        }
    }

    // Material Symbol icon for the currently-active device, or generic bluetooth
    // states when no device is connected. Uses BluetoothDevice.icon (XDG icon
    // name like "audio-headset", "input-keyboard") to pick a device-specific
    // glyph so the bar reflects what's actually connected.
    readonly property string activeIcon: {
        if (!root.enabled) return "bluetooth_disabled";
        if (!root.connected) return "bluetooth";
        return root._materialIconForDevice(root.firstActiveDevice);
    }

    function activeDeviceSummary(includeAdditionalCount = false): string {
        const device = root.firstActiveDevice;
        if (!device) return "";
        let summary = device.name || Translation.tr("Unknown device");
        if (device.batteryAvailable)
            summary += ` (${Math.round(device.battery * 100)}%)`;
        if (includeAdditionalCount && root.activeDeviceCount > 1)
            summary += ` +${root.activeDeviceCount - 1}`;
        return summary;
    }

    function connectionTooltip(): string {
        if (!root.enabled) return Translation.tr("Bluetooth is disabled");
        if (!root.connected) return Translation.tr("Bluetooth disconnected");
        return root.activeDeviceSummary() || Translation.tr("Bluetooth connected");
    }

    function iconForDevice(device: BluetoothDevice): string {
        return root._materialIconForDevice(device);
    }

    function _materialIconForDevice(device: BluetoothDevice): string {
        const xdg = (device?.icon ?? "").toLowerCase();
        if (xdg.length === 0) return "bluetooth_connected";
        if (xdg.includes("headset") || xdg.includes("headphone")) return "headphones";
        if (xdg.includes("audio-card") || xdg.includes("speaker")) return "speaker";
        if (xdg.includes("audio")) return "speaker";
        if (xdg.includes("keyboard")) return "keyboard";
        if (xdg.includes("mouse") || xdg.includes("pointer")) return "mouse";
        if (xdg.includes("phone")) return "smartphone";
        if (xdg.includes("watch")) return "watch";
        if (xdg.includes("camera")) return "photo_camera";
        if (xdg.includes("printer")) return "print";
        if (xdg.includes("scanner")) return "scanner";
        if (xdg.includes("gamepad") || xdg.includes("joystick") || xdg.includes("input-gaming")) return "sports_esports";
        if (xdg.includes("computer") || xdg.includes("laptop")) return "laptop";
        if (xdg.includes("tablet")) return "tablet";
        if (xdg.includes("tv") || xdg.includes("video")) return "tv";
        return "bluetooth_connected";
    }
}
