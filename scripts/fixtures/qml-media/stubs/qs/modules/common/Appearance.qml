pragma Singleton
import QtQuick
QtObject {
    property bool animationsEnabled: false
    function calcEffectiveDuration(value) { return value }
}
