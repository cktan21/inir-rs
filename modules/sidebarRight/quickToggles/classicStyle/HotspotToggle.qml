pragma ComponentBehavior: Bound

import qs
import qs.services
import qs.modules.common
import qs.modules.common.widgets
import qs.modules.sidebarRight.quickToggles
import QtQuick

QuickToggleButton {
    id: root

    toggled: Hotspot.active
    buttonIcon: "wifi_tethering"

    onClicked: Hotspot.toggle()

    StyledToolTip {
        text: Translation.tr("Hotspot: %1").arg(Hotspot.ssid)
    }
}
