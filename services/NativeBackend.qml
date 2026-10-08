pragma Singleton
pragma ComponentBehavior: Bound

import QtQuick
import Quickshell
import Quickshell.Io

Singleton {
    id: root
    readonly property bool requested: Quickshell.env("INIR_RUST_BACKEND") === "1"
    readonly property bool loading: provider.active && (provider.status === Loader.Null || provider.status === Loader.Loading)
    readonly property bool available: provider.status === Loader.Ready
    readonly property var systemInfo: available ? provider.item.systemInfo : null
    readonly property var config: available ? provider.item.config : null
    readonly property var desktop: available ? provider.item.desktop : null
    property int _sequence: 0
    property var _callbacks: ({})

    function sendCommand(command, callback = null): bool {
        if (!root.desktop) {
            if (callback) callback(false, "Rust backend is unavailable")
            return false
        }
        const id = String(++root._sequence)
        if (callback) root._callbacks[id] = callback
        return root.desktop.execute(id, JSON.stringify(command))
    }

    Connections {
        target: root.desktop
        function onCommandFinished(id, success, error) {
            const callback = root._callbacks[id]
            delete root._callbacks[id]
            if (callback) callback(success, error)
            else if (!success) console.warn("[NativeBackend] command failed:", error)
        }
    }

    // A file import isolates the optional native module from legacy installs.
    Loader {
        id: provider
        active: root.requested
        asynchronous: true
        source: "native/Provider.qml"
        onStatusChanged: {
            if (status === Loader.Error)
                console.warn("[NativeBackend] native module could not load; using QML services")
        }
    }

    IpcHandler {
        target: "backend"
        function services(): string {
            if (!root.available)
                return JSON.stringify({ backend: "qml", rustRequested: root.requested, nativeAvailable: false })
            return root.systemInfo.diagnostics()
        }
    }
}
