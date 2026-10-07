#!/usr/bin/env python3
"""Hold the CXX-Qt contracts in rust/ to the QML services they must replace.

Milestone 2.2 of docs/plans/UI_CONSOLIDATION_AND_BACKEND_BOUNDARY.md exists so a
compiled backend can drop in "without changing QML consumers". That promise is
only worth anything if the two sides are checked against each other: renaming a
QML property or dropping a method would otherwise leave the Rust contract quietly
describing a service that no longer exists.

So for every member a bridge declares, the matching QML singleton must still
expose it under its camelCase name, at the root of the singleton — a function
nested inside an `IpcHandler` is not part of the QML API that layouts bind to.

The contract is deliberately a subset: a service may expose more than is
contracted, and `rust/` is not required to cover every service yet. What is
forbidden is a contract that claims something the QML side does not have.
"""

import re
import sys
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
BRIDGES = ROOT / "rust/inir-backend/src"

# Bridge source -> the QML singleton it is a contract for.
CONTRACTS = {
    "audio.rs": "services/media/Audio.qml",
    "battery.rs": "services/power/Battery.qml",
    "brightness.rs": "services/display/Brightness.qml",
}

QPROPERTY = re.compile(r"#\[qproperty\(\s*[^,]+,\s*(\w+)\s*\)\]")
QMETHOD = re.compile(r"#\[(qinvokable|qsignal)\]\s*fn\s+(\w+)")

# Root-level members of a QML singleton sit at exactly one indent level; anything
# deeper belongs to a nested object such as an IpcHandler or a Process.
QML_PROPERTY = re.compile(r"^ {4}(?:readonly\s+)?property\s+[\w.<>]+\s+(\w+)\s*:", re.M)
QML_REQUIRED = re.compile(r"^ {4}required\s+property\s+[\w.<>]+\s+(\w+)", re.M)
QML_FUNCTION = re.compile(r"^ {4}function\s+(\w+)\s*\(", re.M)
QML_SIGNAL = re.compile(r"^ {4}signal\s+(\w+)", re.M)


def camel(snake: str) -> str:
    head, *rest = snake.split("_")
    return head + "".join(part[:1].upper() + part[1:] for part in rest)


def qml_members(path: Path) -> set:
    source = path.read_text(encoding="utf-8")
    members = set()
    for pattern in (QML_PROPERTY, QML_REQUIRED, QML_FUNCTION, QML_SIGNAL):
        members.update(pattern.findall(source))
    return members


def contracted(path: Path):
    source = path.read_text(encoding="utf-8")
    for name in QPROPERTY.findall(source):
        yield "property", name
    for kind, name in QMETHOD.findall(source):
        yield ("signal" if kind == "qsignal" else "method"), name


class RustContractTests(unittest.TestCase):
    def test_every_bridge_maps_to_a_known_service(self):
        bridges = {p.name for p in BRIDGES.glob("*.rs")} - {"lib.rs"}
        self.assertEqual(
            bridges,
            set(CONTRACTS),
            "a bridge without an entry here is a contract nothing checks",
        )

    def test_contracts_are_not_vacuous(self):
        for bridge in CONTRACTS:
            with self.subTest(bridge=bridge):
                self.assertTrue(list(contracted(BRIDGES / bridge)))

    def test_contracted_members_exist_on_the_qml_service(self):
        missing = []
        for bridge, qml in CONTRACTS.items():
            service = ROOT / qml
            self.assertTrue(service.is_file(), qml)
            members = qml_members(service)
            for kind, name in contracted(BRIDGES / bridge):
                if camel(name) not in members:
                    missing.append(f"{bridge}: {kind} {name} -> {qml} has no {camel(name)}")
        self.assertEqual(
            missing,
            [],
            "CXX-Qt contract and QML service have drifted apart:\n  "
            + "\n  ".join(missing),
        )

    def test_bridges_do_not_shadow_the_qml_module(self):
        """A native module named qs.services would hide the QML one it is compared against."""
        build = (ROOT / "rust/inir-backend/build.rs").read_text()
        uris = re.findall(r'QmlModule::new\("([^"]+)"\)', build)
        self.assertTrue(uris)
        for uri in uris:
            self.assertNotEqual(uri, "qs.services")

    def test_cargo_target_is_not_tracked(self):
        ignore = (ROOT / ".gitignore").read_text()
        self.assertIn("rust/target/", ignore)


if __name__ == "__main__":
    unittest.main(verbosity=2)
