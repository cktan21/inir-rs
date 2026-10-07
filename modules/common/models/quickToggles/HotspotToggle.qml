pragma ComponentBehavior: Bound

import qs
import QtQuick
import qs.services
import qs.modules.common
import qs.modules.common.widgets
import Quickshell

/**
 * QuickToggleModel for the Wi-Fi hotspot. State, NetworkManager calls and
 * failure reporting all live in the Hotspot service; this is presentation only.
 */
QuickToggleModel {
    id: root

    name: Translation.tr("Hotspot")
    icon: "wifi_tethering"
    toggled: Hotspot.active
    available: true
    hasMenu: true
    hasStatusText: true
    statusText: root.toggled ? Hotspot.ssid : Translation.tr("Off")

    tooltipText: Translation.tr("Personal Wi-Fi Hotspot")

    mainAction: () => Hotspot.toggle()
}
