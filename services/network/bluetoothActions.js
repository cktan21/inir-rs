.pragma library

// Command outcomes are separate from BlueZ's observed connection state. Never
// pretend a failed disconnect succeeded, or hide a failed action as Connected.
function key(parameters) {
    return parameters?.adapter && parameters?.address
        ? parameters.adapter + "/" + parameters.address.toUpperCase() : ""
}

function current(actions, parameters) {
    return actions[key(parameters)] ?? { phase: "observed", error: "" }
}

function begin(actions, parameters, id, operation) {
    const device = key(parameters)
    if (!device) return actions
    const next = Object.assign({}, actions)
    next[device] = { id: id, operation: operation, phase: "pending", error: "" }
    return next
}

function finish(actions, parameters, id, success, error) {
    const device = key(parameters)
    const previous = actions[device]
    if (!previous || previous.id !== id) return actions
    const next = Object.assign({}, actions)
    next[device] = { id: id, operation: previous.operation,
        phase: success ? "observed" : "error", error: success ? "" : error }
    return next
}

function presentation(action, connected, paired) {
    if (action.phase === "pending") return "pending"
    if (action.phase === "error") return "error"
    return connected ? "connected" : paired ? "paired" : "discovered"
}
