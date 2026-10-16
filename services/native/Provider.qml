pragma ComponentBehavior: Bound
import QtQuick
import qs.services.native as Native
import Quickshell
import qs.services

Item {
    readonly property var systemInfo: Native.SystemInfo
    readonly property var config: Native.ConfigService
    readonly property var niri: Native.NativeNiri
    readonly property var workspaces: Native.NativeWorkspaces
    readonly property var network: Native.NativeNetwork
    readonly property var brightness: Native.NativeBrightness
    readonly property var power: Native.NativePower
    // Lease is owned by KeyboardIndicators through Utilities. This compatibility
    // alias observes the same singleton and never starts another input worker.
    readonly property var keyboard: selected("keyboard") ? Native.NativeKeyboard : null
    function selected(name) {
        const domains = Quickshell.env("INIR_NATIVE_DOMAINS") || "network,brightness,niri,keyboard"
        return domains.split(",").some(domain => domain.trim() === name)
    }
    Component.onCompleted: {
        niri.setEnabled(selected("niri") && CompositorService.isNiri)
        workspaces.setEnabled(selected("niri") && CompositorService.isNiri)
        network.setEnabled(selected("network"))
        brightness.setEnabled(selected("brightness"))
    }
    Component.onDestruction: {
        niri.setEnabled(false)
        workspaces.setEnabled(false)
        network.setEnabled(false)
        brightness.setEnabled(false)
        if (keyboard) keyboard.setEnabled(false)
    }
}
