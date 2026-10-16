#!/usr/bin/env python3
"""Compile changed real components without creating their windows/services."""
import os
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
FILES = ["services/Utilities.qml", "services/LyricsService.qml", "services/DeviceEvents.qml",
    "services/KeyboardIndicators.qml", "services/DevNavigation.qml", "services/deferred/Cliphist.qml",
    "services/network/Vpn.qml", "services/system/ResourceUsage.qml", "services/system/MemoryPressureService.qml",
    "modules/onScreenKeyboard/OskContent.qml", "modules/lock/LockKeyboard.qml"]


def main():
    if not os.environ.get("WAYLAND_DISPLAY"):
        print("SKIP: component compilation requires the Wayland window backend")
        return 77
    import json
    bootstrap = '''//@ pragma ShellId inir-utility-compile-fixture
import QtQuick
import Quickshell
import qs.services
ShellRoot {
    Component.onCompleted: {
        for (const file of %s) {
            const component = Qt.createComponent(Qt.resolvedUrl(file), Component.PreferSynchronous)
            if (component.status === Component.Ready) console.log("UTILITY_COMPILE_PASS: " + file)
            else console.error("UTILITY_COMPILE_FAIL: " + file + " " + component.errorString())
            component.destroy()
        }
    }
    Timer { interval: 100; running: true; onTriggered: Qt.quit() }
}''' % json.dumps(FILES)
    environment = dict(os.environ, INIR_NATIVE_DOMAINS="none", QT_QPA_PLATFORM="wayland",
        QML_IMPORT_PATH=os.environ.get("INIR_TEST_NATIVE_QML_DIR", str(ROOT / "rust/build/qml")))
    with tempfile.NamedTemporaryFile(mode="w", prefix=".inir-utility-compile-", suffix=".qml", dir=ROOT) as stream:
        stream.write(bootstrap)
        stream.flush()
        result = subprocess.run(["qs", "--no-color", "-p", stream.name], env=environment, capture_output=True, text=True, timeout=30)
    output = result.stdout + result.stderr
    print(output, end="")
    return 0 if result.returncode == 0 and "UTILITY_COMPILE_FAIL" not in output and output.count("UTILITY_COMPILE_PASS:") == len(FILES) else 1


if __name__ == "__main__":
    raise SystemExit(main())
