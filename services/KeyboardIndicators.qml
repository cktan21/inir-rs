pragma Singleton
pragma ComponentBehavior: Bound

import QtQuick
import Quickshell
import Quickshell.Io
import qs.modules.common
import qs.services
import qs.services.deferred

Singleton {
    id: root

    property list<string> capsLockPaths: []
    property list<string> numLockPaths: []
    property var _capsLockStates: ({})
    property var _numLockStates: ({})
    property string _knownLayoutName: ""
    property string _lockSource: "unknown"
    property bool _destroying: false
    property bool _initialized: false
    property bool _numLockReliable: false
    property var _lastRawNumLockState: null
    property string lockStateDaemonPath: Quickshell.shellPath("scripts/daemon/keyboard_lock_state_daemon.py")

    property bool capsLock: false
    property bool numLock: false
    property bool ready: false
    property string popupKind: ""
    property bool popupActive: false
    property string popupText: ""
    property int popupSequence: 0

    readonly property bool showPopup: Config.options?.keyboardIndicators?.showPopup ?? true
    readonly property bool showPanel: Config.options?.keyboardIndicators?.showPanel ?? true
    readonly property bool showLayoutPopup: root.showPopup && (Config.options?.keyboardIndicators?.popup?.layout ?? true)
    readonly property bool showCapsPopup: root.showPopup && (Config.options?.keyboardIndicators?.popup?.caps ?? true)
    readonly property bool showNumPopup: root.showPopup && (Config.options?.keyboardIndicators?.popup?.num ?? false)
    readonly property bool showLayoutPanel: root.showPanel && (Config.options?.keyboardIndicators?.panel?.layout ?? true)
    readonly property bool showCapsPanel: root.showPanel && (Config.options?.keyboardIndicators?.panel?.caps ?? true)
    readonly property bool showNumPanel: root.showPanel && (Config.options?.keyboardIndicators?.panel?.num ?? false)
    readonly property bool hasMultipleLayouts: (HyprlandXkb.layoutCodes?.length ?? 0) > 1
    readonly property string currentLayoutName: HyprlandXkb.currentLayoutName ?? ""
    readonly property string currentLayoutCode: HyprlandXkb.currentLayoutCode ?? ""
    readonly property string capsMaterialIcon: "keyboard_capslock"
    readonly property string numMaterialIcon: "dialpad"
    readonly property string capsFluentIcon: "key"
    readonly property string numFluentIcon: "keyboard-dock"
    readonly property string popupMaterialIcon: root.popupKind === "layout" ? "language"
        : root.popupKind === "caps" ? root.capsMaterialIcon
        : root.numMaterialIcon
    readonly property string popupFluentIcon: root.popupKind === "layout" ? "keyboard"
        : root.popupKind === "caps" ? root.capsFluentIcon
        : root.numFluentIcon
    readonly property string currentLayoutCodeMultiline: root.abbreviateLayoutCode(root.currentLayoutCode, "\n").toUpperCase()
    readonly property string currentLayoutCodeInline: root.abbreviateLayoutCode(root.currentLayoutCode, " ").toUpperCase()
    readonly property bool usingEvdev: root._lockSource === "evdev" || root._lockSource === "native"

    // Native caps/num lock worker (Rust evdev). Used in place of the Python daemon
    // when the native backend is active; QML mode keeps the daemon + sysfs fallback.
    readonly property var backendKeyboard: NativeBackend.keyboard
    readonly property bool nativeKeyboardReady: root.lockIndicatorsWanted
        && NativeBackend.typedBridge && (backendKeyboard?.ready ?? false)
    readonly property bool nativeKeyboardPending: root.lockIndicatorsWanted
        && NativeBackend.requested && NativeBackend.typedBridge
        && (NativeBackend.loading || (root.backendKeyboard !== null
            && !root.nativeKeyboardReady && root.backendKeyboard.error.length === 0))
    readonly property bool sysfsActive: root.lockIndicatorsWanted
        && root._lockSource === "sysfs" && !root.nativeKeyboardPending && !root.nativeKeyboardReady
    // A lock source only runs when some caps/num indicator or popup is enabled.
    // Layout indicators come from HyprlandXkb and never need it.
    readonly property bool lockIndicatorsWanted: Config.ready
        && (root.showCapsPopup || root.showNumPopup || root.showCapsPanel || root.showNumPanel)
    readonly property bool layoutVisible: root.showLayoutPanel && root.hasMultipleLayouts && root.currentLayoutCode.length > 0
    readonly property bool capsLockVisible: root.showCapsPanel && root.capsLock
    readonly property bool numLockVisible: root.showNumPanel && root.numLock
    readonly property bool hasPanelIndicators: root.layoutVisible || root.capsLockVisible || root.numLockVisible

    function backendDiagnostics(): var {
        return { owner: !root.lockIndicatorsWanted ? "off"
                : root.nativeKeyboardReady ? "rust"
                : root._lockSource === "evdev" ? "python" : root._lockSource,
            wanted: root.lockIndicatorsWanted, nativeReady: root.nativeKeyboardReady,
            pending: root.nativeKeyboardPending, source: root._lockSource,
            probeRunning: evdevProbeProc.running, monitorRunning: evdevMonitorProc.running,
            caps: root.capsLock, num: root.numLock, devices: root.backendKeyboard?.devices ?? 0 };
    }

    function _log(...args) {
        if (Quickshell.env("QS_DEBUG") === "1")
            console.log("[KeyboardIndicators]", ...args);
    }

    function abbreviateLayoutCode(fullCode, separator) {
        if (!fullCode || !fullCode.length)
            return "";

        return fullCode.split(":").map(layout => {
            const baseLayout = layout.split("-")[0];
            return baseLayout.slice(0, 3);
        }).join(separator);
    }

    function refreshLedPaths() {
        if (!root.sysfsActive)
            return;
        if (!ledDiscoveryProc.running)
            ledDiscoveryProc.running = true;
    }

    function _setCapsLockValue(nextValue, allowPopup) {
        const previousValue = root.capsLock;
        root.capsLock = nextValue;
        if (allowPopup && previousValue !== nextValue)
            root._emitLockPopup("caps", nextValue);
    }

    function _setNumLockValue(nextValue, allowPopup) {
        const previousValue = root.numLock;
        root.numLock = nextValue;
        if (allowPopup && previousValue !== nextValue)
            root._emitLockPopup("num", nextValue);
    }

    function _setEvdevState(nextCapsValue, nextNumValue, allowPopup) {
        root._lockSource = "evdev";
        root._setCapsLockValue(nextCapsValue, allowPopup);
        root._setNumLockRawValue(nextNumValue, allowPopup);
    }

    function _handleEvdevOutput(rawLine) {
        if (!root._pythonAllowed()) return;
        const trimmed = String(rawLine).trim();
        if (!trimmed.length)
            return;

        try {
            const payload = JSON.parse(trimmed);
            if (payload.type !== "state")
                return;

            root._setEvdevState(Boolean(payload.caps), Boolean(payload.num), true);
        } catch (error) {
            root._log("Failed to parse evdev output", trimmed, error);
        }
    }

    function _enableSysfsFallback() {
        if (!root._pythonAllowed())
            return;

        root._lockSource = "sysfs";
        root.refreshLedPaths();
    }

    function _setPopup(kind, active, text) {
        if (!root.ready)
            return;

        if (kind === "layout" && !root.showLayoutPopup)
            return;

        if (kind === "caps" && !root.showCapsPopup)
            return;

        if (kind === "num" && !root.showNumPopup)
            return;

        root.popupKind = kind;
        root.popupActive = active;
        root.popupText = text;
        root.popupSequence += 1;
    }

    function _emitLockPopup(kind, active) {
        if (kind === "caps") {
            root._setPopup(kind, active, active ? Translation.tr("Caps Lock on") : Translation.tr("Caps Lock off"));
            return;
        }

        root._setPopup(kind, active, active ? Translation.tr("Num Lock on") : Translation.tr("Num Lock off"));
    }

    function _emitLayoutPopup() {
        if (!root.hasMultipleLayouts || !root.currentLayoutName.length)
            return;

        root._setPopup("layout", true, root.currentLayoutName);
    }

    function _hasKnownState(paths, states) {
        return paths.some(path => states[path] !== null && states[path] !== undefined);
    }

    function _setLockPaths(kind, paths) {
        if (!root.sysfsActive)
            return;

        const uniquePaths = [...new Set(paths)].sort();
        const previousStates = kind === "caps" ? root._capsLockStates : root._numLockStates;
        const nextStates = {};

        for (const path of uniquePaths)
            nextStates[path] = previousStates.hasOwnProperty(path) ? previousStates[path] : null;

        if (kind === "caps") {
            root.capsLockPaths = uniquePaths;
            root._capsLockStates = nextStates;
            if (!uniquePaths.length)
                root.capsLock = false;
            root._recomputeLockState(kind, false);
            return;
        }

        root.numLockPaths = uniquePaths;
        root._numLockStates = nextStates;
        if (!uniquePaths.length) {
            root._numLockReliable = false;
            root._lastRawNumLockState = null;
            root.numLock = false;
        }
        root._recomputeLockState(kind, false);
    }

    function _setLockState(kind, path, rawValue) {
        if (!root.sysfsActive)
            return;

        const nextValue = Number(String(rawValue).trim()) > 0;

        if (kind === "caps") {
            root._capsLockStates = Object.assign({}, root._capsLockStates, { [path]: nextValue });
            root._recomputeLockState(kind, true);
            return;
        }

        root._numLockStates = Object.assign({}, root._numLockStates, { [path]: nextValue });
        root._recomputeLockState(kind, true);
    }

    function _clearLockState(kind, path) {
        if (!root.sysfsActive)
            return;

        if (kind === "caps") {
            root._capsLockStates = Object.assign({}, root._capsLockStates, { [path]: null });
            root._recomputeLockState(kind, false);
            return;
        }

        root._numLockStates = Object.assign({}, root._numLockStates, { [path]: null });
        root._recomputeLockState(kind, false);
    }

    function _setNumLockRawValue(nextValue, allowPopup) {
        const previousVisibleValue = root._numLockReliable ? root.numLock : root._lastRawNumLockState;

        if (root._lastRawNumLockState === null) {
            root._lastRawNumLockState = nextValue;
            root._numLockReliable = !nextValue;
        } else if (nextValue !== root._lastRawNumLockState) {
            root._lastRawNumLockState = nextValue;
            root._numLockReliable = true;
        }

        root._setNumLockValue(root._numLockReliable ? nextValue : false, false);
        if (root._numLockReliable && allowPopup && previousVisibleValue !== null && previousVisibleValue !== root.numLock)
            root._emitLockPopup("num", root.numLock);
    }

    function _recomputeLockState(kind, allowPopup) {
        const paths = kind === "caps" ? root.capsLockPaths : root.numLockPaths;
        const states = kind === "caps" ? root._capsLockStates : root._numLockStates;

        if (paths.length > 0 && !root._hasKnownState(paths, states))
            return;

        const nextValue = paths.some(path => states[path] === true);

        if (kind === "caps") {
            root._setCapsLockValue(nextValue, allowPopup);
            return;
        }

        if (!paths.length) {
            root._numLockReliable = false;
            root._lastRawNumLockState = null;
            root.numLock = false;
            return;
        }

        root._setNumLockRawValue(nextValue, allowPopup);
    }

    Process {
        id: evdevProbeProc
        running: false
        command: ["/usr/bin/python3", root.lockStateDaemonPath, "--once"]

        stdout: SplitParser {
            onRead: line => root._handleEvdevOutput(line)
        }

        stderr: SplitParser {
            onRead: line => root._log("evdev probe", line)
        }

        onExited: (exitCode, exitStatus) => {
            // A stopped probe may deliver onExited after demand changed or the
            // native owner became ready. It must never restart a second owner.
            if (!root._pythonAllowed()) return;
            if (exitCode === 0) {
                if (!evdevMonitorProc.running)
                    evdevMonitorProc.running = true;
                return;
            }

            root._log("evdev probe exited", exitCode, exitStatus);
            root._enableSysfsFallback();
        }
    }

    Timer {
        id: evdevRestartTimer
        interval: 1000
        running: false
        repeat: false
        onTriggered: {
            if (root._pythonAllowed() && !evdevMonitorProc.running)
                evdevMonitorProc.running = true;
        }
    }

    Process {
        id: evdevMonitorProc
        running: false
        command: ["/usr/bin/python3", "-u", root.lockStateDaemonPath]

        stdout: SplitParser {
            onRead: line => root._handleEvdevOutput(line)
        }

        stderr: SplitParser {
            onRead: line => root._log("evdev monitor", line)
        }

        onExited: (exitCode, exitStatus) => {
            if (!root._pythonAllowed())
                return;

            root._log("evdev monitor exited", exitCode, exitStatus);
            root._enableSysfsFallback();
            // A device/permission failure uses sysfs until demand or ownership
            // changes; do not respawn a failing interpreter every second.
            if (exitCode === 0) evdevRestartTimer.restart();
        }
    }

    Process {
        id: ledDiscoveryProc
        running: false
        command: ["/usr/bin/bash", "-lc", "printf 'capslock\\n'; for f in /sys/class/leds/*::capslock/brightness; do [ -e \"$f\" ] && printf '%s\\n' \"$f\"; done; printf 'numlock\\n'; for f in /sys/class/leds/*::numlock/brightness; do [ -e \"$f\" ] && printf '%s\\n' \"$f\"; done"]

        stdout: StdioCollector {
            id: ledCollector

            onStreamFinished: {
                let section = "";
                const nextCapsPaths = [];
                const nextNumPaths = [];

                for (const rawLine of ledCollector.text.split("\n")) {
                    const line = rawLine.trim();
                    if (!line.length)
                        continue;
                    if (line === "capslock" || line === "numlock") {
                        section = line;
                        continue;
                    }
                    if (section === "capslock")
                        nextCapsPaths.push(line);
                    else if (section === "numlock")
                        nextNumPaths.push(line);
                }

                root._setLockPaths("caps", nextCapsPaths);
                root._setLockPaths("num", nextNumPaths);
            }
        }
    }

    Timer {
        id: readyTimer
        interval: 1500
        running: true
        repeat: false
        onTriggered: {
            root.ready = true;
            root._knownLayoutName = root.currentLayoutName;
        }
    }

    Timer {
        interval: 8000
        running: root.sysfsActive
        repeat: true
        onTriggered: root.refreshLedPaths()
    }

    Instantiator {
        model: root.sysfsActive ? root.capsLockPaths : []

        FileView {
            required property string modelData
            path: modelData
            watchChanges: true
            onFileChanged: reload()
            onLoaded: root._setLockState("caps", modelData, text())
            onLoadFailed: root._clearLockState("caps", modelData)
        }
    }

    Instantiator {
        model: root.sysfsActive ? root.numLockPaths : []

        FileView {
            required property string modelData
            path: modelData
            watchChanges: true
            onFileChanged: reload()
            onLoaded: root._setLockState("num", modelData, text())
            onLoadFailed: root._clearLockState("num", modelData)
        }
    }

    Connections {
        target: CompositorService.isHyprland || CompositorService.isNiri ? HyprlandXkb : null
        function onCurrentLayoutNameChanged() {
            if (!root.currentLayoutName.length)
                return;
            if (!root._knownLayoutName.length) {
                root._knownLayoutName = root.currentLayoutName;
                return;
            }
            if (root._knownLayoutName === root.currentLayoutName)
                return;

            root._knownLayoutName = root.currentLayoutName;
            root._emitLayoutPopup();
        }
    }

    // Chooses the lock-state source: nothing when no caps/num indicator is enabled,
    // the native Rust worker when the backend is active, else the Python daemon.
    function _pythonAllowed(): bool {
        return root._initialized && !root._destroying && root.lockIndicatorsWanted
            && !root.nativeKeyboardReady && !root.nativeKeyboardPending;
    }

    function _stopPython(): void {
        evdevRestartTimer.stop();
        evdevProbeProc.running = false;
        evdevMonitorProc.running = false;
    }

    function _setNativeState(allowPopup): void {
        root._lockSource = "native";
        root._setCapsLockValue(root.backendKeyboard.caps, allowPopup);
        root._setNumLockRawValue(root.backendKeyboard.num, allowPopup);
    }

    function _applyLockSource() {
        if (!root._initialized || root._destroying)
            return;

        if (!root.lockIndicatorsWanted) {
            if (root.backendKeyboard)
                root.backendKeyboard.setEnabled(false);
            root._stopPython();
            ledDiscoveryProc.running = false;
            root._lockSource = "disabled";
            root.capsLockPaths = [];
            root.numLockPaths = [];
            root._capsLockStates = ({});
            root._numLockStates = ({});
            root.capsLock = false;
            root.numLock = false;
            root._numLockReliable = false;
            root._lastRawNumLockState = null;
            return;
        }

        const preferNative = NativeBackend.typedBridge && root.backendKeyboard !== null;
        if (preferNative) {
            root.backendKeyboard.setEnabled(true);
            if (root.nativeKeyboardReady) {
                root._stopPython();
                ledDiscoveryProc.running = false;
                root._setNativeState(false);
                return;
            }
        }
        if (root.nativeKeyboardPending) {
            root._stopPython();
            ledDiscoveryProc.running = false;
            root._lockSource = "pending";
            return;
        }
        if (["native", "pending", "disabled"].includes(root._lockSource))
            root._lockSource = "unknown";
        if (root._lockSource === "unknown" && !evdevProbeProc.running && !evdevMonitorProc.running)
            evdevProbeProc.running = true;
    }

    onLockIndicatorsWantedChanged: root._applyLockSource()
    onBackendKeyboardChanged: root._applyLockSource()
    onNativeKeyboardPendingChanged: root._applyLockSource()
    onNativeKeyboardReadyChanged: root._applyLockSource()

    Connections {
        target: root.backendKeyboard
        enabled: root.backendKeyboard !== null
        function onErrorChanged() { root._applyLockSource() }
        function onStateUpdated() {
            if (root.nativeKeyboardReady)
                root._setNativeState(true);
        }
    }

    Component.onCompleted: {
        root._knownLayoutName = root.currentLayoutName;
        root._initialized = true;
        root._applyLockSource();
    }

    Component.onDestruction: {
        root._destroying = true;
        if (root.backendKeyboard)
            root.backendKeyboard.setEnabled(false);
    }
}
