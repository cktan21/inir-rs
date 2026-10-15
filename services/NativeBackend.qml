pragma Singleton
pragma ComponentBehavior: Bound

import QtQuick
import Quickshell
import Quickshell.Io
import qs.modules.common

Singleton {
    id: root
    readonly property bool requested: Quickshell.env("INIR_RUST_BACKEND") === "1"
    readonly property bool typedBridge: Quickshell.env("INIR_NATIVE_BRIDGE") === "1"
    readonly property bool loading: provider.active && (provider.status === Loader.Null || provider.status === Loader.Loading)
    readonly property bool available: provider.status === Loader.Ready
    readonly property var systemInfo: available ? provider.item.systemInfo : null
    readonly property var config: available ? provider.item.config : null
    readonly property var desktop: null // Deprecated; domains have independent QObjects.
    readonly property var niri: available ? provider.item.niri : null
    readonly property var workspaces: available ? provider.item.workspaces : null
    readonly property var network: available ? provider.item.network : null
    readonly property var brightness: available ? provider.item.brightness : null
    readonly property var power: available ? provider.item.power : null
    readonly property var keyboard: available ? provider.item.keyboard : null
    property int _sequence: 0
    property var _callbacks: ({})
    // Transient controls used only by the exclusive profiling harness.
    readonly property bool benchmarkEnabled: Quickshell.env("INIR_BENCHMARK") === "1"
    property bool benchmarkStopWorkspaceAnimations: false
    property bool benchmarkDisableWallpaperBlur: false
    property bool benchmarkHideMediaWidget: false
    property var _benchmarkPresentation: null

    function sendCommand(command, callback = null): bool {
        if (!root.available) {
            if (callback) callback(false, "Rust backend is unavailable")
            return false
        }
        const id = String(++root._sequence)
        if (callback) root._callbacks[id] = callback
        let sent = false
        switch (command.type) {
        case "wifiEnabled": sent = root.network.enableWifi(id, command.enabled); break
        case "wifiScan": sent = root.network.scan(id); break
        case "wifiDisconnect": sent = root.network.disconnectDevice(id, command.device); break
        case "wifiConnect": sent = root.network.connectAccessPoint(id, command.accessPoint, command.password ?? ""); break
        case "wifiConnectSsid": sent = root.network.connectSsid(id, command.ssid, command.password ?? ""); break
        case "profileForget": sent = root.network.forgetProfile(id, command.ssid); break
        case "profileSetPassword": sent = root.network.setProfilePassword(id, command.ssid, command.password); break
        case "profileSecret": sent = root.network.revealProfile(id, command.ssid); break
        case "brightness": sent = root.brightness.setBrightness(id, command.device, command.value); break
        case "powerProfile": sent = root.power.setProfile(id, command.profile); break
        case "niriRefreshOutputs": sent = root.niri.refreshOutputs(id); break
        case "niri": {
            const name = Object.keys(command.action)[0]
            const args = command.action[name]
            if (name === "FocusWindow") sent = root.niri.focusWindow(id, args.id)
            else if (name === "CloseWindow") sent = root.niri.closeWindow(id, args.id)
            else if (name === "FocusWorkspace") sent = root.niri.focusWorkspace(id, args.reference.Id ?? args.reference.Index, args.reference.Index !== undefined)
            else if (name === "MoveWindowToWorkspace") sent = root.niri.moveWindow(id, args.window_id, args.reference.Id ?? args.reference.Index, args.reference.Index !== undefined, args.focus ?? false)
            else sent = root.niri.action(id, name, args)
            break
        }
        }
        if (!sent) {
            delete root._callbacks[id]
            if (callback) callback(false, "Native domain is unavailable or command unsupported")
        }
        return sent
    }

    Connections {
        target: root.niri
        function onCommandFinished(id, success, error) {
            const callback = root._callbacks[id]
            delete root._callbacks[id]
            if (callback) callback(success, error)
            else if (!success) console.warn("[NativeBackend] command failed:", error)
        }
    }
    Connections {
        target: root.network
        function onCommandFinished(id, success, error) { root.finishCommand(id, success, error) }
        // The saved secret travels only on this signal, never through a command error.
        function onProfileRevealed(id, success, value, error) { root.finishCommand(id, success, error, value) }
    }
    Connections {
        target: root.brightness
        function onCommandFinished(id, success, error) { root.finishCommand(id, success, error) }
    }
    function finishCommand(id, success, error, value = "") {
        const callback = root._callbacks[id]
        delete root._callbacks[id]
        if (callback) callback(success, error, value)
        else if (!success) console.warn("[NativeBackend] command failed:", error)
    }

    // A file import isolates the optional native module from legacy installs.
    Loader {
        id: provider
        active: root.requested && root.typedBridge
        asynchronous: true
        source: "native/Provider.qml"
        onStatusChanged: {
            if (status === Loader.Error)
                console.warn("[NativeBackend] native module could not load; using QML services")
        }
    }

    IpcHandler {
        target: "backend"
        function prepareBenchmark(): string {
            // Per-process inhibition only; do not alter persistent user policy.
            Idle.inhibit = true
            return JSON.stringify({ idleInhibited: Idle.inhibit, gameMode: GameMode.active })
        }
        function benchmarkPresentation(variant: string): string {
            if (!root.benchmarkEnabled)
                throw new Error("Presentation controls require INIR_BENCHMARK=1")
            if (!["baseline", "no-animations", "no-workspace-animation", "no-effects", "no-wallpaper-blur", "no-media-widget", "no-blur-or-media-widget", "bar-hidden"].includes(variant))
                throw new Error("Unknown benchmark presentation: " + variant)
            if (root._benchmarkPresentation === null)
                root._benchmarkPresentation = { animations: Appearance.animationsEnabled,
                    effects: Appearance.effectsEnabled, bar: GlobalStates.barOpen }
            Appearance.animationsEnabled = variant === "no-animations" ? false : root._benchmarkPresentation.animations
            Appearance.effectsEnabled = variant === "no-effects" ? false : root._benchmarkPresentation.effects
            root.benchmarkStopWorkspaceAnimations = variant === "no-workspace-animation"
            root.benchmarkDisableWallpaperBlur = variant === "no-wallpaper-blur" || variant === "no-blur-or-media-widget"
            root.benchmarkHideMediaWidget = variant === "no-media-widget" || variant === "no-blur-or-media-widget"
            GlobalStates.barOpen = variant === "bar-hidden" ? false : root._benchmarkPresentation.bar
            return JSON.stringify({ variant: variant, animationsEnabled: Appearance.animationsEnabled,
                effectsEnabled: Appearance.effectsEnabled, barOpen: GlobalStates.barOpen,
                stopWorkspaceAnimations: root.benchmarkStopWorkspaceAnimations,
                disableWallpaperBlur: root.benchmarkDisableWallpaperBlur,
                hideMediaWidget: root.benchmarkHideMediaWidget,
                style: Appearance.globalStyle, motionProfile: Appearance.iiMotionProfile,
                mediaPlaying: MprisController.activePlayer?.isPlaying ?? false })
        }
        function ownership(): string {
            return JSON.stringify({ requested: root.requested, available: root.available,
                typedBridge: root.typedBridge,
                desktopActive: (root.niri?.active ?? false) || (root.network?.active ?? false) || (root.brightness?.active ?? false),
                idleInhibited: Idle.inhibit, gameMode: GameMode.active,
                selectedNativeDomains: Quickshell.env("INIR_NATIVE_DOMAINS"),
                ui: { overviewOpen: GlobalStates.overviewOpen, mediaControlsOpen: GlobalStates.mediaControlsOpen,
                    sidebarLeftOpen: GlobalStates.sidebarLeftOpen, sidebarRightOpen: GlobalStates.sidebarRightOpen,
                    controlPanelOpen: GlobalStates.controlPanelOpen, dashboardOpen: GlobalStates.dashboardOpen,
                    mediaPlaying: MprisController.activePlayer?.isPlaying ?? false },
                nativeFocus: root.available ? { workspaceId: root.niri.focusedWorkspaceId,
                    workspaceIndex: root.niri.focusedWorkspaceIndex, windowId: root.niri.focusedWindowId } : null,
                niri: NiriService.backendDiagnostics(), network: Network.backendDiagnostics(),
                brightness: Brightness.backendDiagnostics(),
                keyboard: KeyboardIndicators.backendDiagnostics(),
                nativePowerReady: false,
                models: root.available ? { niri: { count: root.niri.count, applyCalls: root.niri.applyCalls, applyTotalUs: root.niri.applyTotalUs, applyMaxUs: root.niri.applyMaxUs },
                    workspaces: { count: root.workspaces.count, applyCalls: root.workspaces.applyCalls, applyTotalUs: root.workspaces.applyTotalUs, applyMaxUs: root.workspaces.applyMaxUs } } : null })
        }
        function services(): string {
            if (!root.available)
                return JSON.stringify({ backend: "qml", rustRequested: root.requested, nativeAvailable: false })
            return JSON.stringify(root.systemInfo.diagnostics())
        }
        function windowTitle(windowId: string): string {
            return NiriService.windows.find(w => String(w.id) === windowId)?.title ?? ""
        }
    }
}
