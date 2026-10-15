#!/usr/bin/env python3
"""Run the unmodified discovery presentation helper with deterministic peers."""
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]


def main():
    # Hardware-facing singleton dependencies are replaced; the production
    # helper's timers, sorting, subscriptions and lifecycle remain unchanged.
    runner = shutil.which("qmltestrunner") or "/usr/lib/qt6/bin/qmltestrunner"
    with tempfile.TemporaryDirectory(prefix="inir-bluetooth-test-") as temporary:
        target = Path(temporary)
        fixture = ROOT / "scripts/fixtures/qml-bluetooth"
        shutil.copytree(fixture, target, dirs_exist_ok=True)
        shutil.copy(ROOT / "services/network/BluetoothDiscoveryView.qml", target)
        shutil.copy(ROOT / "services/network/bluetoothActions.js", target)
        return subprocess.run([runner, "-input", str(target),
                               "-import", str(target / "stubs"), "-o", "-,txt"],
                              env=dict(os.environ, QT_QPA_PLATFORM="offscreen", QT_QUICK_BACKEND="software"),
                              timeout=30).returncode


if __name__ == "__main__":
    raise SystemExit(main())
