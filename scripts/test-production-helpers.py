#!/usr/bin/env python3
"""Synthetic compatibility checks; never access the real clipboard or devices."""
import html.entities
import importlib.util
import json
import os
import signal
from pathlib import Path
import subprocess
import time
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
HELPER = Path(os.environ.get("INIR_HELPER_BIN", ROOT / "rust/target/debug/inir-helper"))


def load(name, relative):
    spec = importlib.util.spec_from_file_location(name, ROOT / relative)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class InventoryTests(unittest.TestCase):
    def test_every_first_party_python_file_is_classified(self):
        manifest = json.loads((ROOT / "sdata/runtime-helpers.json").read_text())
        classified = {path for group in manifest["groups"] for path in group["sources"]}
        for paths in manifest["exceptions"].values():
            classified.update(paths)
        prefixes = manifest["exceptions"]["prefixes"]
        tracked = subprocess.check_output(["git", "ls-files", "*.py"], cwd=ROOT, text=True).splitlines()
        missing = [path for path in tracked if path not in classified and not any(path.startswith(p) for p in prefixes)]
        self.assertEqual(missing, [], "Unclassified Python must not disappear from migration scope")
        self.assertEqual(len(classified), sum(len(g["sources"]) for g in manifest["groups"]) + sum(len(v) for k, v in manifest["exceptions"].items() if k != "prefixes") + len(prefixes))


@unittest.skipUnless(HELPER.is_file(), "build inir-helper before compatibility checks")
class CompatibilityTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.clipboard = load("clipboard_reference", "scripts/clipboard-store.py")
        cls.sensor_fixture = load("sensor_fixtures", "scripts/test-detect-sensors.py")
        cls.sensors = load("sensor_reference", "scripts/detect_sensors.py")
        cls.usb = load("usb_reference", "scripts/devices/usb-snapshot.py")

    def command(self, *args, payload=b""):
        return subprocess.check_output([str(HELPER), *args], input=payload, timeout=10)

    def filter(self, data):
        if self.clipboard.is_browser_markup(data):
            expected = self.clipboard.strip_browser_markup(data.decode("utf-8", "replace")).encode()
        else:
            expected = data
        self.assertEqual(self.command("clipboard", "filter", payload=data), expected)

    def test_clipboard_bytes_and_browser_wrappers(self):
        examples = [b"", b"<div>source code</div>\r\n", bytes(range(256)), b"exact\n\n",
            b"<!--StartFragment--><p>Hello <b>world</b></p><div>next<br>line</div>",
            b"<meta http-equiv=\"content-type\" content=\"text/html\"><img src='https://example.test/i.png'>",
            b"<!--StartFragment--><SCRIPT>bad</SCRIPT><STYLE>bad</STYLE><p>yes</p>",
            b"<!--StartFragment--><style><script>ignored</style>kept</script>",
            b"<!--StartFragment--><script>unclosed<style>hidden</style>visible",
            b"<!--StartFragment-->\x1c\x1d\x1e\x1ftext\x1f",
            b"<!--StartFragment-->\xff\xfe", b"<!--StartFragment--><script>empty</script>"]
        for data in examples:
            with self.subTest(payload=data[:50]):
                self.filter(data)

    def test_all_html5_entities_and_invalid_numeric_references(self):
        markup = "<!--StartFragment-->" + "|".join("&" + key for key in sorted(html.entities.html5))
        markup += "|&notit;|&#x80;|&#0;|&#xD800;|&#1;|&#xFFFF;|&#999999999999999999999999;"
        self.filter(markup.encode())

    def test_sensor_discovery_matches_reference(self):
        fixture = self.sensor_fixture.SensorFixture()
        self.addCleanup(fixture.close)
        fixture.hwmon(0, "k10temp", ("Tctl", "Tdie"))
        amd = fixture.card(0, "0x1002", busy=True)
        nvidia = fixture.card(1, "0x10de", runtime="suspended")
        fixture.link_gpu_hwmon(1, amd, labels=("Edge",))
        fixture.link_gpu_hwmon(2, nvidia, name="nvidia", labels=("Hotspot", "Memory"))
        result = self.command("system", "sensors", str(fixture.root)).decode()
        actual = dict(line.split(":", 1) for line in result.splitlines())
        self.assertEqual(actual, self.sensors.detect(fixture.root))

    def test_usb_snapshot_matches_reference(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for name, vendor, product, interface in [
                ("1-1", "Example Technology Inc.", "Wireless Receiver", "030101"),
                ("1-2", "Sony", "DualSense Controller", "030000"),
                ("1-3", "Other", "USB Device", "090000"),
                ("1-4", "Storage", "Disk", "080650"),
            ]:
                path = root / name
                path.mkdir()
                for key, value in {"idVendor": "abcd", "idProduct": "1234", "manufacturer": vendor, "product": product, "bDeviceClass": "00"}.items():
                    (path / key).write_text(value)
                sub = path / (name + ":1.0")
                sub.mkdir()
                for index, key in enumerate(("bInterfaceClass", "bInterfaceSubClass", "bInterfaceProtocol")):
                    (sub / key).write_text(interface[index*2:index*2+2])
            import contextlib
            import io
            original = self.usb.ROOT
            self.usb.ROOT = directory
            try:
                with contextlib.redirect_stdout(io.StringIO()) as stream:
                    self.usb.main()
                expected = json.loads(stream.getvalue())
            finally:
                self.usb.ROOT = original
            self.assertEqual(json.loads(self.command("devices", "snapshot", directory)), expected)

    def test_job_envelope_and_validation(self):
        result = json.loads(self.command("job", "system.networkCounters", payload=b'{"interfaces":["../escape"]}'))
        self.assertFalse(result["success"])
        self.assertEqual(result["error"], "Invalid network interface name")
        result = json.loads(self.command("job", "lyrics.fetch", payload=b'{"requestId":"empty"}'))
        self.assertEqual(result["result"], {"status": "no_info", "requestId": "empty"})

    def test_native_keyboard_layout_labels_match_xkb_reference(self):
        for description in ["English (US)", "English (US, intl., with dead keys)", "German", "Arabic", "Hebrew", "Russian", "not-a-real-layout"]:
            with self.subTest(layout=description):
                reference = subprocess.run(["python3", str(ROOT / "scripts/osk-layout-labels.py"), description], capture_output=True, timeout=10)
                actual = json.loads(self.command("job", "input.layoutLabels", payload=json.dumps({"description": description}).encode()))
                self.assertTrue(actual["success"], actual)
                self.assertEqual(actual["result"], json.loads(reference.stdout))

    def test_development_watcher_debounces_and_exits_without_reloading_a_real_shell(self):
        with tempfile.TemporaryDirectory(prefix="inir-dev-fixture-") as directory:
            root = Path(directory)
            (root / "services").mkdir()
            (root / "modules/lazy").mkdir(parents=True)
            (root / "bin").mkdir()
            (root / "shell.qml").write_text("import qs.services\nQtObject {}\n")
            (root / "services/Used.qml").write_text("QtObject {}\n")
            lazy = root / "modules/lazy/Lazy.qml"
            lazy.write_text("Item {}\n")
            stub = root / "bin/qs"
            stub.write_text('#!/bin/sh\nprintf "%s\\n" "$*" >> "$INIR_TEST_RELOAD_LOG"\n')
            stub.chmod(0o755)
            marker = root / "reloads"
            environment = dict(os.environ, PATH=str(root / "bin") + ":" + os.environ["PATH"], INIR_TEST_RELOAD_LOG=str(marker))
            process = subprocess.Popen([str(HELPER), "dev", "watch", str(root), str(os.getpid())], env=environment, stderr=subprocess.PIPE)
            try:
                time.sleep(0.2)
                lazy.write_text("Item { property int value: 1 }\n")
                lazy.write_text("Item { property int value: 2 }\n")
                deadline = time.monotonic() + 5
                while not marker.exists() and time.monotonic() < deadline:
                    self.assertIsNone(process.poll())
                    time.sleep(0.05)
                self.assertTrue(marker.exists(), "Unreachable QML did not request a reload")
                self.assertEqual(marker.read_text().splitlines(), [f"-p {root} ipc call dev reload"])
                # Identical writes must not generate a second reload after cooldown.
                lazy.write_text("Item { property int value: 2 }\n")
                time.sleep(3)
                self.assertEqual(len(marker.read_text().splitlines()), 1)
                reached = json.loads(self.command("dev", "reach", str(root)))
                self.assertIn(str(root / "services/Used.qml"), reached)
                self.assertNotIn(str(lazy), reached)
            finally:
                process.terminate()
                process.communicate(timeout=2)

    def test_clipboard_migration_only_changes_the_managed_watcher(self):
        with tempfile.TemporaryDirectory(prefix="inir-migration-fixture-") as directory:
            root = Path(directory)
            runtime = root / "quickshell/inir/scripts"
            runtime.mkdir(parents=True)
            (runtime / "inir-helper").symlink_to(HELPER)
            startup = root / "50-startup.kdl"
            original = '\n'.join([
                '// user comment',
                'spawn-at-startup "bash" "-c" "wl-paste --no-newline --type text --watch ~/.config/quickshell/inir/scripts/clipboard-store.py &"',
                'spawn-at-startup "bash" "-c" "wl-paste --type text --watch /custom/clipboard-store.py &"',
                'spawn-at-startup "bash" "-c" "wl-paste --type image --watch cliphist store &"',
            ]) + '\n'
            startup.write_text(original)
            command = ['bash', '-c', 'set -euo pipefail\nsource "$1"\n_rust_clipboard_startup="$2"\nmigration_check\nmigration_apply\n! migration_check\n',
                'fixture', str(ROOT / "sdata/migrations/043-rust-clipboard-helper.sh"), str(startup)]
            subprocess.run(command, check=True, env=dict(os.environ, XDG_CONFIG_HOME=str(root)), timeout=10)
            expected = original.replace("~/.config/quickshell/inir/scripts/clipboard-store.py", "~/.config/quickshell/inir/scripts/inir-helper clipboard store")
            self.assertEqual(startup.read_text(), expected)

    def test_watcher_does_not_survive_its_owner(self):
        with tempfile.TemporaryDirectory(prefix="inir-owner-fixture-") as directory:
            root = Path(directory)
            (root / "services").mkdir()
            (root / "modules").mkdir()
            (root / "shell.qml").write_text("QtObject {}\n")
            parent = subprocess.Popen(['bash', '-c', '"$1" dev watch "$2" "$$" &\nprintf "%s\\n" "$!"\nwait\n',
                'fixture', str(HELPER), str(root)], stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
            child = int(parent.stdout.readline())
            try:
                time.sleep(0.1)
                parent.kill()
                # The helper inherits stdout. EOF cannot arrive until it also
                # closes its fds, even though only the fixture parent was killed.
                parent.communicate(timeout=2)
                state = Path(f"/proc/{child}/stat")
                try:
                    zombie = state.read_text().rsplit(") ", 1)[1].startswith("Z ")
                except (FileNotFoundError, ProcessLookupError):
                    zombie = True  # already reaped
                self.assertTrue(zombie, "Watcher remained alive after its owner exited")
            finally:
                if parent.poll() is None:
                    parent.kill()
                try:
                    arguments = Path(f"/proc/{child}/cmdline").read_bytes().split(b"\0")
                    if os.fsencode(str(root)) in arguments:
                        os.kill(child, signal.SIGTERM)
                except (FileNotFoundError, ProcessLookupError):
                    pass
                parent.communicate(timeout=2)


if __name__ == "__main__":
    unittest.main(verbosity=2)
