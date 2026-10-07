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

    // ── Persistent shared-AP profile ──
    // The link pill keeps its own always-saved AP profile instead of the one-shot
    // `nmcli dev wifi hotspot` above: it lets the user edit the name and password
    // in place and reads both back out of NetworkManager, which needs a profile
    // that survives being brought down. Kept as a second profile rather than
    // merged into `connectionName` so existing saved APs keep working.
    readonly property string apConnectionName: "RicelinHotspot"
    property bool apActive: false
    property bool apBusy: false
    property string apSsid: "Ricelin"
    property string apPassword: ""

    /**
     * Brings the shared AP up with `ssid`/`password`, creating the profile on
     * first use and modifying it afterwards. Values are passed as positional
     * arguments, never spliced into the shell string, so an odd character can
     * neither break nor inject the command. Passwords shorter than the WPA2
     * minimum of 8 characters are rejected.
     */
    function applyAp(ssid: string, password: string, iface: string): void {
        if (root.apBusy || password.length < 8)
            return;
        root.apBusy = true;
        apApplyProc.command = ["sh", "-c",
            'c="$4"; '
            + 'if nmcli -t connection show "$c" >/dev/null 2>&1; then '
            +   'nmcli connection modify "$c" 802-11-wireless.ssid "$1" 802-11-wireless-security.key-mgmt wpa-psk 802-11-wireless-security.psk "$2"; '
            + 'else '
            +   'nmcli connection add type wifi ifname "$3" con-name "$c" autoconnect no 802-11-wireless.ssid "$1" 802-11-wireless.mode ap 802-11-wireless-security.key-mgmt wpa-psk 802-11-wireless-security.psk "$2" ipv4.method shared; '
            + 'fi; '
            + 'nmcli connection up "$c"',
            "sh", ssid, password, iface, root.apConnectionName];
        apApplyProc.running = true;
    }

    function stopAp(): void {
        if (root.apBusy)
            return;
        root.apBusy = true;
        apDownProc.running = true;
    }

    function refreshAp(): void {
        apStateProc.running = true;
        apReadProc.running = true;
    }

    /** Eight characters from an alphabet with no look-alikes, for a first-run AP password. */
    function generateApPassword(): string {
        const cs = "abcdefghijkmnpqrstuvwxyz23456789";
        let s = "";
        for (let i = 0; i < 8; i++)
            s += cs.charAt(Math.floor(Math.random() * cs.length));
        return s;
    }

    Process {
        id: apApplyProc
        onExited: {
            root.apBusy = false;
            root.refreshAp();
        }
    }

    Process {
        id: apDownProc
        command: ["nmcli", "connection", "down", root.apConnectionName]
        onExited: {
            root.apBusy = false;
            root.refreshAp();
        }
    }

    Process {
        id: apStateProc
        command: ["sh", "-c", "nmcli -t -f NAME connection show --active | grep -qx \"$1\" && echo on || echo off",
            "sh", root.apConnectionName]
        stdout: StdioCollector {
            id: apStateCollector
            onStreamFinished: root.apActive = apStateCollector.text.trim() === "on"
        }
    }

    Process {
        id: apReadProc
        command: ["nmcli", "-t", "-s", "-g", "802-11-wireless.ssid,802-11-wireless-security.psk",
            "connection", "show", root.apConnectionName]
        stdout: StdioCollector {
            id: apReadCollector
            onStreamFinished: {
                const lines = apReadCollector.text.split("\n");
                if (lines.length >= 1 && lines[0].length)
                    root.apSsid = lines[0];
                if (lines.length >= 2 && lines[1].length)
                    root.apPassword = lines[1];
            }
        }
    }
}
