pragma Singleton
pragma ComponentBehavior: Bound

import QtQuick
import Quickshell
import Quickshell.Io
import qs.modules.common
import qs.services

// Memory pressure monitoring for JSGCHeap accumulation (#164).
// Qt's V4 JS engine creates memfd mappings that persist as "(deleted)" after
// Loader teardown. This service monitors that accumulation and notifies the
// user when a restart would help reclaim memory.
Singleton {
    id: root

    // ── Config ────────────────────────────────────────────────────────────
    readonly property bool enabled: Config.options?.performance?.memoryMonitoring ?? true
    readonly property bool notifyEnabled: Config.options?.performance?.memoryWarningNotification ?? false
    readonly property int deletedMappingsThreshold: Config.options?.performance?.jsgcThreshold ?? 300
    readonly property int checkIntervalMs: 300000  // check every 5 min

    // ── State ─────────────────────────────────────────────────────────────
    property int currentDeletedMappings: 0
    property int currentTotalMappings: 0
    property bool notificationShown: false
    property bool userDismissed: false

    // ── Public API ────────────────────────────────────────────────────────
    function forceGc(): void {
        gc()
        _log("gc() forced")
    }

    function restart(): void {
        _log("user requested restart")
        Quickshell.execDetached([
            "/usr/bin/notify-send",
            "iNiR",
            Translation.tr("Restarting shell..."),
            "-a", "Shell",
            "--hint=int:transient:1",
        ])
        // Small delay so notification shows
        Qt.callLater(() => {
            Quickshell.execDetached(["systemctl", "--user", "restart", "inir.service"])
        })
    }

    function dismiss(): void {
        root.userDismissed = true
        root.notificationShown = false
        _log("user dismissed memory warning")
    }

    function reset(): void {
        root.userDismissed = false
        root.notificationShown = false
        _log("reset state")
    }

    function getStats(): string {
        return JSON.stringify({
            deletedMappings: root.currentDeletedMappings,
            totalMappings: root.currentTotalMappings,
            threshold: root.deletedMappingsThreshold,
            notificationShown: root.notificationShown,
            userDismissed: root.userDismissed,
            enabled: root.enabled,
            notifyEnabled: root.notifyEnabled
        })
    }

    // ── Internal ──────────────────────────────────────────────────────────
    function _log(...args): void {
        if (Quickshell.env("QS_DEBUG") === "1")
            console.log("[MemoryPressure]", ...args)
    }

    function _checkMemoryPressure(): void {
        if (!root.enabled || root._mapsJob) return
        root._mapsJob = Utilities.request("system.memoryMaps", { pid: Quickshell.processId }, (success, result, error) => {
            root._mapsJob = ""
            if (!success || !root.enabled) return
            root.currentDeletedMappings = result.deleted
            root.currentTotalMappings = result.total
            if (root.currentDeletedMappings >= root.deletedMappingsThreshold) root._notifyUser()
        })
    }

    function _notifyUser(): void {
        if (!root.notifyEnabled) { _log("threshold hit, notification disabled"); return }
        if (root.notificationShown || root.userDismissed) return
        
        root.notificationShown = true
        const mbEstimate = Math.round(root.currentDeletedMappings * 0.5)  // ~0.5 MB per mapping

        Quickshell.execDetached([
            "/usr/bin/notify-send",
            "iNiR",
            Translation.tr("Memory usage is high (~%1 MB accumulated). A restart would free it. Run: inir memory restart").arg(mbEstimate),
            "-u", "critical",
            "-a", "Shell",
        ])
        _log("notified user, estimated leak:", mbEstimate, "MB")
    }

    // ── Timers ────────────────────────────────────────────────────────────
    Timer {
        id: _checkTimer
        interval: root.checkIntervalMs
        repeat: true
        running: root.enabled
        onTriggered: root._checkMemoryPressure()
    }

    // Pass the shell's PID explicitly: a helper's /proc/self is the helper.
    property string _mapsJob: ""
    Component.onDestruction: if (root._mapsJob) Utilities.cancel(root._mapsJob)

    // ── IPC ───────────────────────────────────────────────────────────────
    IpcHandler {
        target: "memory"
        function collect(): string { root.forceGc(); return "gc() called" }
        function stats(): string { return root.getStats() }
        function restart(): string { root.restart(); return "restarting..." }
        function dismiss(): string { root.dismiss(); return "dismissed" }
        function reset(): string { root.reset(); return "reset" }
    }

    Component.onCompleted: {
        if (!root.enabled) return
        Qt.callLater(() => {
            _checkTimer.start()
            // Prime it once: the timer's first tick is a full interval away, so
            // without this the service reports 0 mappings for the first 5 minutes
            // after every restart.
            root._checkMemoryPressure()
        })
    }
}
