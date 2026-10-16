#!/usr/bin/env python3
"""Hold the CXX-Qt native contract in rust/ to the QML that consumes it.

The native domains drop in "without changing QML consumers". That promise only
holds if both sides are checked against each other: every member the QML reads
or calls through a native alias must still be declared by the matching
per-domain bridge (qproperty, qinvokable or qsignal), and nothing Quickshell
serves in C++ (audio, battery, Bluetooth, media) may reappear as Rust state.
"""

import re
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
QOBJECTS = ROOT / "rust/inir-qt/src/qobjects"

# QML files and the native bridge each alias resolves to.
CONSUMERS = {
    "services/NativeBackend.qml": {
        "niri": "niri.rs", "network": "network.rs", "brightness": "brightness.rs",
        "power": "power.rs", "workspaces": "workspaces.rs", "keyboard": "keyboard.rs",
    },
    "services/network/Network.qml": {"backendNetwork": "network.rs"},
    "services/display/Brightness.qml": {"backendBrightness": "brightness.rs"},
    "services/compositor/NiriService.qml": {"_nativeBackend": "niri.rs"},
    "services/compositor/NiriModelFacade.qml": {"niri": "niri.rs", "workspaceModel": "workspaces.rs"},
    "services/KeyboardIndicators.qml": {"backendKeyboard": "keyboard.rs"},
    "services/network/Vpn.qml": {"backendNetwork": "network.rs"},
    "services/network/Hotspot.qml": {"backendNetwork": "network.rs"},
    "services/native/BluetoothProvider.qml": {"controller": "bluetooth_control.rs"},
    "services/network/BluetoothStatus.qml": {"controller": "bluetooth_control.rs"},
    "services/Utilities.qml": {"controller": "utility_jobs.rs", "keyboard": "keyboard.rs"},
    "services/native/UtilityProvider.qml": {"controller": "utility_jobs.rs", "keyboard": "keyboard.rs"},
}

# Members inherited from QAbstractListModel/QObject or from the shared list_model.h
# base, which the bridge does not declare itself.
BASE_MEMBERS = {"jsValue", "count", "rowCount", "data", "roleNames", "index", "objectName"}

# Native domains the Provider exposes as QML types.
PROVIDED_TYPES = {
    "NativeNiri": "niri.rs", "NativeWorkspaces": "workspaces.rs", "NativeNetwork": "network.rs",
    "NativeBrightness": "brightness.rs", "NativePower": "power.rs",
    "NativeKeyboard": "keyboard.rs",
    "SystemInfo": "system_info.rs", "ConfigService": "config.rs",
    "NativeBluetoothController": "bluetooth_control.rs",
    "NativeUtilityJobs": "utility_jobs.rs",
}

QPROPERTY = re.compile(r"#\[qproperty\(\s*[^,]+,\s*(\w+)\s*\)\]")
QMETHOD = re.compile(r"#\[(qinvokable|qsignal)\]\s*fn\s+(\w+)")
QML_REFERENCE = r"(?<![\w.])(?:root\.)?{alias}\??\.([A-Za-z_]\w*)"

# Quickshell-owned domains must not reappear as native Rust state.
FORBIDDEN = ("bluetooth", "battery", "audio", "media")


def camel(snake: str) -> str:
    head, *rest = snake.split("_")
    return head + "".join(part[:1].upper() + part[1:] for part in rest)


def bridge_members(name: str) -> set:
    source = (QOBJECTS / name).read_text(encoding="utf-8")
    members = set(QPROPERTY.findall(source))
    members.update(method for _kind, method in QMETHOD.findall(source))
    return {camel(member) for member in members}


def referenced_members(qml: str, alias: str) -> set:
    source = (ROOT / qml).read_text(encoding="utf-8")
    return set(re.findall(QML_REFERENCE.format(alias=re.escape(alias)), source))


class RustContractTests(unittest.TestCase):
    def test_bridges_exist_and_backend_scaffolding_stays_removed(self):
        for name in set(PROVIDED_TYPES.values()) | {b for m in CONSUMERS.values() for b in m.values()}:
            self.assertTrue((QOBJECTS / name).is_file(), name)
        self.assertFalse((ROOT / "rust/inir-backend").exists(), "inir-backend scaffolding must not return")

    def test_every_consumed_member_is_declared_by_its_bridge(self):
        missing = []
        for qml, aliases in CONSUMERS.items():
            for alias, bridge in aliases.items():
                declared = bridge_members(bridge) | BASE_MEMBERS
                for member in referenced_members(qml, alias) - declared:
                    missing.append(f"{qml}: {alias}.{member} (not in {bridge})")
        self.assertEqual(missing, [], "QML consumes native members the bridge no longer declares:\n  " + "\n  ".join(missing))

    def test_provider_types_are_exported_as_qml_singletons(self):
        for type_name, bridge in PROVIDED_TYPES.items():
            source = (QOBJECTS / bridge).read_text(encoding="utf-8")
            self.assertIn(f"type {type_name}", source, f"{bridge} does not declare {type_name}")
            self.assertIn("qml_singleton", source, f"{bridge} is not a QML singleton")

    def test_removed_domains_stay_out_of_the_bridges(self):
        present = []
        for path in QOBJECTS.glob("*.rs"):
            text = path.read_text(encoding="utf-8").lower()
            if path.name == "bluetooth_control.rs":
                # Control, pairing agents and transfer jobs are native. Device
                # discovery data still has exactly one owner: Quickshell.
                self.assertNotIn("bluetooth_devices", text)
                self.assertNotIn("bluetooth_adapters", text)
                continue
            present.extend(f"{path.name}:{name}" for name in FORBIDDEN if f"fn {name}" in text or f"type {name}" in text)
        self.assertEqual(present, [], "Quickshell-owned domains must not reappear in the bridge")

    def test_bridges_export_camel_case_qt_names(self):
        for path in QOBJECTS.glob("*.rs"):
            text = path.read_text(encoding="utf-8")
            if "cxx_qt::bridge" in text:
                self.assertIn("#[auto_cxx_name]", text, path.name)

    def test_cargo_target_is_not_tracked(self):
        self.assertIn("rust/target/", (ROOT / ".gitignore").read_text())


if __name__ == "__main__":
    unittest.main(verbosity=2)
