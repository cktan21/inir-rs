pragma ComponentBehavior: Bound

import qs
import qs.services
import qs.modules.common
import qs.modules.common.widgets
import QtQuick

AndroidQuickToggleButton {
    id: root

    name: Translation.tr("Hotspot")
    statusText: root.toggled ? Hotspot.ssid : Translation.tr("Off")

    toggled: Hotspot.active
    buttonIcon: "wifi_tethering"

    altAction: () => root.openMenu()
    mainAction: () => Hotspot.toggle()

    StyledToolTip {
        text: Translation.tr("Personal Wi-Fi Hotspot")
    }
}
