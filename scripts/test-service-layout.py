#!/usr/bin/env python3
"""Guard service consolidation without instantiating side-effectful QML services."""

import importlib.util
import re
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SERVICES = ROOT / "services"
REGISTRATION = re.compile(
    r"^(?:(singleton)\s+)?([A-Z]\w*)\s+(\d+\.\d+)\s+(\S+\.qml)\s*$", re.M
)
DOMAIN_TYPES = {
    "network": ("Network", "Vpn", "BluetoothStatus", "Hotspot"),
    "compositor": (
        "CompositorService",
        "NiriService",
        "DankSocket",
        "HyprlandData",
        "NiriAnimationPresets",
    ),
    "display": ("Brightness", "Hyprsunset"),
    "media": ("Audio", "MprisController"),
    "power": ("Battery", "Idle", "PowerProfilePersistence"),
    "system": ("ResourceUsage", "SystemInfo", "MemoryPressureService"),
}
POLICIES = {"display": "brightnessPolicy.js", "power": "idlePolicy.js"}


def registrations(path):
    return REGISTRATION.findall(path.read_text(encoding="utf-8"))


class ServiceLayoutTests(unittest.TestCase):
    def test_service_module_registrations_resolve(self):
        for registry in SERVICES.rglob("qmldir"):
            entries = registrations(registry)
            names = [entry[1] for entry in entries]
            self.assertEqual(len(names), len(set(names)), str(registry))
            for _, name, _, filename in entries:
                with self.subTest(registry=registry.relative_to(ROOT), name=name):
                    target = registry.parent / filename
                    self.assertTrue(target.is_file(), str(target))
                    self.assertTrue(target.resolve().is_relative_to(SERVICES.resolve()))

    def test_public_backend_names_and_declarations_are_preserved(self):
        registry = SERVICES / "qmldir"
        self.assertEqual(registry.read_text().splitlines()[0], "module qs.services")
        entries = {
            name: (kind, version, filename)
            for kind, name, version, filename in registrations(registry)
        }
        for domain, names in DOMAIN_TYPES.items():
            for name in names:
                with self.subTest(name=name):
                    # Preserve existing registrations, including the legacy HyprlandData declaration.
                    kind = "" if name in ("DankSocket", "HyprlandData") else "singleton"
                    self.assertEqual(
                        entries[name], (kind, "1.0", f"{domain}/{name}.qml")
                    )

    def test_domain_folders_do_not_add_public_modules(self):
        for domain in DOMAIN_TYPES:
            if domain != "network":
                self.assertFalse((SERVICES / domain / "qmldir").exists(), domain)
        registry = SERVICES / "network/qmldir"
        self.assertEqual(
            registry.read_text().splitlines()[0], "module qs.services.network"
        )
        self.assertEqual(
            registrations(registry),
            [("", "WifiAccessPoint", "1.0", "WifiAccessPoint.qml")],
        )

    def test_old_flat_implementations_are_absent(self):
        for names in DOMAIN_TYPES.values():
            for name in names:
                self.assertFalse((SERVICES / f"{name}.qml").exists(), name)
        for filename in POLICIES.values():
            self.assertFalse((SERVICES / filename).exists(), filename)

    def test_cross_domain_dependencies_are_explicit(self):
        independent = {"DankSocket", "PowerProfilePersistence", "SystemInfo"}
        for domain, names in DOMAIN_TYPES.items():
            for name in names:
                if name not in independent:
                    source = (SERVICES / domain / f"{name}.qml").read_text()
                    self.assertRegex(source, r"(?m)^import qs\.services\s*$", name)

    def test_relative_javascript_imports_resolve(self):
        for path in SERVICES.rglob("*.qml"):
            for filename in re.findall(
                r'^import "([^"\n]+\.js)"', path.read_text(), re.M
            ):
                target = (
                    ROOT / filename[5:]
                    if filename.startswith("root:")
                    else path.parent / filename
                )
                self.assertTrue(target.is_file(), f"{path}: {filename}")
        for domain, filename in POLICIES.items():
            self.assertTrue((SERVICES / domain / filename).is_file())

    def test_runtime_payload_contains_registered_types_and_helpers(self):
        spec = importlib.util.spec_from_file_location(
            "runtime_payload", ROOT / "sdata/lib/runtime-payload.py"
        )
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        shipped = set(module.Payload(ROOT).paths())
        for registry in SERVICES.rglob("qmldir"):
            self.assertIn(registry.relative_to(ROOT).as_posix(), shipped)
            for _, _, _, filename in registrations(registry):
                target = (registry.parent / filename).relative_to(ROOT).as_posix()
                self.assertIn(target, shipped)
        for domain, filename in POLICIES.items():
            self.assertIn(f"services/{domain}/{filename}", shipped)


if __name__ == "__main__":
    unittest.main(verbosity=2)
