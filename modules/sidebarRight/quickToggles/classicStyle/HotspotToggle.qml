pragma ComponentBehavior: Bound

import qs
import qs.services
import qs.modules.common
import qs.modules.common.widgets
import qs.modules.sidebarRight.quickToggles
import QtQuick

QuickToggleButton {
    id: root
    property bool _holding: false
    function _syncLease(): void {
        if (root._holding === root.visible) return
        root._holding = root.visible
        if (root._holding) Hotspot.keepAlive()
        else Hotspot.releaseKeepAlive()
    }
    onVisibleChanged: root._syncLease()
    Component.onCompleted: root._syncLease()
    Component.onDestruction: if (root._holding) Hotspot.releaseKeepAlive()

    toggled: Hotspot.active
    buttonIcon: "wifi_tethering"

    onClicked: Hotspot.toggle()

    StyledToolTip {
        text: Translation.tr("Hotspot: %1").arg(Hotspot.ssid)
    }
}
