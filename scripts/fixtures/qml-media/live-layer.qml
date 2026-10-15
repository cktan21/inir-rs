//@ pragma ShellId inir-media-activity-test
import QtQuick
import Quickshell
import Quickshell.Wayland
import "."

ShellRoot {
    id: tests
    property int step: 0
    property int passed: 0
    property bool failed: false
    property var owners: ({})
    property int serial: 0

    function check(ok, description) {
        if (!ok) {
            failed = true
            console.error("ACTIVITY_TEST_FAIL: " + description)
        } else {
            passed++
            console.log("ACTIVITY_TEST_PASS: " + description)
        }
    }
    function pulse() {
        const window = host.contentItem.Window.window
        check(window !== null, "native host window exists")
        if (window) window.afterAnimating()
    }
    function checkOwner() {
        const keys = Object.keys(owners)
        check(keys.filter(key => owners[key]).length <= 1, "at most one drawing owner")
    }

    Component {
        id: payload
        Item {
            property bool drawing: false
            property int serial: 0
            onDrawingChanged: if (serial > 0) tests.owners[serial] = drawing
            Component.onCompleted: { serial = ++tests.serial; tests.owners[serial] = drawing }
            Component.onDestruction: delete tests.owners[serial]
        }
    }

    PanelWindow {
        id: host
        color: "transparent"
        implicitWidth: 160
        implicitHeight: 160
        anchors { left: true; top: true }
        margins { left: 10; top: 40 }
        mask: Region {}
        exclusionMode: ExclusionMode.Ignore
        WlrLayershell.layer: WlrLayer.Bottom
        WlrLayershell.keyboardFocus: WlrKeyboardFocus.None
        WlrLayershell.namespace: "inir-media-activity-test"
        property bool liveCalm: true
        property int liveLayer: WlrLayer.Bottom
        property int liveEpoch: 0

        Item {
            id: ancestor
            width: 140
            height: 140
            LiveLayer {
                id: layer
                x: 10
                y: 10
                width: 40
                height: 40
                content: payload
                settleMs: 80
            }
        }
    }

    Timer {
        interval: 200
        repeat: true
        running: true
        onTriggered: {
            switch (tests.step++) {
            case 0:
                tests.check(!layer.trackingHostFrames && !layer.away, "inactive layer skips host frames")
                layer.live = true
                tests.pulse()
                tests.check(layer.trackingHostFrames, "eligible layer checks host frames")
                break
            case 1:
                tests.check(layer.detached, "settled layer detaches after its first presented frame")
                tests.checkOwner()
                ancestor.x = 20
                tests.pulse()
                tests.check(!layer.away && Math.abs(layer.restRect.x - 30) < 0.01, "ancestor movement returns content home immediately")
                break
            case 2:
                tests.check(layer.detached, "moved layer settles and detaches again")
                ancestor.opacity = 0.5
                tests.pulse()
                tests.check(!layer.away && Math.abs(layer.composedOpacity - 0.5) < 0.01, "ancestor fading is still checked")
                break
            case 3:
                ancestor.opacity = 1
                ancestor.clip = true
                layer.x = 120
                tests.pulse()
                tests.check(!layer.away, "clipped content stays home")
                ancestor.clip = false
                layer.x = 10
                ancestor.layer.enabled = true
                tests.pulse()
                tests.check(!layer.away, "ancestor effects keep content home")
                ancestor.layer.enabled = false
                break
            case 4:
                layer.allowed = false
                tests.check(!layer.trackingHostFrames && !layer.away, "disallowing detachment disables frame checks and returns home")
                layer.allowed = true
                tests.check(layer.trackingHostFrames, "allowing detachment restarts frame checks")
                break
            case 5:
                host.liveCalm = false
                tests.check(!layer.trackingHostFrames && !layer.away, "host movement gate disables frame checks and returns home")
                host.liveCalm = true
                break
            case 6:
                layer.visible = false
                tests.check(!layer.trackingHostFrames && !layer.away, "hidden layer disables frame checks and returns home")
                layer.visible = true
                break
            case 7:
                layer.width = 0
                tests.check(!layer.trackingHostFrames && !layer.away, "zero-sized layer disables frame checks")
                layer.width = 40
                break
            case 8:
                layer.content = null
                tests.check(!layer.trackingHostFrames && !layer.away, "empty content disables frame checks")
                layer.content = payload
                break
            case 9:
                host.liveEpoch++
                tests.check(!layer.away, "host recreation invalidates placement")
                break
            case 10:
                tests.check(layer.trackingHostFrames && layer.detached, "host recreation recovers without stale placement")
                tests.checkOwner()
                layer.live = false
                tests.check(!layer.trackingHostFrames && !layer.away, "stopping motion returns content home")
                break
            default:
                console.log("ACTIVITY_TEST_RESULT: " + JSON.stringify({passed: tests.passed, failed: tests.failed}))
                Qt.quit()
            }
        }
    }
}
