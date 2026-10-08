#!/usr/bin/env python3
"""Validate baseline math and error handling without a desktop session."""
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("backend_profile", ROOT / "scripts/profile-backend.py")
profile = importlib.util.module_from_spec(spec)
spec.loader.exec_module(profile)


class BaselineTests(unittest.TestCase):
    def test_process_name_with_spaces_and_parentheses(self):
        with tempfile.TemporaryDirectory() as temporary:
            proc = Path(temporary)
            (proc / "123").mkdir()
            # Linux fields 3..52; set ppid, utime, stime, starttime and rss.
            fields = ["0"] * 50
            fields[0], fields[1], fields[11], fields[12], fields[19], fields[21] = "S", "7", "10", "20", "42", "5"
            (proc / "123/stat").write_text("123 (qs (inir) shell) " + " ".join(fields))
            result = profile.process_stat(123, proc)
            self.assertEqual(result, {"ppid": 7, "ticks": 30, "start": 42, "rss": 5*os.sysconf("SC_PAGE_SIZE")})

    def test_percentiles_are_interpolated(self):
        self.assertEqual(profile.percentile([4, 1, 3, 2], .5), 2.5)
        self.assertAlmostEqual(profile.percentile([4, 1, 3, 2], .95), 3.85)

    def test_live_capture_reports_process_metrics(self):
        report = profile.capture(os.getpid(), .15, .05)
        self.assertGreater(report["rssMeanBytes"], 0)
        self.assertGreaterEqual(report["cpuMeanPercent"], 0)
        self.assertGreaterEqual(report["durationSeconds"], .15)
        self.assertTrue(report["samples"])
        self.assertNotIn("forksPerMinute", report)
        self.assertNotIn("wakeupsPerSecond", report)

    def test_unavailable_process_produces_an_explicit_artifact(self):
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary) / "baseline.json"
            run = subprocess.run([sys.executable, str(ROOT / "scripts/profile-backend.py"),
                                  "--pid", "2147483647", "--layout", "iris", "--output", str(output)], capture_output=True, text=True)
            self.assertEqual(run.returncode, 2)
            report = json.loads(output.read_text())
            self.assertFalse(report["available"])
            self.assertIsNone(report["metrics"])
            self.assertIn("error", report)


if __name__ == "__main__":
    unittest.main(verbosity=2)
