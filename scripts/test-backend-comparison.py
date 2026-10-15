#!/usr/bin/env python3
"""Validate benchmark ownership guards without touching desktop services."""
import copy
import importlib.util
import json
from pathlib import Path
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("comparison", ROOT / "scripts/compare-backends.py")
comparison = importlib.util.module_from_spec(spec)
spec.loader.exec_module(comparison)

QML = {
    "typedBridge": False, "requested": False, "available": False,
    "desktopActive": False, "nativePowerReady": False,
    "idleInhibited": True, "gameMode": False,
    "niri": {"owner": "qml", "nativeReady": False, "fallbackLoaded": True,
             "fallback": {"eventSocketConnected": True}, "nativeApplyCalls": 0},
    "network": {"owner": "qml", "nativeReady": False, "monitorRunning": True},
    "brightness": {"owner": "qml", "nativeReady": False},
}


class OwnershipTests(unittest.TestCase):
    def check_ownership(self, state=None, *, loaded=True, bluetooth=True):
        def ipc(_pid, target, _method):
            return json.dumps(state if target == "backend" else {
                "mode": "external", "available": bluetooth, "ready": bluetooth})
        with patch.object(comparison, "ipc", side_effect=ipc), \
                patch.object(comparison, "shell_pids", return_value=[123]), \
                patch.object(Path, "read_text", return_value="libqs_services_native.so" if loaded else ""):
            return comparison.ownership(123, "qml")

    def test_common_bluetooth_plugin_does_not_count_as_native_domain_ownership(self):
        state = self.check_ownership(copy.deepcopy(QML))
        self.assertTrue(state["commonBluetooth"]["available"])
        self.assertEqual(state["niri"]["owner"], "qml")

    def test_missing_plugin_retains_qml_fallback(self):
        self.check_ownership(copy.deepcopy(QML), loaded=False, bluetooth=False)

    def test_unexplained_native_plugin_is_rejected(self):
        with self.assertRaises(AssertionError):
            self.check_ownership(copy.deepcopy(QML), bluetooth=False)

    def test_native_domain_leak_is_still_rejected(self):
        state = copy.deepcopy(QML)
        state["desktopActive"] = True
        with self.assertRaises(AssertionError):
            self.check_ownership(state)


if __name__ == "__main__":
    unittest.main()
