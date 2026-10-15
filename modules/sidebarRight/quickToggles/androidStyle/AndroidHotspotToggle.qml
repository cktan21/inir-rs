pragma ComponentBehavior: Bound

import qs
import qs.services
import qs.modules.common
import qs.modules.common.widgets
import QtQuick

AndroidQuickToggleButton {
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
