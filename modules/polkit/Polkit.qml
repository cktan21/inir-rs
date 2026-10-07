pragma ComponentBehavior: Bound

import qs
import qs.services
import qs.modules.common
import qs.modules.common.widgets
import qs.modules.common.functions
import QtQuick
import Quickshell
import Quickshell.Wayland

Scope {
    id: root

    readonly property bool useIris: (Config.options?.panelFamily ?? "ii") === "iris"

    // ── Iris presentation ──
    // FullscreenPolkitWindow targets the focused screen with its own
    // PolkitService.active guard; we just need to keep it alive.
    Loader {
        active: root.useIris
        sourceComponent: irisPolkitComponent
    }

    Component {
        id: irisPolkitComponent
        FullscreenPolkitWindow {
            contentComponent: Component {
                IrisPolkitContent {}
            }
        }
    }

    // ── ii / waffle presentation ──
    Loader {
        active: !root.useIris && PolkitService.available && PolkitService.active
        sourceComponent: iiPolkitComponent
    }

    Component {
        id: iiPolkitComponent
        Variants {
            model: Quickshell.screens
            delegate: PanelWindow {
                id: panelWindow
                required property var modelData
                screen: modelData

                anchors {
                    top: true
                    left: true
                    right: true
                    bottom: true
                }

                color: "transparent"
                WlrLayershell.namespace: "quickshell:polkit"
                WlrLayershell.keyboardFocus: WlrKeyboardFocus.OnDemand
                WlrLayershell.layer: WlrLayer.Overlay
                exclusionMode: ExclusionMode.Ignore

                PolkitContent {
                    anchors.fill: parent
                }
            }
        }
    }
}
