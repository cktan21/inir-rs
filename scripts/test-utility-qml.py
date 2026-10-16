#!/usr/bin/env python3
"""Run the real utility facade in an isolated shell root, with no user config."""
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
BOOTSTRAP = r'''//@ pragma ShellId inir-utility-fixture
import QtQuick
import Quickshell
import qs.services
ShellRoot {
    property int finished: 0
    property bool failed: false
    function check(condition, message) {
        if (!condition) { console.error("UTILITY_FAIL: " + message); failed = true }
        finished++
        if (finished === 5) {
            const nativeExpected = Quickshell.env("INIR_UTILITY_TEST_MODE") === "native"
            if (nativeExpected !== Boolean(Utilities.controller?.ready)) {
                console.error("UTILITY_FAIL: wrong transport"); failed = true
            }
            console.log(failed ? "UTILITY_FAILED" : "UTILITY_PASS"); Qt.quit()
        }
    }
    Component.onCompleted: {
        Utilities.request("files.read", {paths: [Quickshell.shellPath("payload.txt")]}, (ok, result, error) =>
            check(ok && Array.isArray(result) && result[0].text === "exact\n\n", "file/array payload: " + error))
        Utilities.request("lyrics.fetch", {}, (ok, result, error) => check(ok && result.status === "no_info", "lyrics: " + error))
        Utilities.request("system.networkCounters", {interfaces: ["../invalid"]}, (ok, result, error) =>
            check(!ok && error === "Invalid network interface name", "error envelope: " + error))
        const canceled = Utilities.request("lyrics.fetch", {}, (ok, result, error) =>
            check(!ok && error.includes("canceled"), "cancellation: " + error))
        Utilities.cancel(canceled)
        Utilities.request("lyrics.fetch", {title: "fixture", artist: "fixture", api: "http://127.0.0.1:1"},
            (ok, result, error) => check(!ok && error.includes("timed out"), "deadline: " + error), 50)
    }
    Timer { interval: 10000; running: true; onTriggered: { console.error("UTILITY_FAIL: timed out"); Qt.quit() } }
}'''


def main():
    native = Path(os.environ.get("INIR_TEST_NATIVE_QML_DIR", ROOT / "rust/build/qml"))
    for mode in ("cli", "native"):
        if mode == "native" and not (native / "qs/services/native/qmldir").is_file():
            print("SKIP: utility native facade needs a built plugin")
            return 77
        with tempfile.TemporaryDirectory(prefix="inir-utility-qml-") as directory:
            root = Path(directory)
            (root / "services/native").mkdir(parents=True)
            (root / "modules/common").mkdir(parents=True)
            (root / "services/qmldir").write_text("module qs.services\nsingleton Utilities 1.0 Utilities.qml\n")
            shutil.copy(ROOT / "services/Utilities.qml", root / "services/Utilities.qml")
            provider = root / "services/native/UtilityProvider.qml"
            if mode == "native":
                shutil.copy(ROOT / "services/native/UtilityProvider.qml", provider)
            else:
                provider.write_text("import QtQuick\nItem { readonly property var controller: null; readonly property var keyboard: null }\n")
            (root / "modules/common/qmldir").write_text("module qs.modules.common\nsingleton Directories 1.0 Directories.qml\n")
            (root / "modules/common/Directories.qml").write_text('pragma Singleton\nimport Quickshell\nSingleton { readonly property string utilityHelperPath: Quickshell.env("INIR_HELPER_BIN") }\n')
            (root / "payload.txt").write_bytes(b"exact\n\n")
            (root / "shell.qml").write_text(BOOTSTRAP)
            environment = dict(os.environ, QT_QPA_PLATFORM="offscreen", INIR_RUST_BACKEND="0", INIR_NATIVE_DOMAINS="none",
                INIR_HELPER_BIN=str(ROOT / "rust/target/debug/inir-helper"), QML_IMPORT_PATH=str(native), INIR_UTILITY_TEST_MODE=mode)
            result = subprocess.run(["qs", "--no-color", "-p", str(root)], capture_output=True, text=True, env=environment, timeout=20)
            output = result.stdout + result.stderr
            print(f"{mode}:\n{output}", end="")
            if result.returncode or "UTILITY_PASS" not in output or "UTILITY_FAIL" in output:
                return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
