#!/usr/bin/env python3
"""Exercise real ticker/snapshot/Canvas components with deterministic inputs."""
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]


def check_wiring():
    presets = ROOT / "modules/mediaControls/presets"
    for name in ("FullPlayer", "CompactPlayer", "MinimalPlayer", "ClassicPlayer",
                 "AlbumArtPlayer", "VisualizerPlayer", "LyricsPlayer", "LyricsSplitPlayer",
                 "ExpandingLyricsPlayer"):
        text = (presets / (name + ".qml")).read_text()
        assert "property bool motion: true" in text, name
        base = re.search(r"PlayerBase\s*\{([^}]+)\}", text).group(1)
        assert "positionUpdatesActive: root.visible && root.motion" in base, name
        for overlay in re.findall(r"MediaVisualizerOverlay\s*\{([^}]+)\}", text):
            assert "motion: root.motion" in overlay, name
    print("Media activity wiring: all 9 presets forward presentation gates", flush=True)


def main():
    check_wiring()
    runner = shutil.which("qmltestrunner") or "/usr/lib/qt6/bin/qmltestrunner"
    environment = dict(os.environ, QT_QPA_PLATFORM="offscreen", QT_QUICK_BACKEND="rhi",
                       QSG_RHI_BACKEND="opengl", LIBGL_ALWAYS_SOFTWARE="1")
    result = subprocess.run([runner, "-input", str(ROOT / "scripts/qml-tests"), "-o", "-,txt"],
                            cwd=ROOT, env=environment, timeout=60)
    if result.returncode:
        return result.returncode
    # Copy the unmodified production Canvas into an isolated test module. Only
    # its theme/settings dependencies are stubbed; its paint/lifecycle code is real.
    with tempfile.TemporaryDirectory(prefix="inir-cava-test-") as temporary:
        target = Path(temporary)
        shutil.copy(ROOT / "modules/common/widgets/CavaSpectrum.qml", target)
        fixture = ROOT / "scripts/fixtures/qml-media"
        shutil.copy(fixture / "tst_cava_activity.qml", target)
        shutil.copytree(fixture / "stubs", target / "stubs")
        return subprocess.run([runner, "-input", str(target / "tst_cava_activity.qml"),
                               "-import", str(target / "stubs"), "-o", "-,txt"],
                              cwd=ROOT, env=environment, timeout=60).returncode


if __name__ == "__main__":
    sys.exit(main())
