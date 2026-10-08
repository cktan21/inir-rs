pragma Singleton
pragma ComponentBehavior: Bound

import Quickshell
import Quickshell.Bluetooth
import Quickshell.Io
import QtQuick
import qs.services

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
