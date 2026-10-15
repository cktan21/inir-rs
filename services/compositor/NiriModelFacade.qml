pragma ComponentBehavior: Bound
import QtQuick
import QtQml.Models
import qs.services

// Role-backed, stable row views. Collection membership is published only after
// insert/remove/move; title/focus/geometry update the existing views in place.
Item {
    id: root
    required property var facade
    readonly property var niri: NativeBackend.niri
    readonly property var workspaceModel: NativeBackend.workspaces
    property bool windowsQueued: false
    property bool workspacesQueued: false
    property var knownWindowWorkspaces: ({})
    function queueWindows() {
        if (windowsQueued) return
        windowsQueued = true
        Qt.callLater(() => {
            windowsQueued = false
            const rows = [], nextWorkspaces = {}, affectedWorkspaces = new Set()
            for (let i = 0; i < windows.count; ++i) {
                const view = windows.objectAt(i)
                if (view) {
                    rows.push(view.record)
                    const id = String(view.windowId)
                    nextWorkspaces[id] = view.workspaceId
                    if (root.knownWindowWorkspaces[id] !== view.workspaceId)
                        affectedWorkspaces.add(view.workspaceId)
                }
            }
            for (const id in root.knownWindowWorkspaces)
                if (nextWorkspaces[id] === undefined)
                    affectedWorkspaces.add(root.knownWindowWorkspaces[id])
            root.knownWindowWorkspaces = nextWorkspaces
            facade.windows = rows
            syncFocus()
            facade.windowOrderChanged()
            // The fallback checks this policy after opens/closes as well as
            // workspace focus. Do not schedule it on unrelated title updates.
            for (const id of affectedWorkspaces)
                if (id >= 0) facade._scheduleSingleWindowCheck(id)
        })
    }
    function queueWorkspaces() {
        if (workspacesQueued) return
        workspacesQueued = true
        Qt.callLater(() => {
            workspacesQueued = false
            const rows = [], byId = {}
            for (let i = 0; i < workspaces.count; ++i) {
                const view = workspaces.objectAt(i)
                if (view) { rows.push(view.record); byId[view.workspaceId] = view.record }
            }
            facade.allWorkspaces = rows
            facade.workspaces = byId
            syncWorkspace()
            facade.windowOrderChanged()
        })
    }
    function syncFocus() {
        facade.activeWindow = facade.windows.find(w => w.id === niri.focusedWindowId) ?? null
    }
    function syncWorkspace() {
        const previous = facade.focusedWorkspaceId
        facade.focusedWorkspaceId = niri.focusedWorkspaceId >= 0 ? String(niri.focusedWorkspaceId) : ""
        facade.focusedWorkspaceIndex = niri.focusedWorkspaceIndex
        facade.currentOutput = niri.currentOutput
        facade.updateCurrentOutputWorkspaces()
        if (previous !== facade.focusedWorkspaceId && facade.focusedWorkspaceId)
            facade._scheduleSingleWindowCheck(Number(facade.focusedWorkspaceId))
    }
    function syncOutputs() {
        facade.outputs = niri.jsValue(niri.outputs) ?? ({})
        facade.updateDisplayScales()
    }
    function syncConfig() {
        if (niri.configGeneration > 0) facade.handleConfigLoaded({failed: niri.configFailed})
    }
    Connections {
        target: root.facade
        function onWindowListUpdateIntervalMsChanged() {
            root.niri.setUpdateInterval(root.facade.windowListUpdateIntervalMs)
        }
    }
    Connections {
        target: root.niri
        function onFocusedWindowIdChanged() { root.syncFocus() }
        function onFocusedWorkspaceIdChanged() { root.syncWorkspace() }
        function onFocusedWorkspaceIndexChanged() { root.syncWorkspace() }
        function onCurrentOutputChanged() { root.syncWorkspace() }
        function onOverviewOpenChanged() { root.facade.inOverview = root.niri.overviewOpen }
        function onOutputsChanged() { root.syncOutputs() }
        function onKeyboardLayoutsChanged() { root.facade.keyboardLayoutNames = root.niri.jsValue(root.niri.keyboardLayouts) }
        function onKeyboardLayoutIndexChanged() { root.facade.currentKeyboardLayoutIndex = root.niri.keyboardLayoutIndex }
        function onMruWindowIdsChanged() { root.facade.mruWindowIds = root.niri.jsValue(root.niri.mruWindowIds) }
        function onConfigGenerationChanged() { root.syncConfig() }
        function onRowsUpdated(roles) {
            root.facade.windowRolesChanged(roles & 0x7fffffff)
            if ((roles & 1621) !== 0) root.facade.windowOrderChanged()
            if ((roles & 128) !== 0) root.facade.windowUrgentChanged()
        }
        function onStructureChanged() { root.queueWindows() }
    }
    Connections {
        target: root.workspaceModel
        function onStructureChanged() { root.queueWorkspaces() }
        function onRowsUpdated(roles) {
            if ((roles & 138) !== 0) root.facade.windowOrderChanged()
            if ((roles & 128) !== 0) root.facade.windowUrgentChanged()
        }
    }
    Instantiator {
        id: windows
        model: root.niri
        onObjectAdded: root.queueWindows()
        onObjectRemoved: root.queueWindows()
        delegate: QtObject {
            id: windowView
            required property real windowId
            required property string title
            required property string appId
            required property int pid
            required property real workspaceId
            required property bool focused
            required property bool floating
            required property bool urgent
            required property real focusSerial
            required property int column
            required property int row
            required property real tileWidth
            required property real tileHeight
            required property int windowWidth
            required property int windowHeight
            required property real tilePosX
            required property real tilePosY
            required property bool tilePosValid
            required property real windowOffsetX
            required property real windowOffsetY
            required property string focusTimestamp
            required property bool titlePresent
            required property bool appIdPresent
            readonly property var layout: ({
                pos_in_scrolling_layout: column >= 0 ? [column,row] : null,
                tile_size: [tileWidth,tileHeight], window_size: [windowWidth,windowHeight],
                tile_pos_in_workspace_view: tilePosValid ? [tilePosX,tilePosY] : null,
                window_offset_in_tile: [windowOffsetX,windowOffsetY]
            })
            readonly property var timestamp: focusTimestamp ? ({ secs: Number(focusTimestamp.split(":")[0]), nanos: Number(focusTimestamp.split(":")[1]) }) : null
            readonly property var record: ({
                    get id() { return windowView.windowId },
                    get title() { return windowView.titlePresent ? windowView.title : null },
                    get app_id() { return windowView.appIdPresent ? windowView.appId : null },
                    get pid() { return windowView.pid < 0 ? null : windowView.pid },
                    get workspace_id() { return windowView.workspaceId < 0 ? null : windowView.workspaceId },
                    get is_focused() { return windowView.focused },
                    get is_floating() { return windowView.floating },
                    get is_urgent() { return windowView.urgent },
                    get layout() { return windowView.layout },
                    get focus_timestamp() { return windowView.timestamp }
            })
        }
    }
    Instantiator {
        id: workspaces
        model: root.workspaceModel
        onObjectAdded: root.queueWorkspaces()
        onObjectRemoved: root.queueWorkspaces()
        delegate: QtObject {
            id: workspaceView
            required property real workspaceId
            required property int idx
            required property string name
            required property string output
            required property bool active
            required property bool focused
            required property real activeWindowId
            required property bool urgent
            required property bool namePresent
            readonly property var record: ({
                    get id() { return workspaceView.workspaceId },
                    get idx() { return workspaceView.idx },
                    get name() { return workspaceView.namePresent ? workspaceView.name : null },
                    get output() { return workspaceView.output || null },
                    get is_active() { return workspaceView.active },
                    get is_focused() { return workspaceView.focused },
                    get active_window_id() { return workspaceView.activeWindowId < 0 ? null : workspaceView.activeWindowId },
                    get is_urgent() { return workspaceView.urgent }
            })
        }
    }
    Component.onCompleted: {
        niri.setUpdateInterval(facade.windowListUpdateIntervalMs)
        queueWindows()
        queueWorkspaces()
        syncOutputs()
        facade.inOverview = niri.overviewOpen
        facade.keyboardLayoutNames = niri.jsValue(niri.keyboardLayouts)
        facade.currentKeyboardLayoutIndex = niri.keyboardLayoutIndex
        facade.mruWindowIds = niri.jsValue(niri.mruWindowIds)
        syncConfig()
    }
}
