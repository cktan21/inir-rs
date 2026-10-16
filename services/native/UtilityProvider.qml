pragma ComponentBehavior: Bound
import QtQuick
import qs.services.native as Native

Item {
    readonly property var controller: Native.NativeUtilityJobs
    readonly property var keyboard: Native.NativeKeyboard
}
