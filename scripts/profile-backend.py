#!/usr/bin/env python3
"""Repeatable per-shell procfs baseline. Unsupported metrics remain unavailable."""
import argparse
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import shutil
import statistics
import sys
import time


def process_stat(pid, proc=Path("/proc")):
    # comm may contain spaces and parentheses; fields begin after the final ')'.
    text = (proc / str(pid) / "stat").read_text()
    fields = text[text.rfind(")") + 2:].split()
    return {"ppid": int(fields[1]), "ticks": int(fields[11]) + int(fields[12]),
            "start": int(fields[19]), "rss": int(fields[21]) * os.sysconf("SC_PAGE_SIZE")}


def context_switches(pid):
    total = 0
    for path in Path(f"/proc/{pid}/task").glob("*/status"):
        try:
            for line in path.read_text().splitlines():
                if line.startswith(("voluntary_ctxt_switches:", "nonvoluntary_ctxt_switches:")):
                    total += int(line.split()[1])
        except (OSError, ValueError):
            continue
    return total


def gpu_engines(pid):
    # Observe only DRM clients already open by the shell; never wake a parked GPU.
    engines = {}
    for path in Path(f"/proc/{pid}/fdinfo").glob("*"):
        try:
            lines = dict(line.split(":", 1) for line in path.read_text().splitlines() if ":" in line)
            client = lines.get("drm-client-id")
            device = lines.get("drm-pdev", "unknown")
            if client is None:
                continue
            for key, value in lines.items():
                if key.startswith("drm-engine-") and value.strip().endswith(" ns"):
                    engines[(device.strip(), client.strip(), key)] = int(value.split()[0])
        except (OSError, ValueError):
            continue
    return engines


def child_processes(pid):
    processes = {}
    for path in Path("/proc").iterdir():
        if not path.name.isdigit():
            continue
        try:
            processes[int(path.name)] = process_stat(int(path.name))
        except (OSError, ValueError, IndexError):
            continue
    children, parents = {}, {pid}
    while True:
        found = {child for child, state in processes.items() if state["ppid"] in parents} - parents
        if not found:
            return children
        for child in found:
            children[child] = processes[child]["start"]
        parents |= found


def find_shell():
    candidates = []
    for path in Path("/proc").iterdir():
        if not path.name.isdigit():
            continue
        try:
            args = (path / "cmdline").read_bytes().split(b"\0")
            if Path(os.fsdecode(args[0])).name not in {"qs", "quickshell"}:
                continue
            if any(b"inir" in arg for arg in args[1:]) and not any(b"settings.qml" in arg.lower() for arg in args):
                candidates.append(int(path.name))
        except OSError:
            continue
    if len(candidates) > 1:
        raise ValueError("multiple iNiR shells found; select one with --pid")
    return candidates[0] if candidates else None


def percentile(values, fraction):
    values = sorted(values)
    point = (len(values) - 1) * fraction
    lower = int(point)
    return values[lower] + (values[min(lower + 1, len(values)-1)] - values[lower]) * (point - lower)


def capture(pid, duration, interval):
    clock_ticks = os.sysconf("SC_CLK_TCK")
    previous = process_stat(pid)
    start_identity = previous["start"]
    previous_contexts = context_switches(pid)
    previous_gpu = gpu_engines(pid)
    started = previous_time = time.monotonic()
    initial_children = set(child_processes(pid).items())
    seen = set(initial_children)
    rows = []
    while time.monotonic() - started < duration:
        time.sleep(min(interval, max(0, duration - (time.monotonic() - started))))
        current = process_stat(pid)
        if current["start"] != start_identity:
            raise ValueError("shell exited and its PID was reused during capture")
        now = time.monotonic()
        elapsed = now - previous_time
        contexts = context_switches(pid)
        gpu = gpu_engines(pid)
        gpu_values = [(value - previous_gpu[key]) / (elapsed * 1e9) * 100
                      for key, value in gpu.items() if key in previous_gpu and value >= previous_gpu[key]]
        children = child_processes(pid)
        seen |= set(children.items())
        rows.append({"elapsedSeconds": now-started,
                     "cpuPercent": max(0, current["ticks"]-previous["ticks"]) / clock_ticks / elapsed * 100,
                     "rssBytes": current["rss"],
                     # Context switches are a proxy, not scheduler wakeups.
                     "contextSwitchesPerSecond": max(0, contexts-previous_contexts) / elapsed,
                     "gpuBusiestEnginePercent": max(gpu_values) if gpu_values else None,
                     "activeChildProcesses": len(children)})
        previous, previous_time, previous_contexts, previous_gpu = current, now, contexts, gpu
    return {"durationSeconds": previous_time-started,
            "cpuMeanPercent": statistics.fmean(row["cpuPercent"] for row in rows),
            "cpuMaxPercent": max(row["cpuPercent"] for row in rows),
            "rssMeanBytes": statistics.fmean(row["rssBytes"] for row in rows),
            "rssMaxBytes": max(row["rssBytes"] for row in rows),
            "observedChildStarts": len(seen-initial_children),
            "samples": rows}


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--pid", type=int, help="shell PID (required if detection is ambiguous)")
    parser.add_argument("--layout", choices=["ii", "iris", "waffle"], required=True)
    parser.add_argument("--scenario", choices=["idle", "settings", "network", "windows"], default="idle")
    parser.add_argument("--duration", type=float, default=10)
    parser.add_argument("--interval", type=float, default=1)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--frame-times", type=Path, help="JSON array of frame times in milliseconds from a Qt profiler capture")
    args = parser.parse_args(argv)
    if args.duration <= 0 or args.interval < 0.05 or args.interval > args.duration:
        parser.error("duration must be positive and interval must be between 0.05 and duration")
    if args.pid is not None and args.pid <= 0:
        parser.error("pid must be positive")
    report = {"formatVersion": 1, "capturedAt": datetime.now(timezone.utc).isoformat(),
              "layout": args.layout, "scenario": args.scenario,
              "backendRequested": "rust" if os.environ.get("INIR_RUST_BACKEND") == "1" else "qml",
              "tools": {name: bool(shutil.which(name)) for name in ["qs", "quickshell", "niri", "pidstat", "perf", "strace"]},
              "available": False, "metrics": None,
              "unavailableMetrics": {"wakeupsPerSecond": "Requires scheduler tracing; context switches are reported separately.",
                                     "forksPerMinute": "Short-lived processes require strace -f -e trace=clone,clone3,execve.",
                                     "coldStartupMilliseconds": "Requires shell boot markers from a cold start.",
                                     "settingsOpenMilliseconds": "Requires a UI interaction trace.",
                                     "qmlObjectCount": "Requires the QML profiler."}}
    status = 0
    try:
        pid = args.pid or find_shell()
        if pid is None:
            raise ValueError("no running iNiR Quickshell process; capture this layout in a live Niri session")
        report["pid"] = pid
        try:
            environment = Path(f"/proc/{pid}/environ").read_bytes().split(b"\0")
            report["backendRequested"] = "rust" if b"INIR_RUST_BACKEND=1" in environment else "qml"
        except OSError:
            report["backendRequested"] = "unknown"
        report["metrics"] = capture(pid, args.duration, args.interval)
        report["available"] = True
        if args.frame_times:
            frames = json.loads(args.frame_times.read_text())
            if not frames or not all(isinstance(x, (float, int)) and not isinstance(x, bool) and x >= 0 and x < float("inf") for x in frames):
                raise ValueError("frame-times must contain a nonempty array of finite nonnegative numbers")
            report["metrics"]["frameTimeMilliseconds"] = {key: percentile(frames, p) for key, p in [("p50", .5), ("p95", .95), ("p99", .99)]}
        else:
            report["unavailableMetrics"]["frameTimes"] = "Supply --frame-times from a Qt profiler capture."
    except (OSError, ValueError, IndexError) as error:
        report["error"] = str(error)
        status = 2
    output = json.dumps(report, indent=2) + "\n"
    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(output)
    sys.stdout.write(output)
    return status


if __name__ == "__main__":
    sys.exit(main())
