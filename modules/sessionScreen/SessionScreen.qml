pragma ComponentBehavior: Bound

import qs
import qs.modules.common
import QtQuick
import Quickshell

Scope {
    id: root

    readonly property bool useIris: (Config.options?.panelFamily ?? "ii") === "iris"

    Loader {
        active: root.useIris
        sourceComponent: irisSessionComponent
    }

    Component {
        id: irisSessionComponent
        IrisSessionScreen {}
    }

    Loader {
        active: !root.useIris
        sourceComponent: defaultSessionComponent
    }

    Component {
        id: defaultSessionComponent
        DefaultSessionScreen {}
    }
}
