#!/usr/bin/env python3
"""Headless behavior tests for Niri live row bindings and deferred overview updates."""
import os
from pathlib import Path
import shutil
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
runner = shutil.which("qmltestrunner") or "/usr/lib/qt6/bin/qmltestrunner"
if not Path(runner).is_file():
    sys.exit("Qt 6 qmltestrunner is required")
environment = dict(os.environ, QT_QPA_PLATFORM="offscreen", QT_QUICK_BACKEND="software")
sys.exit(subprocess.run([runner, "-input", str(ROOT / "scripts/fixtures/qml-niri-hotspots"),
                         "-o", "-,txt"], cwd=ROOT, env=environment, timeout=30).returncode)
