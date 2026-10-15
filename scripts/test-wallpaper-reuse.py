#!/usr/bin/env python3
"""Run the real wallpaper blur lifecycle and pixel-refresh tests headlessly."""
import os
from pathlib import Path
import shutil
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]


def main():
    runner = shutil.which("qmltestrunner") or "/usr/lib/qt6/bin/qmltestrunner"
    if not Path(runner).is_file():
        print("Qt Quick Test's qmltestrunner is required", file=sys.stderr)
        return 1
    # Offscreen defaults to Qt's software scene graph on some distributions;
    # that cannot draw GaussianBlur. Use OpenGL/RHI with Mesa software rendering
    # to exercise actual cached pixels without opening a desktop window.
    environment = dict(os.environ, QT_QPA_PLATFORM="offscreen",
                       QT_QUICK_BACKEND="rhi", QSG_RHI_BACKEND="opengl",
                       LIBGL_ALWAYS_SOFTWARE="1")
    return subprocess.run([runner, "-input", str(ROOT / "scripts/qml-tests"),
                           "-o", "-,txt"], cwd=ROOT, env=environment,
                          timeout=60).returncode


if __name__ == "__main__":
    sys.exit(main())
