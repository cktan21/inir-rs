.pragma library

// Wayland has no Niri window ID on foreign-toplevel handles. Keep this join
// one-to-one and authoritative to Niri, including when callers filter a subset.
function score(handle, window) {
    if (handle.appId !== window.app_id) return 0
    if (handle.title && window.title) {
        if (handle.title === window.title) return 3
        if (handle.title.includes(window.title) || window.title.includes(handle.title)) return 2
    }
    return 1
}

function sameFields(previous, appId, title) {
    return previous && previous.appId === appId && previous.title === title
}

function createMatcher() {
    return {
        windows: new Map(),
        handles: new Map(),
        associations: new Map(),
        rematches: 0,
        update: function(windows, handles) {
            const nextWindows = new Map(), nextHandles = new Map()
            let dirty = false
            for (const window of windows) {
                const key = String(window.id)
                nextWindows.set(key, { appId: window.app_id, title: window.title })
                if (!sameFields(this.windows.get(key), window.app_id, window.title)) dirty = true
            }
            for (const handle of handles) {
                if (!handle) continue
                nextHandles.set(handle, { appId: handle.appId, title: handle.title })
                if (!sameFields(this.handles.get(handle), handle.appId, handle.title)) dirty = true
            }
            if (nextWindows.size !== this.windows.size || nextHandles.size !== this.handles.size) dirty = true
            // Focus, geometry, workspace and collection order do not affect the
            // association. Native live getters and fallback row replacement can
            // therefore reuse both the join and the enriched record identities.
            if (!dirty) return this.associations
            this.windows = nextWindows
            this.handles = nextHandles

            const index = new Map()
            for (const handle of nextHandles.keys()) {
                if (!handle.appId) continue
                if (!index.has(handle.appId)) index.set(handle.appId, [])
                index.get(handle.appId).push(handle)
            }
            const previous = this.associations, result = new Map(), used = new Set()
            // Matching must not change when layout order changes. Exact title
            // matches take precedence over containment and app-only matches so
            // an ambiguous window cannot consume another window's exact handle.
            const ordered = windows.slice().sort((a, b) => String(a.id).localeCompare(String(b.id)))
            for (let rank = 3; rank > 0; --rank) {
                for (const window of ordered) {
                    const key = String(window.id)
                    if (result.has(key)) continue
                    const candidates = index.get(window.app_id) || []
                    const retained = previous.get(key)
                    let match = candidates.includes(retained) && !used.has(retained)
                        && score(retained, window) === rank ? retained : null
                    if (!match) match = candidates.find(handle => !used.has(handle) && score(handle, window) === rank)
                    if (match) {
                        result.set(key, match)
                        used.add(match)
                    }
                }
            }
            this.associations = result
            this.rematches++
            return result
        }
    }
}
