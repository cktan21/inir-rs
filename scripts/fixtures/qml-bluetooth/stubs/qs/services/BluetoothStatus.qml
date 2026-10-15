pragma Singleton
import QtQuick
QtObject {
    property var leases: ({})
    property int starts: 0
    property int releases: 0
    property int sequence: 0
    function newScanOwner() { return "test-" + (++sequence) }
    function scanRequested(owner) { return !!leases[owner] }
    function requestScan(owner, adapter) {
        if (!owner || !adapter?.enabled) return false
        const next = Object.assign({}, leases)
        next[owner] = adapter
        leases = next
        starts++
        return true
    }
    function releaseScan(owner) {
        if (!leases[owner]) return
        const next = Object.assign({}, leases)
        delete next[owner]
        leases = next
        releases++
    }
}
