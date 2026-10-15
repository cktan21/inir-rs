//@ pragma ShellId inir-bluetooth-component-test
import QtQuick
import Quickshell

// Compile actual shell components without creating their windows/singletons.
// This catches import/type/property errors without unlocking or restarting the
// running shell, opening another Bluetooth observer, or changing configuration.
ShellRoot {
    Component.onCompleted: {
        const files = [
            "../../../modules/bluetooth/BluetoothManager.qml",
            "../../../services/network/BluetoothDiscoveryView.qml",
            "../../../services/network/BluetoothStatus.qml",
            "../../../services/native/BluetoothProvider.qml",
            "../../../modules/pill/LinkBt.qml",
            "../../../modules/iris/components/IrisBluetoothList.qml",
            "../../../modules/sidebarRight/bluetoothDevices/BluetoothDialog.qml",
            "../../../modules/waffle/actionCenter/bluetooth/BluetoothControl.qml",
            "../../../modules/waffle/actionCenter/nightLight/NightLightControl.qml"
        ]
        for (const file of files) {
            const component = Qt.createComponent(Qt.resolvedUrl(file), Component.PreferSynchronous)
            if (component.status !== Component.Ready)
                console.error("BLUETOOTH_COMPILE_FAIL: " + file + " " + component.errorString())
            else console.log("BLUETOOTH_COMPILE_PASS: " + file)
            component.destroy()
        }
    }
    Timer { interval: 100; running: true; onTriggered: Qt.quit() }
}
