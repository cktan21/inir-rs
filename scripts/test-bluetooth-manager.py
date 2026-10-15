#!/usr/bin/env python3
"""Check reversible user autostart migration without touching the live session."""
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("manager", ROOT / "scripts/bluetooth-manager.py")
manager = importlib.util.module_from_spec(spec)
spec.loader.exec_module(manager)


class MigrationTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.config = Path(self.temporary.name) / "config"
        self.state = Path(self.temporary.name) / "state"
        self.settings = {"mode": "external", "launcher": "blueman-manager"}

    def test_create_override_and_remove_only_the_created_file_on_rollback(self):
        manager.disable_autostart(self.config, self.state, self.settings)
        target = self.config / "autostart/blueman.desktop"
        self.assertIn("Hidden=true", target.read_text())
        self.assertEqual(manager.restore_autostart(self.config, self.state)["launcher"], "blueman-manager")
        self.assertFalse(target.exists())

    def test_existing_override_is_restored_exactly(self):
        target = self.config / "autostart/blueman.desktop"
        original = "[Desktop Entry]\nName=My Bluetooth\nExec=blueman-applet\nHidden=false\n[Other]\nName=Keep this\n"
        manager.atomic_write(target, original)
        manager.disable_autostart(self.config, self.state, self.settings)
        manager.disable_autostart(self.config, self.state, {"mode": "integrated"})
        manager.restore_autostart(self.config, self.state)
        self.assertEqual(target.read_text(), original)

    def test_manual_changes_after_migration_are_preserved(self):
        manager.disable_autostart(self.config, self.state, self.settings)
        target = self.config / "autostart/blueman.desktop"
        manager.atomic_write(target, "user edit\n")
        with self.assertRaises(RuntimeError):
            manager.restore_autostart(self.config, self.state)
        self.assertEqual(target.read_text(), "user edit\n")
        self.assertTrue((self.state / "inir/bluetooth-migration.json").exists())

    def test_symlink_does_not_overwrite_its_target(self):
        target = self.config / "autostart/blueman.desktop"
        target.parent.mkdir(parents=True)
        outside = self.config / "original.desktop"
        manager.atomic_write(outside, "original\n")
        target.symlink_to(outside)
        with self.assertRaises(RuntimeError):
            manager.disable_autostart(self.config, self.state, self.settings)
        self.assertEqual(outside.read_text(), "original\n")

    def test_preview_then_enable_keeps_the_pre_preview_settings(self):
        manager.atomic_write(self.state / "inir/bluetooth-preview.json", json.dumps(self.settings))
        saved = manager.original_settings(self.state, {"mode": "integrated", "launcher": "custom-manager"})
        self.assertEqual(saved, self.settings)
        manager.disable_autostart(self.config, self.state, saved)
        self.assertEqual(manager.original_settings(self.state, {"mode": "integrated"}), self.settings)


if __name__ == "__main__":
    unittest.main()
