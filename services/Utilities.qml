pragma Singleton
pragma ComponentBehavior: Bound
import QtQuick
import Quickshell
import Quickshell.Io
import qs.modules.common

Singleton {
    id: root
    property int _sequence: 0
    property bool _wanted: false
    property var _requests: ({})
    readonly property var controller: provider.status === Loader.Ready ? (provider.item.controller ?? null) : null
    readonly property var keyboard: provider.status === Loader.Ready ? (provider.item.keyboard ?? null) : null
    readonly property bool loading: root._wanted && (provider.status === Loader.Null || provider.status === Loader.Loading)
    property bool _keyboardWanted: false
    function setKeyboardEnabled(enabled: bool): void {
        root._keyboardWanted = enabled
        if (enabled) root._wanted = true
        root.keyboard?.setEnabled(enabled)
    }
    property var _deviceOwners: ({})
    property string _deviceMode: ""
    readonly property bool deviceEventsEnabled: Object.keys(root._deviceOwners).length > 0
    readonly property bool deviceEventsReady: root._deviceMode === "native"
        ? (root.controller?.deviceEventsReady ?? false) : root._deviceMode === "cli" && deviceWatch.ready
    signal devicesChanged(string kind)

    function setDeviceEventsEnabled(owner: string, enabled: bool): void {
        const owners = Object.assign({}, root._deviceOwners)
        if (enabled) owners[owner] = true
        else delete owners[owner]
        root._deviceOwners = owners
    }
    onDeviceEventsEnabledChanged: {
        if (root.deviceEventsEnabled) root._wanted = true
        root._startDeviceEvents()
    }
    function _startDeviceEvents(): void {
        if (provider.status !== Loader.Ready && provider.status !== Loader.Error) return
        if (!root.deviceEventsEnabled) {
            root.controller?.setDeviceEventsEnabled(false)
            deviceWatch.running = false
            root._deviceMode = ""
        } else if (!root._deviceMode) {
            if (root.controller?.ready) {
                root._deviceMode = "native"
                root.controller.setDeviceEventsEnabled(true)
            } else {
                root._deviceMode = "cli"
                deviceWatch.running = true
            }
        }
    }

    // Only requested utilities load this provider. It is deliberately independent
    // of the experimental desktop-backend toggle and starts no polling services.
    Loader {
        id: provider
        active: root._wanted
        asynchronous: true
        source: "native/UtilityProvider.qml"
        onStatusChanged: if (status === Loader.Ready || status === Loader.Error) {
            for (const id of Object.keys(root._requests)) root._start(id)
            root._startDeviceEvents()
            root.keyboard?.setEnabled(root._keyboardWanted)
        }
    }
    function request(operation: string, parameters, callback, timeoutMs = 60000): string {
        if (Object.keys(root._requests).length >= 64) {
            Qt.callLater(() => callback(false, null, "Utility queue is full"))
            return ""
        }
        const id = "utility-" + (++root._sequence)
        const timer = deadlineComponent.createObject(root, { requestId: id, interval: Math.max(1, Math.min(180000, timeoutMs)) })
        root._requests[id] = { operation: operation, parameters: parameters, callback: callback, timer: timer, process: null, started: false }
        root._wanted = true
        root._start(id)
        return id
    }
    function _start(id: string): void {
        const request = root._requests[id]
        if (!request || request.started || provider.status === Loader.Loading || provider.status === Loader.Null) return
        request.started = true
        if (root.controller?.ready) {
            if (!root.controller.submit(id, request.operation, request.parameters))
                Qt.callLater(() => root._finish(id, false, null, "Utility request could not be queued"))
        } else {
            const process = processComponent.createObject(root, { requestId: id, operation: request.operation, parameters: request.parameters })
            request.process = process
            process.running = true
        }
    }
    function _finish(id: string, success: bool, result, error: string): void {
        const request = root._requests[id]
        if (!request) return
        delete root._requests[id]
        request.timer.stop()
        request.timer.destroy()
        if (request.process) { request.process.running = false; request.process.destroy() }
        request.callback(success, result, error)
    }
    function cancel(id: string): void {
        if (!root._requests[id]) return
        root.controller?.cancel(id)
        root._finish(id, false, null, "Utility request canceled")
    }
    Connections {
        target: root.controller
        function onFinished(id, success, result, error): void {
            root._finish(id, success, root.controller.jsValue(result), error)
        }
        function onDevicesChanged(kind): void { root.devicesChanged(kind) }
    }
    Process {
        id: deviceWatch
        property bool ready: false
        command: [Directories.utilityHelperPath, "devices", "watch"]
        stdout: SplitParser {
            onRead: line => {
                try {
                    const kind = JSON.parse(line).kind
                    if (kind === "ready") deviceWatch.ready = true
                    else if (kind === "usb" || kind === "block") root.devicesChanged(kind)
                } catch (error) { console.warn("Invalid device event from Rust helper") }
            }
        }
        onExited: ready = false
    }
    Component {
        id: deadlineComponent
        Timer {
            required property string requestId
            running: true
            onTriggered: { root.controller?.cancel(requestId); root._finish(requestId, false, null, "Utility request timed out") }
        }
    }
    Component {
        id: processComponent
        Process {
            id: command
            required property string requestId
            required property string operation
            required property var parameters
            command: [Directories.utilityHelperPath, "job", operation]
            stdinEnabled: true
            onStarted: { write(JSON.stringify(parameters)); stdinEnabled = false }
            stdout: StdioCollector { id: response }
            stderr: StdioCollector { id: diagnostics }
            onExited: (code, status) => {
                let envelope
                try { envelope = JSON.parse(response.text) } catch (error) {
                    root._finish(requestId, false, null, "Rust utility helper failed (exit " + code + ")")
                    return
                }
                root._finish(requestId, code === 0 && envelope.success === true, envelope.result, String(envelope.error ?? ""))
            }
        }
    }
}
