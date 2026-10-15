pragma ComponentBehavior: Bound

import QtQuick
import Qt5Compat.GraphicalEffects

// Keep the effect (and its output cache) across focus-opacity fades. The caller
// owns eligibility: disabling the feature or changing wallpaper kind releases it.
Loader {
    id: root

    property Item sourceItem: null
    property real radius: 32
    property real fadeProgress: 0
    property bool retain: false
    property bool sourceAnimating: false

    active: retain && sourceItem !== null && radius > 0
    opacity: Math.max(0, Math.min(1, fadeProgress))

    sourceComponent: GaussianBlur {
        source: root.sourceItem
        radius: root.radius
        // Keep the existing wallpaper blur quality and sample cap (#159).
        samples: Math.min(33, radius * 2 + 1)
        // Focus changes animate the Loader's opacity, not the effect's source.
        // Qt updates the cache when source pixels, dimensions or radius change.
        cached: !root.sourceAnimating
    }
}
