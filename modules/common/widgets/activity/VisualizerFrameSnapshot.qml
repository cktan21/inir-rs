import QtQuick

// Hold both samples and their scale when motion is suspended. Another visible
// consumer may keep shared Cava acquisition running; it must not animate this one.
QtObject {
    id: root
    property var points: []
    property real normalizationCeiling: 100
    property bool updating: true
    property var shownPoints: []
    property real shownCeiling: 100

    property Binding pointsBinding: Binding {
        target: root
        property: "shownPoints"
        value: root.updating ? root.points : []
        when: root.updating
        restoreMode: Binding.RestoreNone
    }
    property Binding ceilingBinding: Binding {
        target: root
        property: "shownCeiling"
        value: root.updating ? root.normalizationCeiling : 100
        when: root.updating
        restoreMode: Binding.RestoreNone
    }

    // Copy only when freezing, not on every live audio frame. Retaining a Qt
    // sequence wrapper would otherwise keep reading its producer while frozen.
    onUpdatingChanged: if (!updating)
        shownPoints = Array.prototype.slice.call(shownPoints ?? [])
    // Prime a newly created inactive home/away copy once, before surface handoff.
    Component.onCompleted: if (!updating) {
        shownPoints = Array.prototype.slice.call(points ?? [])
        shownCeiling = normalizationCeiling
    }
}
