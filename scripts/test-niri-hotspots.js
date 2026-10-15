#!/usr/bin/env node
const assert = require("assert/strict")
const fs = require("fs")
const path = require("path")
const vm = require("vm")

const context = {}
vm.runInNewContext(fs.readFileSync(path.resolve(__dirname,
    "../services/compositor/toplevelMatching.js"), "utf8").replace(/^\.pragma library\s*/, ""), context)
const matcher = context.createMatcher()
const handles = [{ appId: "editor", title: "one" }, { appId: "editor", title: "two" },
    { appId: "closed-app", title: "ghost" }]
let windows = [{ id: 1, app_id: "editor", title: "", workspace_id: 1 },
    { id: 2, app_id: "editor", title: "one", workspace_id: 2 }]
let mapping = matcher.update(windows, handles)
assert.equal(mapping.size, 2, "unmatched Wayland ghost must be excluded")
assert.equal(mapping.get("2"), handles[0], "exact match wins over ambiguous app-only match")
assert.equal(mapping.get("1"), handles[1])
const original = mapping
const rematches = matcher.rematches
// Fallback replaces JS snapshots. Focus, workspace and layout order are not
// matching fields and must reuse the associations instead of scanning candidates.
windows = windows.map(w => ({ ...w, is_focused: w.id === 1, workspace_id: 3 })).reverse()
mapping = matcher.update(windows, handles)
assert.equal(mapping, original)
assert.equal(matcher.rematches, rematches)
assert.equal(new Set(mapping.values()).size, mapping.size, "join stays one-to-one")
// Filtering a workspace selects the global map rather than matching a subset.
assert.equal(mapping.get("2"), original.get("2"))
windows.find(w => w.id === 1).title = "one"
windows.find(w => w.id === 2).title = "two"
mapping = matcher.update(windows, handles)
assert.equal(mapping.get("1"), handles[0], "Niri title change invalidates association")
assert.equal(mapping.get("2"), handles[1])
handles[0].title = "two"
handles[1].title = "one"
mapping = matcher.update(windows, handles)
assert.equal(mapping.get("1"), handles[1], "foreign handle title change invalidates association")
assert.equal(mapping.get("2"), handles[0])
const newHandle = { appId: "terminal", title: "term" }
windows[0].app_id = "terminal"
windows[0].title = "term"
mapping = matcher.update(windows, [...handles, newHandle])
assert.equal(mapping.get(String(windows[0].id)), newHandle, "app ID and handle membership changes invalidate")
mapping = matcher.update(windows.filter(w => w.id !== 1), handles)
assert.equal(mapping.has("1"), false, "closed Niri window cannot be resurrected by a live handle")
mapping = matcher.update([], handles)
assert.equal(mapping.size, 0)
console.log("Niri matching: one-to-one, stable focus/order, invalidation and ghost exclusion passed")
