import QtQuick
import QtTest
import "../../../services/compositor/records" as NiriViews
import "../../../modules/overview/helpers" as OverviewViews

Item {
    id: root
    property var taskbarRows: []
    property var overviewRows: []
    readonly property bool activated: taskbarRows.length ? taskbarRows[0].activated : false
    readonly property var workspaceId: taskbarRows.length ? taskbarRows[0].niriWorkspaceId : null
    readonly property string forwarded: taskbarRows.length ? taskbarRows[0].customProperty : ""
    readonly property int windowCount: overviewRows.length ? overviewRows[0].windowCount : 0
    readonly property string windowTitle: overviewRows.length ? overviewRows[0].window.title : ""
    property int rebuilds: 0

    Component {
        id: handleComponent
        QtObject {
            property string appId: "editor"
            property string title: "title"
            property string customProperty: "before"
            property int closes: 0
            function close() { closes++; return true }
        }
    }
    Component { id: toplevelComponent; NiriViews.NiriToplevelRecord {} }
    Component { id: overviewComponent; OverviewViews.OverviewWindowRecord {} }
    OverviewViews.OverviewRebuildQueue { id: queue; onTriggered: root.rebuilds++ }

    TestCase {
        name: "NiriLiveRecords"
        when: windowShown
        function init() {
            root.taskbarRows = []
            root.overviewRows = []
            root.rebuilds = 0
            queue.cancel()
        }
        function test_fallbackSnapshotReplacementKeepsRecordIdentityAndBindings() {
            const handle = createTemporaryObject(handleComponent, root)
            const view = createTemporaryObject(toplevelComponent, root, {
                windowId: 1, handle: handle,
                window: { id: 1, is_focused: false, workspace_id: 2 }
            })
            root.taskbarRows = [view.record]
            const record = root.taskbarRows[0]
            compare(root.activated, false)
            compare(root.workspaceId, 2)
            view.window = { id: 1, is_focused: true, workspace_id: 3 }
            compare(view.record, record)
            compare(root.taskbarRows[0], record)
            compare(root.activated, true)
            compare(root.workspaceId, 3)
            handle.customProperty = "after"
            compare(root.forwarded, "after")
            verify(record.close())
            compare(handle.closes, 1)
        }
        function test_overviewMetadataUpdatesWithoutCollectionPublication() {
            const view = createTemporaryObject(overviewComponent, root, {
                windowId: 7, window: { title: "before" }, windowCount: 1
            })
            root.overviewRows = [view.record]
            const record = root.overviewRows[0]
            view.windowCount = 3
            view.window = { title: "after" }
            compare(view.record, record)
            compare(root.overviewRows[0], record)
            compare(root.windowCount, 3)
            compare(root.windowTitle, "after")
        }
        function test_structuralSignalsAndDeferredCollectionPublishCoalesce() {
            queue.schedule()
            queue.schedule()
            Qt.callLater(() => { queue.schedule(); queue.schedule() })
            tryCompare(root, "rebuilds", 1)
            wait(25)
            compare(root.rebuilds, 1)
            queue.schedule()
            tryCompare(root, "rebuilds", 2)
            queue.schedule()
            queue.cancel()
            wait(25)
            compare(root.rebuilds, 2)
        }
    }
}
