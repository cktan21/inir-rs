pragma Singleton
pragma ComponentBehavior: Bound

import QtQuick
import Quickshell
import Quickshell.Io
import qs
import qs.modules.common
import qs.services

/**
 * Wi-Fi hotspot (access point) state, owned here rather than in each toggle.
 *
 * NetworkManager carries the hotspot as an ordinary connection profile named
 * by `connectionName`. Enabling deletes any stale profile of that name before
 * creating a fresh one: NM otherwise keeps phantom-active profiles that it
 * reports as up while nothing is broadcasting.
 *
 * `active` is parsed from the active-connection list instead of trusting exit
 * codes, because `nmcli c show --active <name>` reports false positives against
 * those same stale profiles on some NM versions.
 *
 * Polling exists because NetworkManager state also changes outside this shell
 * (nmtui, GNOME's applet, `nmcli` by hand); it runs only while a surface that
 * shows the toggle is open.
 *
 * Config keys read: hotspot.ssid, hotspot.password, hotspot.band.
 */
Singleton {
    id: root

    readonly property string connectionName: "Hotspot"
    readonly property string ssid: Config.options?.hotspot?.ssid ?? "iNiR Hotspot"
    readonly property string password: Config.options?.hotspot?.password ?? "inirhotspot"
    readonly property string band: Config.options?.hotspot?.band ?? "bg"

    property bool active: false
    readonly property bool busy: startProc.running || stopProc.running

    // Emitted alongside the desktop notification so callers can surface their own
    // affordance; `operation` is "start" or "stop".
    signal failed(string operation, string message)

    function refresh(): void {
        checkProc.running = false
        checkProc.running = true
    }

    function enable(): void {
        // Positional params keep user-configured values out of the shell's parsing.
        startProc.exec(["/bin/sh", "-c",
            'nmcli connection delete id "$1" 2>/dev/null; exec nmcli dev wifi hotspot con-name "$1" ssid "$2" band "$3" password "$4"',
            "sh", root.connectionName, root.ssid, root.band, root.password])
    }

    function disable(): void {
        stopProc.running = true
    }

    function toggle(): void {
        if (root.active)
            root.disable()
        else
            root.enable()
    }

    function _notifyFailure(operation: string, message: string, fallback: string): void {
        const text = message.length > 0 ? message : fallback
        root.failed(operation, text)
        Quickshell.execDetached([
            "/usr/bin/notify-send",
            Translation.tr("Hotspot"),
            text,
            "-a", "iNiR"
        ])
    }

    Process {
        id: checkProc
        running: false
        command: ["nmcli", "-t", "-f", "NAME", "connection", "show", "--active"]

        stdout: StdioCollector {
            id: checkCollector
            onStreamFinished: {
                const out = checkCollector.text?.trim() ?? ""
                if (out.length === 0) {
                    root.active = false
                    return
                }
                root.active = out.split("\n").some(line => line.trim() === root.connectionName)
            }
        }

        onExited: (exitCode, exitStatus) => {
            if (exitCode !== 0)
                root.active = false
        }
    }

    Process {
        id: startProc
        running: false

        stderr: StdioCollector {
            id: startErrCollector
        }

        onExited: (exitCode, exitStatus) => {
            if (exitCode !== 0)
                root._notifyFailure("start", startErrCollector.text?.trim() ?? "",
                    Translation.tr("Failed to start hotspot. Ensure your Wi-Fi adapter supports AP mode."))
            root.refresh()
        }
    }

    Process {
        id: stopProc
        running: false
        command: ["nmcli", "connection", "down", root.connectionName]

        stderr: StdioCollector {
            id: stopErrCollector
        }

        onExited: (exitCode, exitStatus) => {
            if (exitCode !== 0)
                root._notifyFailure("stop", stopErrCollector.text?.trim() ?? "",
                    Translation.tr("Failed to stop hotspot."))
            root.refresh()
        }
    }

    Timer {
        interval: 5000
        repeat: true
        triggeredOnStart: true
        running: GlobalStates.sidebarRightOpen || GlobalStates.waffleActionCenterOpen
        onTriggered: root.refresh()
    }

    Component.onCompleted: root.refresh()
}
