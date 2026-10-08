pragma Singleton
pragma ComponentBehavior: Bound

import QtQuick
import Quickshell
import Quickshell.Io
import qs.services

/**
 * Provides some system info: distro, username.
 */
Singleton {
    id: root
    readonly property var backendInfo: NativeBackend.systemInfo
    property string distroName: backendInfo ? backendInfo.distroName : "Unknown"
    property string distroId: backendInfo ? backendInfo.distroId : "unknown"
    property string distroIcon: backendInfo ? backendInfo.distroIcon : "linux-symbolic"
    // Seed identity from the process environment so consumers do not build
    // transient paths for the placeholder user while `id -un` is starting.
    // The asynchronous lookup below remains the authoritative refresh.
    property string username: backendInfo?.ready ? backendInfo.username : (Quickshell.env("USER") || "user")
    property string displayName: backendInfo ? backendInfo.displayName : ""
    // Static hostname. `/etc/hostname` is the portable source; the env var is a
    // seed for the frame before the file is read and is absent on most systems.
    property string hostname: backendInfo ? backendInfo.hostname : (Quickshell.env("HOSTNAME") || "")
    property string homeUrl: backendInfo ? backendInfo.homeUrl : ""
    property string documentationUrl: backendInfo ? backendInfo.documentationUrl : ""
    property string supportUrl: backendInfo ? backendInfo.supportUrl : ""
    property string bugReportUrl: backendInfo ? backendInfo.bugReportUrl : ""
    property string privacyPolicyUrl: backendInfo ? backendInfo.privacyPolicyUrl : ""
    property string logo: backendInfo ? backendInfo.logo : ""
    property string desktopEnvironment: backendInfo ? backendInfo.desktopEnvironment : String(Quickshell.env("XDG_CURRENT_DESKTOP") ?? "").trim()
    property string windowingSystem: backendInfo ? backendInfo.windowingSystem : (String(Quickshell.env("WAYLAND_DISPLAY") ?? "").trim().length > 0 ? "Wayland" : "X11")

    function refreshIdentity(): void {
        if (backendInfo) {
            backendInfo.refreshIdentity()
            return
        }
        if (NativeBackend.loading)
            return
        if (getUsername.running || getDisplayName.running)
            return
        getUsername.running = true
    }

    Timer {
        triggeredOnStart: true
        interval: 1
        running: !root.backendInfo && !NativeBackend.loading
        repeat: false
        onTriggered: {
            refreshIdentity()
            fileHostname.reload()
            const textHostname = fileHostname.text().trim()
            if (textHostname.length > 0) hostname = textHostname.split("\n")[0].trim()
            fileOsRelease.reload()
            const textOsRelease = fileOsRelease.text()

            // Extract the friendly name (PRETTY_NAME field, fallback to NAME)
            const prettyNameMatch = textOsRelease.match(/^PRETTY_NAME="(.+?)"/m)
            const nameMatch = textOsRelease.match(/^NAME="(.+?)"/m)
            distroName = prettyNameMatch ? prettyNameMatch[1] : (nameMatch ? nameMatch[1].replace(/Linux/i, "").trim() : "Unknown")

            // Extract the ID
            const idMatch = textOsRelease.match(/^ID="?(.+?)"?$/m)
            distroId = idMatch ? idMatch[1] : "unknown"

            // Extract additional URLs and logo
            const homeUrlMatch = textOsRelease.match(/^HOME_URL="(.+?)"/m)
            homeUrl = homeUrlMatch ? homeUrlMatch[1] : ""
            const documentationUrlMatch = textOsRelease.match(/^DOCUMENTATION_URL="(.+?)"/m)
            documentationUrl = documentationUrlMatch ? documentationUrlMatch[1] : ""
            const supportUrlMatch = textOsRelease.match(/^SUPPORT_URL="(.+?)"/m)
            supportUrl = supportUrlMatch ? supportUrlMatch[1] : ""
            const bugReportUrlMatch = textOsRelease.match(/^BUG_REPORT_URL="(.+?)"/m)
            bugReportUrl = bugReportUrlMatch ? bugReportUrlMatch[1] : ""
            const privacyPolicyUrlMatch = textOsRelease.match(/^PRIVACY_POLICY_URL="(.+?)"/m)
            privacyPolicyUrl = privacyPolicyUrlMatch ? privacyPolicyUrlMatch[1] : ""
            const logoFieldMatch = textOsRelease.match(/^LOGO="?(.+?)"?$/m)
            logo = logoFieldMatch ? logoFieldMatch[1] : ""

            // Update the distroIcon property based on distroId
            switch (distroId) {
                case "arch": distroIcon = "arch-symbolic"; break;
                case "endeavouros": distroIcon = "endeavouros-symbolic"; break;
                case "cachyos": distroIcon = "cachyos-symbolic"; break;
                case "nixos": distroIcon = "nixos-symbolic"; break;
                case "fedora": distroIcon = "fedora-symbolic"; break;
                case "linuxmint":
                case "ubuntu":
                case "zorin":
                case "popos": distroIcon = "ubuntu-symbolic"; break;
                case "debian":
                case "raspbian":
                case "kali": distroIcon = "debian-symbolic"; break;
                case "funtoo":
                case "gentoo": distroIcon = "gentoo-symbolic"; break;
                default: distroIcon = "linux-symbolic"; break;
            }
            if (textOsRelease.toLowerCase().includes("nyarch")) {
                distroIcon = "nyarch-symbolic"
            }

            if (logo.trim().length === 0) {
                logo = distroIcon
            }

        }
    }


    Process {
        id: getUsername
        command: ["/usr/bin/id", "-un"]
        stdout: StdioCollector {
            id: usernameCollector
            onStreamFinished: {
                const name = usernameCollector.text.trim() || Quickshell.env("USER") || root.username
                root.username = name
                getDisplayName.command = ["/usr/bin/getent", "passwd", name]
                getDisplayName.running = true
            }
        }
    }

    Process {
        id: getDisplayName
        running: false
        command: ["/usr/bin/getent", "passwd", root.username]
        stdout: StdioCollector {
            id: displayNameCollector
            onStreamFinished: {
                const passwdLine = displayNameCollector.text.trim().split("\n")[0] ?? ""
                const fields = passwdLine.split(":")
                const gecosField = fields.length >= 5 ? fields[4] : ""
                const name = gecosField.split(",")[0].trim()
                root.displayName = name.length > 0 ? name : root.username
            }
        }
    }

    FileView {
        id: fileOsRelease
        path: "/etc/os-release"
    }

    FileView {
        id: fileHostname
        path: "/etc/hostname"
    }
}
