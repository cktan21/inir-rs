pragma ComponentBehavior: Bound
import QtQml

QtObject {
    id: root
    signal triggered()
    readonly property bool pending: frame.running
    function schedule() { if (!frame.running) frame.start() }
    function cancel() { frame.stop() }
    // Native structural notifications arrive before the facade's Qt.callLater
    // collection publication. A frame-sized bucket includes both and does not
    // push its deadline out on each additional notification.
    property Timer frame: Timer {
        interval: 16
        repeat: false
        onTriggered: root.triggered()
    }
}
