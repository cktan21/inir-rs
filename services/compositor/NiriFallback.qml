pragma ComponentBehavior: Bound

import QtQuick
import Quickshell.Io
import qs.services

// Loaded only while the optional native Niri domain is unavailable.
Item {
    id: root
    required property var facade
    readonly property bool actionReady: requestSocket.connected
    property int eventsRead: 0

    function diagnostics(): var {
        return { loaded: true, eventSocketConnected: eventStreamSocket.socketConnected,
            actionSocketConnected: requestSocket.socketConnected, eventsRead: root.eventsRead,
            outputsProcessRunning: fetchOutputsProcess.running, windowsTimerRunning: windowsUpdateTimer.running }
    }

    function send(request) {
        if (!requestSocket.connected) return false
        requestSocket.send(request)
        return true
    }

    DankSocket {
        id: eventStreamSocket
        path: facade.socketPath
        connected: CompositorService.isNiri

        onConnectionStateChanged: {
            if (connected) {
                send('"EventStream"')
                fetchOutputs()
                facade.refreshOverviewHotCorners()
            }
        }

        parser: SplitParser {
            onRead: line => {
                try {
                    const event = JSON.parse(line)
                    root.eventsRead++
                    handleNiriEvent(event)
                } catch (e) {
                    console.warn("NiriService: Failed to parse event:", line, e)
                }
            }
        }
    }

    DankSocket {
        id: requestSocket
        path: facade.socketPath
        connected: CompositorService.isNiri
    }

    Process {
        id: fetchOutputsProcess
        command: ["niri", "msg", "-j", "outputs"]

        stdout: StdioCollector {
            id: fetchOutputsCollector
            onStreamFinished: {
                try {
                    const outputsData = JSON.parse(fetchOutputsCollector.text)
                    facade.outputs = outputsData
                    console.info("NiriService: Loaded", Object.keys(outputsData).length, "outputs")
                    facade.updateDisplayScales()
                    if (facade.windows.length > 0) {
                        facade.windows = sortWindowsByLayout(facade.windows)
                    }
                    facade.windowOrderChanged()
                } catch (e) {
                    console.warn("NiriService: Failed to parse outputs:", e)
                }
            }
        }
    }

    // Debounce timer for fetchOutputs
    Timer {
        id: fetchOutputsDebounce
        interval: 200
        onTriggered: {
            if (CompositorService.isNiri)
                fetchOutputsProcess.running = true
        }
    }

    function fetchOutputs() {
        if (!CompositorService.isNiri)
            return
        fetchOutputsDebounce.restart()
    }

    function sortWindowsByLayout(windowList) {
        const enriched = windowList.map(w => {
            const ws = facade.workspaces[w.workspace_id]
            if (!ws) {
                return {
                    window: w,
                    outputX: 999999,
                    outputY: 999999,
                    wsIdx: 999999,
                    col: 999999,
                    row: 999999
                }
            }

            const outputInfo = facade.outputs[ws.output]
            const outputX = (outputInfo && outputInfo.logical) ? outputInfo.logical.x : 999999
            const outputY = (outputInfo && outputInfo.logical) ? outputInfo.logical.y : 999999

            const pos = w.layout?.pos_in_scrolling_layout
            const col = (pos && pos.length >= 2) ? pos[0] : 999999
            const row = (pos && pos.length >= 2) ? pos[1] : 999999

            return {
                window: w,
                outputX: outputX,
                outputY: outputY,
                wsIdx: ws.idx,
                col: col,
                row: row
            }
        })

        enriched.sort((a, b) => {
            if (a.outputX !== b.outputX) return a.outputX - b.outputX
            if (a.outputY !== b.outputY) return a.outputY - b.outputY
            if (a.wsIdx !== b.wsIdx) return a.wsIdx - b.wsIdx
            if (a.col !== b.col) return a.col - b.col
            if (a.row !== b.row) return a.row - b.row
            return a.window.id - b.window.id
        })

        return enriched.map(e => e.window)
    }

    function handleNiriEvent(event) {
        const eventType = Object.keys(event)[0]

        // During GameMode, skip non-essential events to reduce CPU usage.
        // WindowsChanged and WindowLayoutsChanged MUST remain — they carry the
        // window size updates that GameMode needs to detect fullscreen exit.
        if (GameMode.active) {
            const criticalEvents = [
                'WindowFocusChanged',
                'WindowClosed',
                'WindowOpenedOrChanged',
                'WindowsChanged',
                'WindowLayoutsChanged',
                'WorkspaceActivated',
                'WorkspaceActiveWindowChanged',
                'WorkspacesChanged',
                'OutputsChanged',
                'ConfigLoaded',
                'OverviewOpenedOrClosed',
                'KeyboardLayoutsChanged',
                'KeyboardLayoutSwitched',
            ]
            if (!criticalEvents.includes(eventType)) {
                return
            }
        }

        switch (eventType) {
        case 'WorkspacesChanged':
            handleWorkspacesChanged(event.WorkspacesChanged)
            break
        case 'WorkspaceActivated':
            handleWorkspaceActivated(event.WorkspaceActivated)
            break
        case 'WorkspaceActiveWindowChanged':
            handleWorkspaceActiveWindowChanged(event.WorkspaceActiveWindowChanged)
            break
        case 'WindowFocusChanged':
            handleWindowFocusChanged(event.WindowFocusChanged)
            break
        case 'WindowsChanged':
            handleWindowsChanged(event.WindowsChanged)
            break
        case 'WindowClosed':
            handleWindowClosed(event.WindowClosed)
            break
        case 'WindowOpenedOrChanged':
            handleWindowOpenedOrChanged(event.WindowOpenedOrChanged)
            break
        case 'WindowLayoutsChanged':
            handleWindowLayoutsChanged(event.WindowLayoutsChanged)
            break
        case 'OutputsChanged':
            handleOutputsChanged(event.OutputsChanged)
            break
        case 'OverviewOpenedOrClosed':
            handleOverviewChanged(event.OverviewOpenedOrClosed)
            break
        case 'ConfigLoaded':
            facade.handleConfigLoaded(event.ConfigLoaded)
            break
        case 'KeyboardLayoutsChanged':
            handleKeyboardLayoutsChanged(event.KeyboardLayoutsChanged)
            break
        case 'KeyboardLayoutSwitched':
            handleKeyboardLayoutSwitched(event.KeyboardLayoutSwitched)
            break
        case 'WorkspaceUrgencyChanged':
            handleWorkspaceUrgencyChanged(event.WorkspaceUrgencyChanged)
            break
        }
    }

    function handleWorkspacesChanged(data) {
        const newWorkspaces = {}

        for (const ws of data.workspaces) {
            const oldWs = facade.workspaces[ws.id]
            const updatedWs = {}
            for (const prop in ws)
                updatedWs[prop] = ws[prop]

            // Current Niri includes active_window_id in WorkspacesChanged and
            // that snapshot is authoritative. The original integration predates
            // that field and always copied the cached value over the fresh one;
            // once the cache held null, fullscreen/output ownership could stay
            // stale even while `niri msg workspaces` reported the real active
            // window. Preserve the cached value only for older event payloads
            // that genuinely omit the field.
            if (updatedWs.active_window_id === undefined
                    && oldWs && oldWs.active_window_id !== undefined)
                updatedWs.active_window_id = oldWs.active_window_id

            newWorkspaces[ws.id] = updatedWs
        }

        facade.workspaces = newWorkspaces
        facade.allWorkspaces = Object.values(newWorkspaces).sort((a, b) => a.idx - b.idx)

        facade.focusedWorkspaceIndex = facade.allWorkspaces.findIndex(w => w.is_focused)
        if (facade.focusedWorkspaceIndex >= 0) {
            const focusedWs = facade.allWorkspaces[facade.focusedWorkspaceIndex]
            facade.focusedWorkspaceId = focusedWs.id
            facade.currentOutput = focusedWs.output || ""
        } else {
            facade.focusedWorkspaceIndex = 0
            facade.focusedWorkspaceId = ""
        }

        facade.updateCurrentOutputWorkspaces()
        facade.windowOrderChanged()
    }

    function handleWorkspaceActivated(data) {
        const ws = facade.workspaces[data.id]
        if (!ws) {
            return
        }
        const output = ws.output

        const updatedWorkspaces = {}
        let hasChanges = false

        for (const id in facade.workspaces) {
            const workspace = facade.workspaces[id]
            const got_activated = workspace.id === data.id
            const needsUpdate = (workspace.output === output) || data.focused

            if (!needsUpdate) {
                updatedWorkspaces[id] = workspace
                continue
            }

            const updatedWs = {}
            for (let prop in workspace) {
                updatedWs[prop] = workspace[prop]
            }

            if (workspace.output === output) {
                updatedWs.is_active = got_activated
            }

            if (data.focused) {
                updatedWs.is_focused = got_activated
            }

            updatedWorkspaces[id] = updatedWs
            hasChanges = true
        }

        if (!hasChanges) return
        facade.workspaces = updatedWorkspaces

        facade.allWorkspaces = Object.values(updatedWorkspaces).sort((a, b) => a.idx - b.idx)

        facade.focusedWorkspaceIndex = facade.allWorkspaces.findIndex(w => w.is_focused)
        if (facade.focusedWorkspaceIndex >= 0) {
            const focusedWs = facade.allWorkspaces[facade.focusedWorkspaceIndex]
            facade.focusedWorkspaceId = focusedWs.id
            facade.currentOutput = focusedWs.output || ""
        } else {
            facade.focusedWorkspaceIndex = 0
            facade.focusedWorkspaceId = ""
        }

        facade.updateCurrentOutputWorkspaces()

        // When switching to a workspace, check single-window policy
        if (data.focused) {
            facade._scheduleSingleWindowCheck(data.id)
        }
    }

    function handleWindowFocusChanged(data) {
        const focusedWindowId = data.id
        root._latestFocusedWindowId = focusedWindowId

        if (focusedWindowId !== null && focusedWindowId !== undefined) {
            const newOrder = []
            for (let i = 0; i < facade.mruWindowIds.length; i++) {
                const id = facade.mruWindowIds[i]
                if (id !== focusedWindowId) {
                    newOrder.push(id)
                }
            }
            newOrder.unshift(focusedWindowId)
            facade.mruWindowIds = newOrder
        }

        const currentList = root._windowsDirty ? root._pendingWindows : facade.windows
        scheduleWindowsUpdate(currentList)
        const focusedWindow = currentList.find(window => window.id === focusedWindowId)

        if (focusedWindow) {
            const ws = facade.workspaces[focusedWindow.workspace_id]
            if (ws && ws.active_window_id !== focusedWindowId) {
                const updatedWs = {}
                for (let prop in ws) {
                    updatedWs[prop] = ws[prop]
                }
                updatedWs.active_window_id = focusedWindowId

                const updatedWorkspaces = {}
                for (const id in facade.workspaces) {
                    updatedWorkspaces[id] = id === String(focusedWindow.workspace_id) ? updatedWs : facade.workspaces[id]
                }
                facade.workspaces = updatedWorkspaces
            }
        }
    }

    function handleWorkspaceActiveWindowChanged(data) {
        const ws = facade.workspaces[data.workspace_id]
        if (!ws)
            return

        const updatedWs = {}
        for (let prop in ws)
            updatedWs[prop] = ws[prop]
        updatedWs.active_window_id = data.active_window_id

        // for-in keys are strings; Niri's ids are numbers.
        const updatedWorkspaces = {}
        for (const id in facade.workspaces)
            updatedWorkspaces[id] = id === String(data.workspace_id) ? updatedWs : facade.workspaces[id]
        facade.workspaces = updatedWorkspaces
    }

    function handleWindowsChanged(data) {
        const focused = (data.windows ?? []).find(window => window.is_focused === true)
        root._latestFocusedWindowId = focused ? focused.id : null
        scheduleWindowsUpdate(data.windows)
    }

    function handleWindowClosed(data) {
        const currentList = _windowsDirty ? _pendingWindows : facade.windows
        const closedWin = currentList.find(w => w.id === data.id)
        const closedWsId = closedWin?.workspace_id
        const updatedWindows = currentList.filter(w => w.id !== data.id)
        scheduleWindowsUpdate(updatedWindows)

        if (facade.mruWindowIds && facade.mruWindowIds.length > 0) {
            facade.mruWindowIds = facade.mruWindowIds.filter(id => id !== data.id)
        }

        // Check if the workspace now has a single tiling window
        if (closedWsId !== undefined) {
            facade._scheduleSingleWindowCheck(closedWsId)
        }
    }

    function handleWindowOpenedOrChanged(data) {
        if (!data.window)
            return

        const window = data.window
        // A window opened with focus gets no WindowFocusChanged of its own.
        if (window.is_focused === true)
            root._latestFocusedWindowId = window.id
        const currentList = _windowsDirty ? _pendingWindows : facade.windows
        const existingIndex = currentList.findIndex(w => w.id === window.id)
        let updatedWindows

        if (existingIndex >= 0) {
            updatedWindows = [...currentList]
            updatedWindows[existingIndex] = window
        } else {
            updatedWindows = [...currentList, window]
        }
        scheduleWindowsUpdate(updatedWindows)

        // New window opened -- check if workspace needs un-maximize or maximize
        if (existingIndex < 0 && window.workspace_id !== undefined) {
            facade._scheduleSingleWindowCheck(window.workspace_id)
        }
    }

    // Batching para actualizaciones de ventanas
    property bool _windowsDirty: false
    property var _pendingWindows: []
    property bool _windowOrderDirty: false
    property var _latestFocusedWindowId

    // WindowLayoutsChanged is applied to _pendingWindows immediately, while the
    // public sorted list is intentionally batched for UI consumers. Behavioural
    // gates such as fullscreen detection must not wait for that presentation
    // batching or an edge interaction can observe the previous geometry.
    readonly property var liveWindows: _windowsDirty ? _pendingWindows : facade.windows

    Timer {
        id: windowsUpdateTimer
        interval: facade.windowListUpdateIntervalMs
        repeat: false
        onTriggered: {
            if (_windowsDirty) {
                const nextWindows = sortWindowsByLayout(_pendingWindows)
                const orderChanged = _windowOrderDirty
                facade.windows = nextWindows
                facade.activeWindow = nextWindows.find(window => window.is_focused) ?? null
                _windowsDirty = false
                _windowOrderDirty = false
                if (orderChanged)
                    facade.windowOrderChanged()
            }
        }
    }

    function scheduleWindowsUpdate(newWindowsList) {
        const normalizedWindows = root._normalizeWindowFocus(newWindowsList)
        _windowOrderDirty = _windowOrderDirty
            || _windowOrderDiffers(_windowsDirty ? _pendingWindows : facade.windows, normalizedWindows)
        _pendingWindows = normalizedWindows
        _windowsDirty = true
        if (!windowsUpdateTimer.running) {
            windowsUpdateTimer.restart()
        }
    }

    function _normalizeWindowFocus(windowList) {
        if (!Array.isArray(windowList) || root._latestFocusedWindowId === undefined)
            return windowList

        let changed = false
        const normalizedWindows = windowList.map(window => {
            const shouldBeFocused = root._latestFocusedWindowId !== null
                && window.id === root._latestFocusedWindowId
            if (window.is_focused === shouldBeFocused)
                return window

            changed = true
            const normalizedWindow = {}
            for (const prop in window)
                normalizedWindow[prop] = window[prop]
            normalizedWindow.is_focused = shouldBeFocused
            return normalizedWindow
        })
        return changed ? normalizedWindows : windowList
    }

    function _windowOrderDiffers(previousWindows, nextWindows) {
        if (!Array.isArray(previousWindows) || !Array.isArray(nextWindows))
            return true
        if (previousWindows.length !== nextWindows.length)
            return true

        const previousById = new Map()
        for (const window of previousWindows)
            previousById.set(window.id, window)

        for (const window of nextWindows) {
            const previous = previousById.get(window.id)
            if (!previous)
                return true
            if (previous.app_id !== window.app_id
                    || previous.workspace_id !== window.workspace_id
                    || !!previous.is_floating !== !!window.is_floating)
                return true

            const previousPos = previous.layout?.pos_in_scrolling_layout
            const nextPos = window.layout?.pos_in_scrolling_layout
            const previousColumn = Array.isArray(previousPos) ? previousPos[0] : undefined
            const previousRow = Array.isArray(previousPos) ? previousPos[1] : undefined
            const nextColumn = Array.isArray(nextPos) ? nextPos[0] : undefined
            const nextRow = Array.isArray(nextPos) ? nextPos[1] : undefined
            if (previousColumn !== nextColumn || previousRow !== nextRow)
                return true
        }

        return false
    }

    function handleWindowLayoutsChanged(data) {
        if (!data.changes)
            return

        // Usar _pendingWindows si hay cambios pendientes, o windows actual
        const currentList = _windowsDirty ? _pendingWindows : facade.windows
        const updatedWindows = [...currentList]
        let hasChanges = false

        for (const change of data.changes) {
            const windowId = change[0]
            const layoutData = change[1]

            const windowIndex = updatedWindows.findIndex(w => w.id === windowId)
            if (windowIndex < 0)
                continue

            const updatedWindow = {}
            for (var prop in updatedWindows[windowIndex]) {
                updatedWindow[prop] = updatedWindows[windowIndex][prop]
            }
            updatedWindow.layout = layoutData
            updatedWindows[windowIndex] = updatedWindow
            hasChanges = true
        }

        if (!hasChanges)
            return

        scheduleWindowsUpdate(updatedWindows)
    }

    function handleOutputsChanged(data) {
        const nextOutputs = data?.outputs ?? {}
        facade.outputs = nextOutputs
        facade.updateDisplayScales()

        const outputNames = Object.keys(nextOutputs)
        let nextCurrentOutput = ""

        if (facade.focusedWorkspaceIndex >= 0 && facade.focusedWorkspaceIndex < facade.allWorkspaces.length) {
            nextCurrentOutput = facade.allWorkspaces[facade.focusedWorkspaceIndex]?.output ?? ""
        }

        if ((!nextCurrentOutput || nextOutputs[nextCurrentOutput] === undefined) && outputNames.length > 0) {
            nextCurrentOutput = outputNames[0]
        }

        facade.currentOutput = nextCurrentOutput
        facade.updateCurrentOutputWorkspaces()

        // Force immediate update for outputs as it affects geometry calculations significantly
        facade.windows = sortWindowsByLayout(facade.windows)
        facade.windowOrderChanged()
        fetchOutputs()
    }

    function handleOverviewChanged(data) {
        facade.inOverview = data.is_open
    }

    function applyKeyboardLayouts(data) {
        const layouts = data?.keyboard_layouts ?? data
        const names = Array.isArray(layouts?.names) ? layouts.names : []
        const index = Number(layouts?.current_idx ?? 0)

        facade.keyboardLayoutNames = names
        facade.currentKeyboardLayoutIndex = isNaN(index) ? 0 : index
    }

    function handleKeyboardLayoutsChanged(data) {
        applyKeyboardLayouts(data)
    }

    function handleKeyboardLayoutSwitched(data) {
        facade.currentKeyboardLayoutIndex = data.idx
    }

    function handleWorkspaceUrgencyChanged(data) {
        const ws = facade.workspaces[data.id]
        if (!ws)
            return

        const updatedWs = {}
        for (let prop in ws) {
            updatedWs[prop] = ws[prop]
        }
        updatedWs.is_urgent = data.urgent

        const updatedWorkspaces = {}
        for (const id in facade.workspaces) {
            updatedWorkspaces[id] = id === String(data.id) ? updatedWs : facade.workspaces[id]
        }
        facade.workspaces = updatedWorkspaces

        facade.allWorkspaces = Object.values(updatedWorkspaces).sort((a, b) => a.idx - b.idx)

        facade.windowUrgentChanged()
    }


}
