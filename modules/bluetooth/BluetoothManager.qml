pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import QtQuick.Dialogs
import Quickshell
import Quickshell.Bluetooth
import qs.services
import qs.modules.common
import qs.modules.common.widgets
import qs.modules.common.functions

Scope {
    id: root
    property int tab: 0
    readonly property var adapters: Bluetooth.adapters.values
    property int adapterIndex: 0
    readonly property var adapter: root.adapters[root.adapterIndex] ?? Bluetooth.defaultAdapter
    readonly property var prompt: BluetoothStatus.prompt
    property string inputPromptId: ""
    property var sendDevice: null
    property bool _live: true
    readonly property var diagnostics: ({ tab: root.tab, active: discovery.active,
        owner: discovery.owner, scanning: discovery.scanning, devices: discovery.devices.length,
        adapter: root.adapter?.adapterId ?? "", enabled: root.adapter?.enabled ?? false,
        mapped: !!manager.niriWindow, pid: Quickshell.processId })
    onDiagnosticsChanged: if (_live) BluetoothStatus.managerDetails = diagnostics
    Component.onDestruction: { _live = false; BluetoothStatus.managerDetails = ({}) }

    function deviceAction(action: string, device): void {
        BluetoothStatus.execute(action, BluetoothStatus.deviceArguments(device))
    }
    function bytes(value): string {
        if (value >= 1048576) return (value / 1048576).toFixed(1) + " MiB"
        if (value >= 1024) return (value / 1024).toFixed(1) + " KiB"
        return value + " B"
    }
    onPromptChanged: {
        if ((root.prompt?.id ?? "") !== root.inputPromptId) {
            root.inputPromptId = root.prompt?.id ?? ""
            promptInput.text = ""
        }
    }

    BluetoothDiscoveryView {
        id: discovery
        active: BluetoothStatus.managerOpen && root.tab === 0
        adapter: root.adapter
    }

    FloatingWindow {
        id: manager
        title: Translation.tr("Bluetooth") + " · iNiR"
        visible: BluetoothStatus.managerOpen
        implicitWidth: 760
        implicitHeight: 650
        color: Appearance.colors.colSurfaceContainer
        minimumSize: Qt.size(560, 420)
        onClosed: BluetoothStatus.managerOpen = false
        readonly property var niriWindow: (NiriService.windows ?? []).find(w => w.title === manager.title && w.pid === Quickshell.processId) ?? null
        property bool focusedOnce: false
        onNiriWindowChanged: {
            if (visible && niriWindow && !focusedOnce) { focusedOnce = true; NiriService.focusWindow(niriWindow.id) }
        }
        onVisibleChanged: if (!visible) focusedOnce = false

        ColumnLayout {
            anchors.fill: parent
            anchors.margins: 20
            spacing: 14

            RowLayout {
                Layout.fillWidth: true
                StyledText { text: Translation.tr("Bluetooth"); font.pixelSize: 26; Layout.fillWidth: true }
                ComboBox {
                    model: root.adapters
                    textRole: "name"
                    currentIndex: root.adapterIndex
                    onActivated: index => root.adapterIndex = index
                }
                Switch {
                    text: Translation.tr("On")
                    checked: root.adapter?.enabled ?? false
                    onToggled: {
                        if (BluetoothStatus.controller?.ready)
                            BluetoothStatus.execute("power", { adapter: root.adapter?.adapterId ?? "", enabled: checked })
                        else if (root.adapter) root.adapter.enabled = checked
                    }
                }
            }
            RowLayout {
                Layout.fillWidth: true
                Switch {
                    text: Translation.tr("Integrated manager")
                    checked: BluetoothStatus.integrated
                    onToggled: Config.setNestedValue("bluetooth.managerMode", checked ? "integrated" : "external")
                }
                StyledText {
                    Layout.fillWidth: true
                    wrapMode: Text.Wrap
                    text: BluetoothStatus.integrated
                        ? Translation.tr("Pairing prompts and transfers are handled by the shell.")
                        : Translation.tr("Blueman still handles pairing prompts. Scan timeouts are active.")
                    font.pixelSize: 12
                }
            }
            StyledText {
                Layout.fillWidth: true
                visible: text.length > 0
                color: Appearance.colors.colError
                wrapMode: Text.Wrap
                text: BluetoothStatus.lastError || BluetoothStatus.controlState.error || ""
            }
            TabBar {
                Layout.fillWidth: true
                currentIndex: root.tab
                onCurrentIndexChanged: root.tab = currentIndex
                TabButton { text: Translation.tr("Devices") }
                TabButton { text: Translation.tr("Transfers") }
                TabButton { text: Translation.tr("Internet") }
            }
            StackLayout {
                Layout.fillWidth: true
                Layout.fillHeight: true
                currentIndex: root.tab

                ColumnLayout {
                    RowLayout {
                        Layout.fillWidth: true
                        DialogButton {
                            buttonText: discovery.scanning ? Translation.tr("Stop search") : Translation.tr("Search again")
                            enabled: root.adapter?.enabled ?? false
                            onClicked: discovery.scanning ? discovery.stopScan() : discovery.rescan()
                        }
                        StyledText {
                            Layout.fillWidth: true
                            text: discovery.scanning ? Translation.tr("Searching for 25 seconds…")
                                : discovery.externalDiscovery ? Translation.tr("Another app is searching for devices.") : Translation.tr("Search stopped")
                        }
                    }
                    ListView {
                        Layout.fillWidth: true
                        Layout.fillHeight: true
                        clip: true
                        spacing: 8
                        model: ScriptModel { values: discovery.devices }
                        delegate: Rectangle {
                            id: deviceRow
                            required property var modelData
                            readonly property var action: BluetoothStatus.deviceAction(modelData)
                            readonly property string presentation: BluetoothStatus.devicePresentation(modelData)
                            width: ListView.view.width
                            implicitHeight: actions.implicitHeight + 24
                            radius: 12
                            color: Appearance.colors.colSurfaceContainerHigh
                            RowLayout {
                                id: actions
                                anchors.fill: parent
                                anchors.margins: 12
                                ColumnLayout {
                                    Layout.fillWidth: true
                                    StyledText { Layout.fillWidth: true; text: deviceRow.modelData.name || deviceRow.modelData.address; elide: Text.ElideRight }
                                    StyledText {
                                        Layout.fillWidth: true
                                        font.pixelSize: 12
                                        color: deviceRow.presentation === "error" ? Appearance.colors.colError : Appearance.colors.colOnSurface
                                        wrapMode: Text.Wrap
                                        text: deviceRow.presentation === "error"
                                            ? BluetoothStatus.deviceActionLabel(deviceRow.action) + ": " + deviceRow.action.error
                                            : deviceRow.presentation === "pending" ? BluetoothStatus.deviceActionLabel(deviceRow.action)
                                            : deviceRow.modelData.connected ? Translation.tr("Connected")
                                            + (deviceRow.modelData.batteryAvailable ? " · " + Math.round(deviceRow.modelData.battery * 100) + "%" : "")
                                            : deviceRow.modelData.paired ? Translation.tr("Paired") : deviceRow.modelData.address
                                    }
                                }
                                DialogButton {
                                    enabled: deviceRow.presentation !== "pending"
                                    buttonText: deviceRow.presentation === "pending" ? BluetoothStatus.deviceActionLabel(deviceRow.action)
                                        : deviceRow.modelData.connected ? Translation.tr("Disconnect") : deviceRow.modelData.paired ? Translation.tr("Connect") : Translation.tr("Pair")
                                    onClicked: {
                                        if (deviceRow.modelData.connected) BluetoothStatus.disconnectDevice(deviceRow.modelData)
                                        else if (deviceRow.modelData.paired) BluetoothStatus.connectDevice(deviceRow.modelData)
                                        else if (BluetoothStatus.integrated) root.deviceAction("pair", deviceRow.modelData)
                                        else BluetoothStatus.pairDevice(deviceRow.modelData.address)
                                    }
                                }
                                DialogButton {
                                    buttonText: "⋯"
                                    enabled: deviceRow.presentation !== "pending"
                                    implicitWidth: 38
                                    implicitHeight: 38
                                    onClicked: deviceMenu.popup()
                                    Menu {
                                        id: deviceMenu
                                        MenuItem { text: Translation.tr("Send files…"); enabled: BluetoothStatus.controlState.obexReady ?? false; onTriggered: { root.sendDevice = deviceRow.modelData; files.open() } }
                                        MenuItem { text: Translation.tr("Use internet connection"); enabled: BluetoothStatus.integrated && (BluetoothStatus.controlState.networkReady ?? false); onTriggered: root.deviceAction("connectTether", deviceRow.modelData) }
                                        MenuItem { text: deviceRow.modelData.trusted ? Translation.tr("Remove trust") : Translation.tr("Trust device"); onTriggered: BluetoothStatus.execute("trust", Object.assign(BluetoothStatus.deviceArguments(deviceRow.modelData), { trusted: !deviceRow.modelData.trusted })) }
                                        MenuItem { text: Translation.tr("Forget device"); enabled: deviceRow.modelData.paired; onTriggered: BluetoothStatus.forgetDevice(deviceRow.modelData) }
                                    }
                                }
                            }
                        }
                    }
                }

                ColumnLayout {
                    StyledText {
                        Layout.fillWidth: true
                        wrapMode: Text.Wrap
                        text: (BluetoothStatus.controlState.obexReady ?? false)
                            ? Translation.tr("Choose Send files from a device’s menu. Incoming files ask for confirmation.")
                            : Translation.tr("Enable the integrated manager to use file transfers. The OBEX service must be installed.")
                    }
                    ListView {
                        Layout.fillWidth: true
                        Layout.fillHeight: true
                        clip: true
                        spacing: 12
                        model: BluetoothStatus.transfers
                        delegate: ColumnLayout {
                            id: transferRow
                            required property string transferId
                            required property string name
                            required property string direction
                            required property string status
                            required property real size
                            required property real transferred
                            required property string filename
                            required property string error
                            width: ListView.view.width
                            RowLayout {
                                Layout.fillWidth: true
                                StyledText { Layout.fillWidth: true; text: transferRow.name; elide: Text.ElideRight }
                                DialogButton {
                                    buttonText: Translation.tr("Cancel")
                                    visible: ["queued", "active", "suspended"].includes(transferRow.status)
                                    onClicked: BluetoothStatus.execute("cancelTransfer", { transfer: transferRow.transferId })
                                }
                            }
                            ProgressBar { Layout.fillWidth: true; from: 0; to: Math.max(1, transferRow.size); value: transferRow.transferred }
                            StyledText {
                                Layout.fillWidth: true
                                wrapMode: Text.Wrap
                                font.pixelSize: 12
                                text: transferRow.direction + " · " + transferRow.status + " · " + root.bytes(transferRow.transferred)
                                    + " / " + root.bytes(transferRow.size) + (transferRow.error ? "\n" + transferRow.error : "")
                            }
                            StyledText { Layout.fillWidth: true; font.pixelSize: 12; wrapMode: Text.Wrap; text: transferRow.filename }
                        }
                    }
                }

                ColumnLayout {
                    StyledText {
                        Layout.fillWidth: true
                        wrapMode: Text.Wrap
                        text: Translation.tr("To use a phone’s internet, enable Bluetooth tethering on the phone, then choose Use internet connection from its device menu.")
                    }
                    DialogButton {
                        buttonText: Translation.tr("Share this laptop’s internet")
                        enabled: BluetoothStatus.integrated && (BluetoothStatus.controlState.networkReady ?? false) && (root.adapter?.enabled ?? false)
                        onClicked: BluetoothStatus.execute("startSharing", { adapter: root.adapter.adapterId })
                    }
                    StyledText {
                        Layout.fillWidth: true
                        wrapMode: Text.Wrap
                        text: Translation.tr("Internet sharing uses NetworkManager and requires dnsmasq. It stays active when this window closes.")
                        font.pixelSize: 12
                    }
                    Switch {
                        text: Translation.tr("Allow new devices to find this laptop for 3 minutes")
                        checked: root.adapter?.discoverable ?? false
                        enabled: root.adapter?.enabled ?? false
                        onToggled: BluetoothStatus.execute("discoverable", { adapter: root.adapter.adapterId, enabled: checked })
                    }
                    Repeater {
                        model: BluetoothStatus.controlState.networks ?? []
                        RowLayout {
                            id: connectionRow
                            required property var modelData
                            Layout.fillWidth: true
                            StyledText { Layout.fillWidth: true; text: connectionRow.modelData.name + " · " + (connectionRow.modelData.state === 2 ? Translation.tr("Connected") : Translation.tr("Connecting")) }
                            DialogButton {
                                buttonText: Translation.tr("Disconnect")
                                onClicked: BluetoothStatus.execute("disconnectTether", { connection: connectionRow.modelData.id })
                            }
                        }
                    }
                    Item { Layout.fillHeight: true }
                }
            }
        }
    }

    FileDialog {
        id: files
        title: Translation.tr("Send files over Bluetooth")
        fileMode: FileDialog.OpenFiles
        onAccepted: {
            const paths = Array.from(selectedFiles).map(url => decodeURIComponent(FileUtils.trimFileProtocol(String(url))))
            BluetoothStatus.execute("sendFiles", Object.assign(BluetoothStatus.deviceArguments(root.sendDevice), { files: paths }))
            root.tab = 1
        }
    }

    FloatingWindow {
        id: promptWindow
        title: Translation.tr("Bluetooth request")
        visible: root.prompt !== null
        implicitWidth: 440
        implicitHeight: 310
        color: Appearance.colors.colSurfaceContainer
        onClosed: if (root.prompt) BluetoothStatus.respond(root.prompt.id, false)
        readonly property var niriWindow: (NiriService.windows ?? []).find(w => w.title === promptWindow.title && w.pid === Quickshell.processId) ?? null
        property bool focusedOnce: false
        onNiriWindowChanged: {
            if (visible && niriWindow && !focusedOnce) { focusedOnce = true; NiriService.focusWindow(niriWindow.id) }
        }
        onVisibleChanged: if (!visible) focusedOnce = false
        ColumnLayout {
            anchors.fill: parent
            anchors.margins: 22
            spacing: 14
            StyledText { Layout.fillWidth: true; font.pixelSize: 21; text: root.prompt?.kind === "incomingFile" ? Translation.tr("Receive file?") : Translation.tr("Bluetooth pairing") }
            StyledText { Layout.fillWidth: true; wrapMode: Text.Wrap; text: root.prompt?.name || root.prompt?.device || "" }
            StyledText {
                Layout.fillWidth: true
                wrapMode: Text.Wrap
                text: root.prompt?.kind === "incomingFile" ? root.bytes(root.prompt?.size ?? 0)
                    : root.prompt?.kind === "confirmation" ? Translation.tr("Do these numbers match?")
                    : ["displayPin", "displayPasskey"].includes(root.prompt?.kind ?? "") ? Translation.tr("Enter this code on the other device.")
                    : root.prompt?.kind === "service" ? Translation.tr("Allow this device to use the requested service?")
                    : root.prompt?.kind === "authorization" ? Translation.tr("Allow this device to pair?") : Translation.tr("Enter the code requested by your device.")
            }
            StyledText {
                Layout.fillWidth: true
                font.pixelSize: root.prompt?.kind === "service" ? 12 : 28
                wrapMode: Text.Wrap
                text: root.prompt?.value ?? ""
            }
            StyledText {
                visible: root.prompt?.kind === "displayPasskey"
                font.pixelSize: 12
                text: (root.prompt?.entered ?? 0) + " / 6 " + Translation.tr("digits entered")
            }
            MaterialTextField {
                id: promptInput
                Layout.fillWidth: true
                visible: ["pin", "passkey"].includes(root.prompt?.kind ?? "")
                enableSettingsSearch: false
                maximumLength: root.prompt?.kind === "passkey" ? 6 : 16
                placeholderText: root.prompt?.kind === "passkey" ? Translation.tr("Passkey") : Translation.tr("PIN")
                onVisibleChanged: if (visible) forceActiveFocus()
                onAccepted: if (root.prompt) BluetoothStatus.respond(root.prompt.id, true, text)
            }
            Item { Layout.fillHeight: true }
            RowLayout {
                Layout.alignment: Qt.AlignRight
                DialogButton { buttonText: Translation.tr("Reject"); onClicked: if (root.prompt) BluetoothStatus.respond(root.prompt.id, false) }
                DialogButton {
                    buttonText: Translation.tr("Accept")
                    visible: !["displayPin", "displayPasskey"].includes(root.prompt?.kind ?? "")
                    enabled: !promptInput.visible || (promptInput.text.length > 0 && (root.prompt?.kind !== "passkey" || /^\d{1,6}$/.test(promptInput.text)))
                    onClicked: if (root.prompt) BluetoothStatus.respond(root.prompt.id, true, promptInput.text)
                }
            }
        }
    }
}
