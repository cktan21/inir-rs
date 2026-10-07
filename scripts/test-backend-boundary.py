#!/usr/bin/env python3
"""Guard the qs.services boundary: UI layers describe, services execute.

Step 2 of docs/plans/UI_CONSOLIDATION_AND_BACKEND_BOUNDARY.md makes
`qs.services.*` the only place that talks to the system, so the whole backend
can later be swapped for a compiled implementation without touching a single
layout. A `Process` spawned from `modules/` is exactly the coupling that would
have to be unpicked again, so it fails here instead.

Comments and doc blocks are stripped before matching: naming a tool while
explaining why a service behaves as it does is not a boundary violation.
"""

import re
import sys
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
MODULES = ROOT / "modules"

# Tools that read or mutate system state and therefore belong behind a service.
SYSTEM_COMMANDS = (
    "wpctl",
    "pactl",
    "nmcli",
    "bluetoothctl",
    "brightnessctl",
    "playerctl",
    "rfkill",
    "pamixer",
    "amixer",
)

BLOCK_COMMENT = re.compile(r"/\*.*?\*/", re.S)
LINE_COMMENT = re.compile(r"//[^\n]*")
STRING_LITERAL = re.compile(r"""(?P<q>['"])(?:\\.|(?!(?P=q))[^\\])*(?P=q)""", re.S)


def strip_comments(source: str) -> str:
    """Remove comments without disturbing line structure or string contents."""
    out = []
    i = 0
    while i < len(source):
        string = STRING_LITERAL.match(source, i)
        if string:
            out.append(string.group(0))
            i = string.end()
            continue
        block = BLOCK_COMMENT.match(source, i)
        if block:
            out.append("\n" * block.group(0).count("\n"))
            i = block.end()
            continue
        line = LINE_COMMENT.match(source, i)
        if line:
            i = line.end()
            continue
        out.append(source[i])
        i += 1
    return "".join(out)


def qml_sources():
    for path in sorted(MODULES.rglob("*.qml")):
        yield path, strip_comments(path.read_text(encoding="utf-8", errors="replace"))


class BackendBoundaryTests(unittest.TestCase):
    def test_modules_do_not_run_system_commands(self):
        offenders = []
        for path, source in qml_sources():
            hits = sorted({c for c in SYSTEM_COMMANDS if re.search(rf"\b{c}\b", source)})
            if hits:
                offenders.append(f"{path.relative_to(ROOT)}: {', '.join(hits)}")
        self.assertEqual(
            offenders,
            [],
            "UI layers must call qs.services.* instead of running system commands:\n  "
            + "\n  ".join(offenders),
        )

    def test_services_remain_the_place_that_executes(self):
        """A guard that passes only because nothing executes anywhere is no guard."""
        services = ROOT / "services"
        executing = [
            path
            for path in services.rglob("*.qml")
            if re.search(r"\bProcess\s*\{", strip_comments(path.read_text(errors="replace")))
        ]
        self.assertTrue(executing, "expected qs.services to own the Process calls")


if __name__ == "__main__":
    unittest.main(verbosity=2)
