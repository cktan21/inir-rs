#!/usr/bin/env python3
"""Compile Bluetooth surfaces with Quickshell, without instantiating windows."""
import os
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
BOOTSTRAP = """//@ pragma ShellId inir-bluetooth-component-test
import QtQuick
import Quickshell
import qs.modules.common.functions
import qs.modules.common.widgets
import qs.services
ShellRoot { Loader { source: "scripts/fixtures/qml-bluetooth/component-smoke.qml" } }
"""


def main():
    if not os.environ.get("WAYLAND_DISPLAY"):
        print("SKIP: component compilation requires a Wayland PanelWindow backend")
        return 77
    environment = dict(os.environ, QT_QPA_PLATFORM="wayland", INIR_NATIVE_DOMAINS="none")
    imports = ROOT / "rust/build/qml"
    if (imports / "qs/services/native/qmldir").exists():
        environment["QML_IMPORT_PATH"] = str(imports) + ":" + environment.get("QML_IMPORT_PATH", "")
    # Quickshell limits dynamic components to its configuration root. A tiny
    # owned temporary bootstrap in that root lets us compile real files while
    # leaving shell.qml, configuration, services and the running lock untouched.
    with tempfile.NamedTemporaryFile(mode="w", prefix=".inir-bt-compile-", suffix=".qml", dir=ROOT) as stream:
        stream.write(BOOTSTRAP)
        stream.flush()
        result = subprocess.run(["qs", "--no-color", "-p", stream.name], cwd=ROOT,
                                env=environment, capture_output=True, text=True, timeout=30)
    output = result.stdout + result.stderr
    print(output, end="")
    return 0 if result.returncode == 0 and "BLUETOOTH_COMPILE_FAIL" not in output \
        and output.count("BLUETOOTH_COMPILE_PASS:") == 9 else 1


if __name__ == "__main__":
    raise SystemExit(main())
