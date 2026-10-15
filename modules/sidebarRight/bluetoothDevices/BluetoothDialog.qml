import qs
import qs.services
import qs.modules.common
import qs.modules.common.widgets
import qs.modules.common.functions
import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import Qt5Compat.GraphicalEffects
import Quickshell.Io
import Quickshell.Bluetooth
import Quickshell
import Quickshell.Wayland

WindowDialog {
    id: root
    backgroundHeight: 450

    BluetoothDiscoveryView {
        id: discovery
        active: root.show && GlobalStates.sidebarRightOpen
    }

    WindowDialogTitle {
        text: Translation.tr("Bluetooth devices")
    }
    WindowDialogSeparator {
        visible: !discovery.scanning
    }
    StyledIndeterminateProgressBar {
        visible: discovery.scanning
        Layout.fillWidth: true
        Layout.topMargin: -8
        Layout.bottomMargin: -8
        Layout.leftMargin: -(Appearance.angelEverywhere ? Appearance.angel.roundingNormal : Appearance.inirEverywhere ? Appearance.inir.roundingNormal : Appearance.rounding.large)
        Layout.rightMargin: -(Appearance.angelEverywhere ? Appearance.angel.roundingNormal : Appearance.inirEverywhere ? Appearance.inir.roundingNormal : Appearance.rounding.large)
    }
    StyledListView {
        Layout.fillHeight: true
        Layout.fillWidth: true
        Layout.topMargin: -15
        Layout.bottomMargin: -16
        Layout.leftMargin: -(Appearance.angelEverywhere ? Appearance.angel.roundingNormal : Appearance.inirEverywhere ? Appearance.inir.roundingNormal : Appearance.rounding.large)
        Layout.rightMargin: -(Appearance.angelEverywhere ? Appearance.angel.roundingNormal : Appearance.inirEverywhere ? Appearance.inir.roundingNormal : Appearance.rounding.large)
        leftMargin: 8
        rightMargin: 8
        topMargin: 8
        bottomMargin: 8

        clip: true
        spacing: 4
        animateAppearance: false

        model: ScriptModel {
            values: discovery.devices
        }
        delegate: BluetoothDeviceItem {
            required property BluetoothDevice modelData
            device: modelData
            anchors {
                left: parent?.left
                right: parent?.right
                leftMargin: 8
                rightMargin: 8
            }
        }
    }
    WindowDialogSeparator {}
    WindowDialogButtonRow {
        DialogButton {
            buttonText: discovery.scanning ? Translation.tr("Stop search") : Translation.tr("Search again")
            enabled: Bluetooth.defaultAdapter?.enabled ?? false
            onClicked: discovery.scanning ? discovery.stopScan() : discovery.rescan()
        }
        DialogButton {
            buttonText: Translation.tr("Details")
            onClicked: {
                AppLauncher.launch("bluetooth")
                GlobalStates.sidebarRightOpen = false;
            }
        }

        Item {
            Layout.fillWidth: true
        }

        DialogButton {
            buttonText: Translation.tr("Done")
            onClicked: root.dismiss()
        }
    }
}
