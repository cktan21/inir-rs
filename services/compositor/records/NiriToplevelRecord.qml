pragma ComponentBehavior: Bound
import QtQml

QtObject {
    id: root
    required property var windowId
    property var window: null
    property var handle: null
    property var workspace: null
    signal activateRequested(var windowId)

    // Backing QML properties make fallback snapshots reactive too. Capturing a
    // JS window object directly would freeze focus/workspace at the first match.
    readonly property var record: {
        const result = {
            get appId() { return root.handle?.appId ?? "" },
            get title() { return root.handle?.title ?? "" },
            get activated() { return root.window?.is_focused ?? (root.workspace?.active_window_id === root.windowId) },
            get niriWindowId() { return root.windowId },
            get niriWorkspaceId() { return root.window?.workspace_id ?? null },
            _sourceKey: `niri:${root.windowId}`,
            get _sourceToplevel() { return root.handle },
            activate: function() { root.activateRequested(root.windowId) },
            close: function() { return root.handle?.close ? root.handle.close() : false }
        }
        if (root.handle) {
            for (const name in root.handle) {
                if (!(name in result)) Object.defineProperty(result, name, {
                    enumerable: true,
                    get: function() { return root.handle?.[name] }
                })
            }
        }
        return result
    }
}
