pragma ComponentBehavior: Bound
import QtQml

QtObject {
    id: root
    required property int windowId
    property var window: null
    property int workspaceNumber: 0
    property int workspaceSlot: 0
    property int indexInWorkspace: 0
    property int windowCount: 1
    property int maxCol: 1
    property int maxRow: 1

    // A stable ScriptModel row whose fields retain QML change notifications.
    // Mutating a plain JS row would preserve identity but leave bindings stale.
    readonly property var record: ({
        get id() { return root.windowId },
        get window() { return root.window },
        get workspaceNumber() { return root.workspaceNumber },
        get workspaceSlot() { return root.workspaceSlot },
        get indexInWorkspace() { return root.indexInWorkspace },
        get windowCount() { return root.windowCount },
        get maxCol() { return root.maxCol },
        get maxRow() { return root.maxRow }
    })
}
