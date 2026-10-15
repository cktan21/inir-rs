#!/usr/bin/env python3
"""Read-only per-process/thread Bluetooth CPU sample (100% = one full core)."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import time


def snapshot():
    rows = {}
    for entry in Path("/proc").iterdir():
        if not entry.name.isdigit():
            continue
        try:
            name = (entry / "comm").read_text().strip()
            if name not in ("qs", "bluetoothd", "blueman-applet", "blueman-tray", "librepods"):
                continue
            entries = [(entry, name)]
            if name == "qs":
                for task in (entry / "task").iterdir():
                    task_name = (task / "comm").read_text().strip()
                    if task_name in ("inir-core", "tokio-rt-worker"):
                        entries.append((task, name + "/" + task_name))
            for path, label in entries:
                fields = (path / "stat").read_text().rsplit(") ", 1)[1].split()
                # Original stat fields 14/15, 20 and 22; fields starts at 3.
                rows[str(path)] = {"pid": int(path.name), "name": label,
                                  "ticks": int(fields[11]) + int(fields[12]),
                                  "threads": int(fields[17]), "start": fields[19]}
        except (OSError, ValueError, IndexError):
            continue
    return rows


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--duration", type=float, default=5)
    parser.add_argument("--config", type=Path, default=Path(__file__).resolve().parents[1])
    args = parser.parse_args()
    if not 0 < args.duration <= 30:
        parser.error("duration must be between 0 and 30 seconds")
    begin = time.monotonic()
    before = snapshot()
    time.sleep(args.duration)
    after = snapshot()
    seconds = time.monotonic() - begin
    ticks_per_second = os.sysconf("SC_CLK_TCK")
    results = []
    for key, row in after.items():
        previous = before.get(key)
        if previous and previous["start"] == row["start"]:
            results.append({"pid": row["pid"], "name": row["name"], "threads": row["threads"],
                            "cpuPercentOneCore": round(100 * (row["ticks"] - previous["ticks"]) / ticks_per_second / seconds, 3)})
    status = subprocess.run(["qs", "-p", str(args.config), "ipc", "call", "bluetooth", "status"],
                            capture_output=True, text=True, timeout=10)
    current = json.loads(status.stdout) if status.returncode == 0 else {}
    print(json.dumps({"durationSeconds": round(seconds, 3), "mode": current.get("mode"),
                      "globalDiscovering": current.get("discovering"), "leases": current.get("leases"),
                      "scans": current.get("state", {}).get("scans", []), "processes": results}, indent=2))


if __name__ == "__main__":
    main()
