#!/usr/bin/env python3
"""Controlled Blueman handoff. Keeping the package permits an immediate rollback."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time


def atomic_write(path, content):
    path.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile(dir=path.parent, prefix=".inir-", delete=False) as stream:
        temporary = Path(stream.name)
        stream.write(content.encode())
        stream.flush()
        os.fsync(stream.fileno())
    try:
        os.replace(temporary, path)
    finally:
        temporary.unlink(missing_ok=True)


def disable_autostart(config_root, state_root, settings):
    target = config_root / "autostart/blueman.desktop"
    record = state_root / "inir/bluetooth-migration.json"
    if record.exists():
        return
    if target.is_symlink():
        raise RuntimeError("Blueman's user autostart entry is a symlink; preserve it and configure Hidden=true manually.")
    original = target.read_text() if target.exists() else None
    content = original if original is not None else "[Desktop Entry]\nType=Application\nName=Blueman\nExec=blueman-applet\n"
    lines = content.splitlines()
    section = None
    found = False
    output = []
    for line in lines:
        if line.startswith("["):
            if section == "[Desktop Entry]" and not found:
                output.append("Hidden=true")
                found = True
            section = line
        if section == "[Desktop Entry]" and line.startswith("Hidden="):
            line = "Hidden=true"
            found = True
        output.append(line)
    if not found:
        output.append("Hidden=true")
    replacement = "\n".join(output) + "\n"
    backup = dict(settings, originalAutostart=original,
                  installedHash=hashlib.sha256(replacement.encode()).hexdigest())
    atomic_write(record, json.dumps(backup, indent=2) + "\n")
    atomic_write(target, replacement)


def restore_autostart(config_root, state_root):
    target = config_root / "autostart/blueman.desktop"
    record = state_root / "inir/bluetooth-migration.json"
    if not record.exists():
        return None
    saved = json.loads(record.read_text())
    if target.is_symlink() or not target.exists() or hashlib.sha256(target.read_bytes()).hexdigest() != saved["installedHash"]:
        raise RuntimeError("Blueman's autostart entry changed after migration; its backup is preserved for manual restoration.")
    if saved["originalAutostart"] is None:
        target.unlink()
    else:
        atomic_write(target, saved["originalAutostart"])
    record.unlink()
    return saved


def original_settings(state_root, before):
    # Preview -> enable must retain the settings from before the first preview.
    for name in ("bluetooth-migration.json", "bluetooth-preview.json"):
        record = state_root / "inir" / name
        if record.exists():
            saved = json.loads(record.read_text())
            return {"mode": saved["mode"], "launcher": saved["launcher"]}
    return {"mode": before["mode"], "launcher": before["launcher"]}


def applet_processes(proc=Path("/proc")):
    found = []
    for path in proc.iterdir():
        if not path.name.isdigit():
            continue
        try:
            args = (path / "cmdline").read_bytes().split(b"\0")
            if any(arg in (b"/usr/bin/blueman-applet", b"/usr/bin/blueman-tray") for arg in args[:2]) and path.stat().st_uid == os.getuid():
                found.append(int(path.name))
        except OSError:
            pass
    return found


def ipc(config, method, *args):
    result = subprocess.run(["/usr/bin/qs", "-p", str(config), "ipc", "call", "bluetooth", method, *args],
                            capture_output=True, text=True, timeout=10)
    if result.returncode:
        raise RuntimeError(result.stderr.strip() or "The shell's Bluetooth IPC is unavailable")
    return result.stdout.strip()


def status(config):
    return json.loads(ipc(config, "status"))


def assert_idle_handoff():
    # Removing a Bluetooth agent during a transfer could interrupt user data.
    result = subprocess.run(["busctl", "--user", "--json=short", "call", "org.freedesktop.DBus",
                             "/org/freedesktop/DBus", "org.freedesktop.DBus", "NameHasOwner", "s", "org.bluez.obex"],
                            capture_output=True, text=True, timeout=5)
    if result.returncode:
        raise RuntimeError("Cannot verify the OBEX service; finish transfers and check the session bus before the handoff.")
    if json.loads(result.stdout)["data"][0]:
        snapshot = subprocess.run(["busctl", "--user", "--json=short", "call", "org.bluez.obex", "/",
                                   "org.freedesktop.DBus.ObjectManager", "GetManagedObjects"],
                                  capture_output=True, text=True, timeout=5)
        if snapshot.returncode:
            raise RuntimeError("Cannot verify active Bluetooth transfers; finish transfers before trying the handoff.")
        objects = json.loads(snapshot.stdout).get("data", [{}])[0]
        for interfaces in objects.values():
            transfer = interfaces.get("org.bluez.obex.Transfer1", {})
            if transfer.get("Status", {}).get("data") in ("queued", "active", "suspended"):
                raise RuntimeError("Finish or cancel active Bluetooth transfers before trying the handoff.")
    for interface in ("pan1", "btnap0"):
        path = Path("/sys/class/net") / interface / "operstate"
        if path.exists() and path.read_text().strip() == "up":
            raise RuntimeError("Stop existing Bluetooth internet sharing before trying the handoff.")


def stop_applets():
    for unit in ("app-blueman@autostart.service", "blueman-applet.service"):
        subprocess.run(["systemctl", "--user", "stop", unit], capture_output=True, timeout=10)
    for pid in applet_processes():
        try:
            os.kill(pid, signal.SIGTERM)
        except ProcessLookupError:
            pass
    deadline = time.monotonic() + 3
    while applet_processes() and time.monotonic() < deadline:
        time.sleep(.1)
    if applet_processes():
        raise RuntimeError("Blueman did not stop; it has not been forcibly terminated.")


def start_applet():
    if not applet_processes():
        subprocess.Popen(["blueman-applet"], stdin=subprocess.DEVNULL,
                         stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, start_new_session=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--config", type=Path, required=True)
    parser.add_argument("action", choices=("preview", "enable", "rollback"))
    parser.add_argument("--hardware-verified", action="store_true",
                        help="Confirm successful headphone/keyboard pairing, both file-transfer directions and both tethering directions")
    args = parser.parse_args()
    config_root = Path(os.environ.get("XDG_CONFIG_HOME", str(Path.home() / ".config")))
    state_root = Path(os.environ.get("XDG_STATE_HOME", str(Path.home() / ".local/state")))
    preview_record = state_root / "inir/bluetooth-preview.json"
    if args.action == "rollback":
        before = status(args.config)
        saved = restore_autostart(config_root, state_root) or original_settings(state_root, before)
        ipc(args.config, "restoreSettings", saved["mode"], saved["launcher"])
        preview_record.unlink(missing_ok=True)
        start_applet()
        print("Blueman restored. LibrePods and bluetoothd were not stopped.")
        return
    if args.action == "enable" and not args.hardware_verified:
        raise RuntimeError("Keep Blueman autostart until hardware checks pass. Use preview first; enable requires --hardware-verified.")
    before = status(args.config)
    if not before.get("available") or not before.get("ready"):
        raise RuntimeError("Build and load the native Bluetooth controller before the handoff.")
    assert_idle_handoff()
    was_running = bool(applet_processes())
    original = original_settings(state_root, before)
    had_migration = (state_root / "inir/bluetooth-migration.json").exists()
    try:
        if not preview_record.exists():
            atomic_write(preview_record, json.dumps(original, indent=2) + "\n")
        stop_applets()
        # Force a clean registration even when preview was already selected.
        ipc(args.config, "setMode", "external")
        ipc(args.config, "setMode", "integrated")
        deadline = time.monotonic() + 20
        while time.monotonic() < deadline:
            current = status(args.config)
            state = current.get("state", {})
            if current.get("ready") and current.get("mode") == "integrated" and all(state.get(k) for k in ("agentReady", "obexReady", "networkReady")):
                break
            time.sleep(.25)
        else:
            raise RuntimeError("Pairing, transfer or networking readiness failed: " + str(current.get("state", {}).get("error", "")))
        if args.action == "enable":
            disable_autostart(config_root, state_root, original)
            ipc(args.config, "migrateLauncher")
            print("Integrated Bluetooth enabled; Blueman's user autostart entry was backed up. Use rollback to restore it.")
        else:
            print("Integrated Bluetooth preview ready. Blueman autostart is unchanged; use rollback after testing, or enable --hardware-verified when checks pass.")
        if not status(args.config).get("locked"):
            ipc(args.config, "open")
    except BaseException:
        try:
            if not had_migration:
                restore_autostart(config_root, state_root)
            ipc(args.config, "restoreSettings", before["mode"], before["launcher"])
        finally:
            if was_running:
                start_applet()
        raise


if __name__ == "__main__":
    try:
        main()
    except (RuntimeError, OSError, subprocess.SubprocessError, ValueError) as error:
        print(str(error), file=sys.stderr)
        sys.exit(1)
