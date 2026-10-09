#!/usr/bin/env python3
"""Hold the CXX-Qt network contract in rust/ to the QML service it must replace.

The compiled backend is meant to drop in "without changing QML consumers". That
promise is only worth anything if the two sides are checked against each other:
renaming a QML property or dropping a method would otherwise leave the Rust
contract quietly describing a service that no longer exists.

Network is the live drop-in domain: `services/network/Network.qml` binds the
native `DesktopServices` members through `NativeBackend.desktop`. So every
network member the bridge declares must still be referenced by that QML service
under its camelCase name. Audio, battery, bluetooth and media are intentionally
served by Quickshell's C++ (not re-implemented in Rust), and power/brightness/
niri are produced natively but not yet consumed, so they are listed as reserved
rather than contracted.
"""

import re
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
BRIDGE = ROOT / "rust/inir-qt/src/qobjects/desktop.rs"
NETWORK_QML = ROOT / "services/network/Network.qml"

QPROPERTY = re.compile(r"#\[qproperty\(\s*[^,]+,\s*(\w+)\s*\)\]")
QMETHOD = re.compile(r"#\[(qinvokable|qsignal)\]\s*fn\s+(\w+)")

# Network members the QML service binds through NativeBackend.desktop.
NETWORK_MEMBERS = {
    "network_ready",
    "network_error",
    "wifi_enabled",
    "ethernet",
    "connectivity",
    "network_revision",
}
# Produced natively but without a QML consumer yet (power/brightness) or held
# for Phase 3 (niri); plus command/lease infrastructure bound indirectly.
RESERVED = {
    "active",
    "power_ready",
    "power_error",
    "active_power_profile",
    "power_profiles_json",
    "brightness_ready",
    "brightness_error",
    "brightness_revision",
    "niri_ready",
    "niri_error",
    "overview_open",
    "niri_revision",
    "set_services_active",
    "execute",
    "command_finished",
}
# Domains that must not reappear as native Rust state; they stay Quickshell C++.
FORBIDDEN_MEMBERS = ("bluetooth", "battery", "audio", "media")


def camel(snake: str) -> str:
    head, *rest = snake.split("_")
    return head + "".join(part[:1].upper() + part[1:] for part in rest)


def bridge_members():
    source = BRIDGE.read_text(encoding="utf-8")
    members = set(QPROPERTY.findall(source))
    members.update(name for _kind, name in QMETHOD.findall(source))
    return members


class RustContractTests(unittest.TestCase):
    def test_bridge_exists(self):
        self.assertTrue(BRIDGE.is_file(), BRIDGE)
        self.assertFalse(
            (ROOT / "rust/inir-backend").exists(),
            "inir-backend scaffolding was removed; it must not return",
        )

    def test_network_contract_is_not_vacuous(self):
        self.assertTrue(NETWORK_MEMBERS <= bridge_members())

    def test_network_members_exist_on_the_qml_service(self):
        source = NETWORK_QML.read_text(encoding="utf-8")
        missing = []
        for member in NETWORK_MEMBERS:
            name = camel(member)
            handler = "on" + name[:1].upper() + name[1:] + "Changed"
            if name not in source and handler not in source:
                missing.append(member)
        self.assertEqual(
            missing,
            [],
            "CXX-Qt network contract and Network.qml have drifted apart: "
            + ", ".join(camel(m) for m in missing),
        )

    def test_every_bridge_member_is_contracted_or_reserved(self):
        unknown = bridge_members() - NETWORK_MEMBERS - RESERVED
        self.assertEqual(
            unknown,
            set(),
            "bridge exposes members that are neither contracted nor reserved: "
            + ", ".join(sorted(unknown)),
        )

    def test_removed_domains_stay_out_of_the_bridge(self):
        source = BRIDGE.read_text(encoding="utf-8").lower()
        present = [name for name in FORBIDDEN_MEMBERS if name in source]
        self.assertEqual(
            present,
            [],
            "domains served by Quickshell C++ must not reappear in the bridge: "
            + ", ".join(present),
        )

    def test_bridge_exports_camel_case_qt_names(self):
        self.assertIn("#[auto_cxx_name]", BRIDGE.read_text(encoding="utf-8"))

    def test_cargo_target_is_not_tracked(self):
        ignore = (ROOT / ".gitignore").read_text()
        self.assertIn("rust/target/", ignore)


if __name__ == "__main__":
    unittest.main(verbosity=2)
