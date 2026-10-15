import QtQuick
import QtTest
import "../../modules/common/widgets/activity" as Activity

TestCase {
    id: tests
    name: "MediaActivity"
    when: windowShown

    QtObject {
        id: player
        property real position: 0
        property string playbackStatus: "Playing"
    }
    QtObject {
        id: otherPlayer
        property real position: 25
    }
    Activity.PlaybackPositionTicker {
        id: ticker
        player: player
        active: false
        interval: 30
    }
    SignalSpy { id: pulses; target: player; signalName: "positionChanged" }
    SignalSpy { id: otherPulses; target: otherPlayer; signalName: "positionChanged" }

    QtObject { id: producer; property list<real> points: [10, 20]; property real ceiling: 100 }
    Activity.VisualizerFrameSnapshot {
        id: snapshot
        points: producer.points
        normalizationCeiling: producer.ceiling
    }
    Component {
        id: frozenFrame
        Activity.VisualizerFrameSnapshot {
            updating: false
            points: [30, 40]
            normalizationCeiling: 75
        }
    }

    function init() {
        ticker.active = false
        ticker.playing = false
        ticker.player = player
        player.position = 0
        snapshot.updating = true
        producer.points = [10, 20]
        producer.ceiling = 100
        pulses.clear()
        otherPulses.clear()
    }

    function test_inactiveDoesNotTick() {
        ticker.playing = true
        wait(100)
        compare(pulses.count, 0)
        compare(player.playbackStatus, "Playing")
        compare(ticker.running, false)
    }
    function test_pausedDoesNotTick() {
        ticker.active = true
        wait(100)
        compare(pulses.count, 0)
    }
    function test_activeTicksWithoutSeeking() {
        ticker.playing = true
        ticker.active = true
        tryVerify(function() { return pulses.count >= 2 })
        compare(player.position, 0)
        compare(player.playbackStatus, "Playing")
    }
    function test_deactivationStopsTicks() {
        ticker.playing = true
        ticker.active = true
        tryVerify(function() { return pulses.count >= 2 })
        ticker.active = false
        pulses.clear()
        wait(100)
        compare(pulses.count, 0)
    }
    function test_resumePublishesImmediately() {
        ticker.playing = true
        ticker.active = true
        tryVerify(function() { return pulses.count >= 1 })
        ticker.active = false
        pulses.clear()
        ticker.active = true
        tryVerify(function() { return pulses.count >= 1 }, 20)
        compare(player.position, 0)
    }
    function test_playerReplacementPublishesToNewPlayer() {
        ticker.playing = true
        ticker.active = true
        ticker.player = otherPlayer
        tryVerify(function() { return otherPulses.count >= 1 }, 20)
        compare(otherPlayer.position, 25)
    }
    function test_nullPlayerStopsTicker() {
        ticker.playing = true
        ticker.active = true
        ticker.player = null
        compare(ticker.running, false)
        pulses.clear()
        wait(100)
        compare(pulses.count, 0)
    }
    function test_liveSnapshotFollowsSamplesAndScale() {
        producer.points = [30, 40]
        producer.ceiling = 75
        compare(snapshot.shownPoints, [30, 40])
        compare(snapshot.shownCeiling, 75)
    }
    function test_frozenSnapshotIgnoresSharedProducer() {
        snapshot.updating = false
        producer.points = [80, 90]
        producer.ceiling = 25
        compare(snapshot.shownPoints, [10, 20])
        compare(snapshot.shownCeiling, 100)
    }
    function test_snapshotCopiesQtSequences() {
        snapshot.updating = false
        producer.points[0] = 99
        compare(snapshot.shownPoints, [10, 20])
    }
    function test_snapshotResumesWithCurrentFrame() {
        snapshot.updating = false
        producer.points = [80, 90]
        producer.ceiling = 25
        snapshot.updating = true
        compare(snapshot.shownPoints, [80, 90])
        compare(snapshot.shownCeiling, 25)
    }
    function test_inactiveCopyPrimesOnceForHandoff() {
        const frame = frozenFrame.createObject(tests)
        compare(frame.shownPoints, [30, 40])
        compare(frame.shownCeiling, 75)
        frame.points = [90, 100]
        frame.normalizationCeiling = 100
        compare(frame.shownPoints, [30, 40])
        compare(frame.shownCeiling, 75)
        frame.destroy()
    }
}
