pragma ComponentBehavior: Bound
import QtQuick
import qs.services.native as Native

Item {
    readonly property var systemInfo: Native.SystemInfo
    readonly property var config: Native.ConfigService
    readonly property var desktop: Native.DesktopServices
    Component.onCompleted: Native.DesktopServices.setServicesActive(true)
    Component.onDestruction: Native.DesktopServices.setServicesActive(false)
}
