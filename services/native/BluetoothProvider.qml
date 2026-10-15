pragma ComponentBehavior: Bound
import QtQuick
import qs.services.native as Native
import qs.services
import qs.modules.common
import qs

Item {
    id: root
    readonly property var controller: Native.NativeBluetoothController
    readonly property var status: controller.jsValue(controller.state) ?? ({})
    property int retryDelay: 2000
    function sync(): void {
        controller.setEnabled(true, BluetoothStatus.integrated, BluetoothStatus.receiveDirectory, GlobalStates.screenLocked)
    }
    Component.onCompleted: sync()
    Component.onDestruction: controller.setEnabled(false, false, "", true)
    Connections {
        target: BluetoothStatus
        function onIntegratedChanged(): void { root.sync() }
        function onReceiveDirectoryChanged(): void { root.sync() }
    }
    Connections {
        target: root.controller
        function onReadyChanged(): void {
            if (root.controller.ready) root.retryDelay = 2000
        }
    }
    Timer {
        interval: root.retryDelay
        repeat: true
        running: !root.controller.ready
        onTriggered: {
            root.sync()
            root.retryDelay = Math.min(30000, root.retryDelay * 2)
        }
    }
}
