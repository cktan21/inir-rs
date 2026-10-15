#!/usr/bin/env python3
"""Check the real LiveLayer on native, transparent, input-inert test surfaces."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]


def main():
    with tempfile.TemporaryDirectory(prefix="inir-live-layer-test-") as temporary:
        target = Path(temporary)
        shutil.copy(ROOT / "modules/common/widgets/LiveLayer.qml", target)
        shutil.copy(ROOT / "scripts/fixtures/qml-media/live-layer.qml", target)
        environment = dict(os.environ, QT_QPA_PLATFORM="wayland", INIR_DISABLE_HOT_RELOAD="1")
        child = subprocess.Popen(["qs", "--no-color", "-p", str(target / "live-layer.qml")],
                                 env=environment, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
        try:
            output, _ = child.communicate(timeout=15)
        except subprocess.TimeoutExpired:
            child.terminate()
            output, _ = child.communicate(timeout=5)
            print(output)
            print("LiveLayer test timed out", file=sys.stderr)
            return 1
        finally:
            if child.poll() is None:
                child.terminate()
                child.wait(timeout=5)
        print(output)
        marker = "ACTIVITY_TEST_RESULT: "
        results = [json.loads(line.split(marker, 1)[1]) for line in output.splitlines() if marker in line]
        return 0 if child.returncode == 0 and results and not results[-1]["failed"] else 1


if __name__ == "__main__":
    sys.exit(main())
