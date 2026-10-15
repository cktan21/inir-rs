import QtQuick

// Notify presentation consumers without seeking or changing playback. Starting
// after an inactive interval publishes immediately, then at the normal cadence.
Timer {
    id: root
    property var player: null
    property bool active: true
    property bool playing: false

    interval: 250
    repeat: true
    triggeredOnStart: true
    running: active && playing && player !== null
    onTriggered: root.player?.positionChanged()
    onPlayerChanged: if (running) root.player?.positionChanged()
}
