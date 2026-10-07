pragma ComponentBehavior: Bound

import qs
import qs.modules.common
import qs.modules.onScreenDisplay.waffle
import QtQuick
import Quickshell

Scope {
    id: root
    property var excludedScreenNames: []

    readonly property string panelFamily: Config.options?.panelFamily ?? "ii"
    readonly property bool useIris: root.panelFamily === "iris"
    readonly property bool useWaffle: root.panelFamily === "waffle"

    Loader {
        active: root.useIris
        sourceComponent: irisOsdComponent
    }

    Component {
        id: irisOsdComponent
        IrisOSD {}
    }

    Loader {
        active: root.useWaffle
        sourceComponent: waffleOsdComponent
    }

    Component {
        id: waffleOsdComponent
        WaffleOSD {}
    }

    Loader {
        active: !root.useIris && !root.useWaffle
        sourceComponent: defaultOsdComponent
    }

    Component {
        id: defaultOsdComponent
        DefaultOSD {
            excludedScreenNames: root.excludedScreenNames
        }
    }
}
