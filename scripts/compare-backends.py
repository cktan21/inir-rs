#!/usr/bin/env python3
"""Compare exclusive native/QML ownership in one live shell at a time.

Temporarily stops inir.service, reuses the original shell environment, and
restores the service and focused workspace in a finally block. Never accepts
a Rust-requested shell that fell back to QML as a native measurement.

Guards: refuses to start or continue while the session is locked (a lock
surface changes what is measured and blocks workspace focus), and removes
orphaned swayidle watchers before and after each run so stale lock and
brightness timers from earlier shell launches cannot fire mid-capture.

Scenarios per run: idle, workspace switching (two focus commands per second),
and Wi-Fi rescans (one NetworkManager rescan every five seconds, issued by this
script for both modes alike). Modes alternate in ABBA order across rounds.
"""
import argparse
import fcntl
import gzip
from datetime import datetime, timezone
import importlib.util
import json
import os
from pathlib import Path
import select
import signal
import shutil
import socket
import statistics
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("profile", ROOT / "scripts/profile-backend.py")
profile = importlib.util.module_from_spec(spec)
spec.loader.exec_module(profile)


def command(*args):
    return subprocess.check_output(args, text=True, timeout=20).strip()


def ipc(pid, target, method, *arguments):
    return command("/usr/bin/qs", "ipc", "--pid", str(pid), "call", target, method, *arguments)


def shell_pids():
    found = []
    for path in Path("/proc").iterdir():
        if not path.name.isdigit():
            continue
        try:
            args = (path / "cmdline").read_bytes().split(b"\0")
            if Path(os.fsdecode(args[0])).name in {"qs", "quickshell"} and b"ipc" not in args and b"list" not in args:
                found.append(int(path.name))
        except (OSError, IndexError):
            pass
    return found


def ownership(pid, mode):
    state = json.loads(ipc(pid, "backend", "ownership"))
    # Bluetooth control is intentionally shared by both modes. The plugin may
    # therefore be mapped even when all benchmarked domains belong to QML.
    bluetooth = json.loads(ipc(pid, "bluetooth", "status"))
    state["commonBluetooth"] = {key: bluetooth.get(key) for key in ("mode", "available", "ready")}
    native = mode == "rust"
    assert state["typedBridge"] == native, state
    assert state["requested"] == native and state["available"] == native, state
    assert state["desktopActive"] == native and not state["nativePowerReady"], state
    assert state["idleInhibited"], state
    assert not state["gameMode"], state
    for domain in ("niri", "network", "brightness"):
        assert state[domain]["owner"] == mode and state[domain]["nativeReady"] == native, state
    if native:
        assert not state["niri"]["fallbackLoaded"] and state["niri"]["fallback"] is None, state
        assert not state["network"]["monitorRunning"] and state["network"]["acquisitionProcessesRunning"] == 0, state
        assert not state["brightness"]["backlightDetectRunning"], state
        assert all(not m["isDdc"] and not any(m[k] for k in ("initRunning", "readbackRunning", "writeRunning"))
                   for m in state["brightness"]["monitors"]), state
        if "keyboard" in (state.get("selectedNativeDomains") or "").split(",") and state.get("keyboard", {}).get("wanted"):
            assert state["keyboard"]["owner"] == "rust" and state["keyboard"]["nativeReady"], state
            assert not state["keyboard"]["probeRunning"] and not state["keyboard"]["monitorRunning"], state
    else:
        assert state["niri"]["fallbackLoaded"] and state["niri"]["fallback"]["eventSocketConnected"], state
        assert state["niri"]["nativeApplyCalls"] == 0 and state["network"]["monitorRunning"], state
        plugin_loaded = "libqs_services_native.so" in Path(f"/proc/{pid}/maps").read_text()
        assert not plugin_loaded or state["commonBluetooth"]["available"], state
    assert shell_pids() == [pid], f"Multiple shells: {shell_pids()}"
    return state


def locked_sessions():
    ids = [line.split()[0] for line in command("loginctl", "list-sessions", "--no-legend").splitlines() if line.strip()]
    return [sid for sid in ids if command("loginctl", "show-session", sid, "-p", "LockedHint", "--value") == "yes"]


# Helpers a shell starts for itself. A child shell that exits leaves these behind
# (re-parented to the user manager); the keyboard daemon alone leaked ~15 MiB each.
SHELL_HELPERS = ("swayidle -w", "keyboard_lock_state_daemon.py", "udevadm monitor --udev", "nmcli monitor")
OWNED_HELPERS = {}
# Start times of shells this test stopped. A shell can (re)spawn a helper during
# shutdown, after its helpers were recorded (Network.qml restarts `nmcli monitor`
# when it exits), so a matching helper that started after such a shell is its own.
OWNED_SHELL_STARTS = []


def remember_shell_helpers(pid):
    try:
        OWNED_SHELL_STARTS.append(profile.process_stat(pid)["start"])
    except (OSError, ValueError, IndexError):
        pass
    for child, start in profile.child_processes(pid).items():
        try:
            cmdline = Path(f"/proc/{child}/cmdline").read_bytes().replace(b"\0", b" ").decode(errors="replace")
            if any(helper in cmdline for helper in SHELL_HELPERS):
                OWNED_HELPERS[child] = start
        except OSError:
            pass


def orphan_swayidle():
    """Shell helpers whose launching shell has exited (re-parented to the user manager)."""
    found = []
    for path in Path("/proc").iterdir():
        if not path.name.isdigit():
            continue
        try:
            cmdline = (path / "cmdline").read_bytes().replace(b"\0", b" ").decode(errors="replace")
            if not any(helper in cmdline for helper in SHELL_HELPERS):
                continue
            ppid = int((path / "stat").read_text().rsplit(")", 1)[1].split()[1])
            if ppid > 1 and Path(f"/proc/{ppid}/comm").read_text().strip() == "systemd":
                found.append(int(path.name))
        except (OSError, ValueError, IndexError):
            continue
    return found


def _owned_helper(pid):
    start = profile.process_stat(pid)["start"]
    return OWNED_HELPERS.get(pid) == start or any(start >= shell for shell in OWNED_SHELL_STARTS)


def remove_orphan_swayidle(timeout=3.0):
    # Only terminate helpers of a shell this test stopped; an unrelated user
    # monitor/watcher is never our target and still fails the check. Helpers get
    # SIGTERM, then SIGKILL for the final second, instead of one fixed 0.5 s wait.
    deadline = time.monotonic() + timeout
    while True:
        orphans = orphan_swayidle()
        if not orphans:
            return True
        if time.monotonic() >= deadline:
            return False
        signal_number = 9 if time.monotonic() >= deadline - 1 else 15
        for pid in orphans:
            try:
                if _owned_helper(pid):
                    os.kill(pid, signal_number)
            except (OSError, ValueError, IndexError):
                pass
        time.sleep(0.2)


def require_unlocked(context):
    locked = locked_sessions()
    if locked:
        raise RuntimeError(f"Session {locked} is locked ({context}); unlock it and rerun")


def memory(pid):
    fields = {}
    for line in Path(f"/proc/{pid}/smaps_rollup").read_text().splitlines():
        if ":" in line:
            key, value = line.split(":", 1)
            if value.strip().endswith("kB"):
                fields[key] = int(value.split()[0]) * 1024
    return {"rssBytes": fields["Rss"], "pssBytes": fields["Pss"],
            "privateBytes": fields.get("Private_Clean", 0) + fields.get("Private_Dirty", 0),
            "swapBytes": fields.get("Swap", 0)}


def gpu_memory(pid):
    clients = {}
    for path in Path(f"/proc/{pid}/fdinfo").glob("*"):
        try:
            values = dict(line.split(":", 1) for line in path.read_text().splitlines() if ":" in line)
            if "drm-client-id" not in values:
                continue
            key = (values.get("drm-pdev", "unknown").strip(), values["drm-client-id"].strip())
            row = {}
            for region in ("vram", "gtt"):
                value = values.get("drm-memory-" + region)
                if value and value.strip().endswith("KiB"):
                    row[region] = int(value.split()[0]) * 1024
            clients[key] = row
        except (OSError, ValueError):
            continue
    return {"gpuResidentVramBytes": sum(c.get("vram", 0) for c in clients.values()) if clients else None,
            "gpuResidentGttBytes": sum(c.get("gtt", 0) for c in clients.values()) if clients else None}


def main_ticks(pid):
    return profile.process_stat(pid, Path(f"/proc/{pid}/task"))["ticks"]


def thread_counters(pid):
    counters = {}
    for path in Path(f"/proc/{pid}/task").iterdir():
        try:
            state = profile.process_stat(int(path.name), path.parent)
            counters[path.name] = {"name": (path / "comm").read_text().strip(),
                                   "ticks": state["ticks"], "start": state["start"]}
        except (OSError, ValueError):
            continue
    return counters


def tree_ticks(pid):
    def total(process):
        text = Path(f"/proc/{process}/stat").read_text()
        fields = text[text.rfind(")") + 2:].split()
        return sum(int(fields[i]) for i in (11, 12, 13, 14))
    result = total(pid)
    for child in profile.child_processes(pid):
        try:
            result += total(child)
        except OSError:
            pass
    return result


def sample(pid):
    stat = profile.process_stat(pid)
    shell_memory = memory(pid)
    children = profile.child_processes(pid)
    helper_pss = 0
    for child in children:
        try:
            helper_pss += memory(child)["pssBytes"]
        except (OSError, KeyError):
            pass
    displays = {path.parent.name: path.read_text().strip() for path in Path("/sys/class/drm").glob("card*-*/dpms")
                if (path.parent / "status").read_text().strip() == "connected"}
    if any(state != "On" for state in displays.values()):
        raise RuntimeError(f"Display blanked during capture: {displays}")
    return {"time": time.monotonic(), "ticks": stat["ticks"], "mainTicks": main_ticks(pid),
            "treeTicks": tree_ticks(pid), "contexts": profile.context_switches(pid),
            "engines": profile.gpu_engines(pid), **shell_memory, **gpu_memory(pid),
            "helperPssBytes": helper_pss,
            "shellAndHelperPssBytes": shell_memory["pssBytes"] + helper_pss,
            "activeChildren": len(children),
            "threadCounters": thread_counters(pid),
            "threads": len(list(Path(f"/proc/{pid}/task").iterdir()))}


def niri_request(stream, reader, request):
    stream.sendall((json.dumps(request) + "\n").encode())
    reply = json.loads(reader.readline())
    if "Err" in reply:
        raise RuntimeError(reply["Err"])
    return reply["Ok"]


def begin_qml_trace(profiler):
    # flush already cleared the previous trace. Send one command per write:
    # Qt's console can consume/discard additional lines from a pipe read.
    profiler.stdin.write("r\n")
    profiler.stdin.flush()
    time.sleep(0.2)


def finish_qml_trace(profiler, path):
    profiler.stdin.write(f"f {path.resolve()}\n")
    profiler.stdin.flush()
    for _ in range(100):
        if path.exists() and path.stat().st_size:
            with path.open("rb") as source:
                source.seek(max(0, path.stat().st_size - 64))
                complete = source.read().rstrip().endswith(b"</trace>")
            if complete:
                archived = path.with_suffix(path.suffix + ".gz")
                with path.open("rb") as source, gzip.open(archived, "wb") as target:
                    shutil.copyfileobj(source, target)
                path.unlink()
                return archived.name
        time.sleep(0.1)
    raise RuntimeError("QML profiler did not write the trace")


def measure(pid, duration, action=None, interval=0.5, observer=None):
    hz = os.sysconf("SC_CLK_TCK")
    previous = sample(pid)
    remember_shell_helpers(pid)
    started = previous["time"]
    next_sample, next_action, next_spawn_check = started + 1, started, started
    rows, actions = [], 0
    thread_totals = {}
    observations = []
    if observer:
        observations.append({"elapsedSeconds": 0, **observer()})
    seen_children, spawns = set(), {}
    while True:
        now = time.monotonic()
        finished = now >= started + duration
        if action and now >= next_action and not finished:
            action(actions)
            actions += 1
            next_action += interval
        if now >= next_spawn_check:
            remember_shell_helpers(pid)
            # Sampled spawn count: helpers that start and exit between checks are missed.
            for child, start in profile.child_processes(pid).items():
                if (child, start) not in seen_children:
                    seen_children.add((child, start))
                    try:
                        name = Path(f"/proc/{child}/comm").read_text().strip()
                    except OSError:
                        name = "exited"
                    spawns[name] = spawns.get(name, 0) + 1
            next_spawn_check = now + 0.5
        if now >= next_sample or finished:
            current = sample(pid)
            elapsed = current["time"] - previous["time"]
            for tid, counter in current["threadCounters"].items():
                old = previous["threadCounters"].get(tid)
                if old and old["start"] == counter["start"]:
                    key = (tid, counter["start"])
                    total = thread_totals.setdefault(key, {"tid": int(tid), "name": counter["name"], "ticks": 0})
                    total["ticks"] += max(0, counter["ticks"] - old["ticks"])
            engines = {"/".join(k): (v - previous["engines"][k]) / elapsed / 1e7
                       for k, v in current["engines"].items() if k in previous["engines"] and v >= previous["engines"][k]}
            row = {k: current[k] for k in ("rssBytes", "pssBytes", "privateBytes", "swapBytes", "helperPssBytes", "shellAndHelperPssBytes",
                   "gpuResidentVramBytes", "gpuResidentGttBytes", "activeChildren", "threads")}
            row.update({"elapsedSeconds": current["time"] - started, "intervalSeconds": elapsed,
                        "cpuPercent": (current["ticks"] - previous["ticks"]) / hz / elapsed * 100,
                        "mainThreadCpuPercent": (current["mainTicks"] - previous["mainTicks"]) / hz / elapsed * 100,
                        "processTreeCpuPercent": max(0, current["treeTicks"] - previous["treeTicks"]) / hz / elapsed * 100,
                        "contextSwitchesPerSecond": max(0, current["contexts"] - previous["contexts"]) / elapsed,
                        "gpuEnginePercent": engines, "gpuBusiestEnginePercent": max(engines.values()) if engines else None})
            rows.append(row)
            if observer:
                observations.append({"elapsedSeconds": current["time"] - started, **observer()})
            previous = current
            next_sample += 1
        if finished:
            break
        time.sleep(min(0.05, max(0, started + duration - time.monotonic())))
    summary = {}
    for key in rows[0]:
        if key in ("elapsedSeconds", "intervalSeconds", "gpuEnginePercent"):
            continue
        values = [r[key] for r in rows if r[key] is not None]
        summary[key] = {"mean": statistics.fmean(values), "min": min(values), "max": max(values)} if values else None
    for total in thread_totals.values():
        total["cpuSeconds"] = total["ticks"] / hz
        total["cpuPercent"] = total["cpuSeconds"] / (previous["time"] - started) * 100
    return {"summary": summary, "samples": rows, "actions": actions,
            "threadCpu": sorted(thread_totals.values(), key=lambda row: row["ticks"], reverse=True),
            "compositorObservations": observations,
            "threadCpuNotes": "Matched thread identities between samples; short-lived or newly observed threads can be undercounted.",
            "durationSeconds": time.monotonic() - started, "spawnsByName": spawns}


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--duration", type=float, default=30)
    parser.add_argument("--warmup", type=float, default=20)
    parser.add_argument("--rounds", type=int, default=5)
    parser.add_argument("--native-domains", default="network,brightness,niri,keyboard",
                        help="Native domain selection; keyboard removes the lock-indicator Python helper")
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--title-churn", action="store_true", help="Add a real static-content window with 20 title changes/second")
    parser.add_argument("--fixture-only", action="store_true", help="Add the static window without a title workload")
    parser.add_argument("--fixture-workspace", choices=("original", "alternate"), default="original",
                        help="Place the title fixture on the other workspace to test populated-to-populated switching")
    parser.add_argument("--workspace-only", action="store_true", help="Skip idle and Wi-Fi scenarios")
    parser.add_argument("--workspace-variants", default="baseline", help="Comma-separated transient presentation ablations")
    parser.add_argument("--qml-profile", action="store_true", help="Separate instrumented run with per-variant Qt QML traces")
    parser.add_argument("--media-fixture", action="store_true", help="Own a silent MPRIS track for controlled presentation activity; never pauses real players")
    args = parser.parse_args()
    def terminate_benchmark(signum, _frame):
        # A terminated command must still restore the user's managed shell.
        # SIGKILL cannot be caught; its interrupted artifacts are never accepted.
        raise SystemExit(128 + signum)
    signal.signal(signal.SIGTERM, terminate_benchmark)
    if args.fixture_workspace == "alternate" and not (args.title_churn or args.fixture_only):
        parser.error("--fixture-workspace alternate requires a fixture")
    variants = args.workspace_variants.split(",")
    if not variants or any(v not in {"baseline", "no-animations", "no-workspace-animation", "no-effects", "no-wallpaper-blur", "no-media-widget", "no-blur-or-media-widget", "bar-hidden"} for v in variants):
        parser.error("unknown workspace variant")
    if args.duration < 5 or args.warmup < 5 or args.rounds < 1:
        parser.error("duration/warmup must be at least 5 seconds; rounds must be positive")
    # Independent coding sessions must not restart the shell under each other.
    lock_path = Path(os.environ.get("XDG_RUNTIME_DIR", f"/run/user/{os.getuid()}")) / "inir-backend-benchmark.lock"
    benchmark_lock = os.open(lock_path, os.O_CREAT | os.O_RDWR | os.O_NOFOLLOW, 0o600)
    try:
        fcntl.flock(benchmark_lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
    except BlockingIOError:
        raise RuntimeError("Another exclusive backend benchmark is running")
    if (args.output / "results.json").exists():
        raise RuntimeError("Output already contains results; choose a new directory")
    fixture_binary = ROOT / "rust/build/inir_title_churn"
    if (args.title_churn or args.fixture_only) and not fixture_binary.is_file():
        raise RuntimeError("Build rust/build/inir_title_churn first")
    if args.qml_profile and not Path("/usr/lib/qt6/bin/qmlprofiler").is_file():
        raise RuntimeError("Qt's qmlprofiler executable is required")
    require_unlocked("before start")
    original_pid = int(command("systemctl", "--user", "show", "inir.service", "-p", "MainPID", "--value"))
    if original_pid <= 0 or shell_pids() != [original_pid]:
        raise RuntimeError("Requires exactly one running shell managed by inir.service")
    if orphan_swayidle():
        raise RuntimeError(f"Unowned orphan helpers already exist: {orphan_swayidle()}; refusing to kill or benchmark around them")
    remember_shell_helpers(original_pid)
    environment = dict(os.fsdecode(item).split("=", 1) for item in Path(f"/proc/{original_pid}/environ").read_bytes().split(b"\0") if b"=" in item)
    original_cmd = [os.fsdecode(arg) for arg in Path(f"/proc/{original_pid}/cmdline").read_bytes().split(b"\0") if arg]
    workspaces = json.loads(command("niri", "msg", "-j", "workspaces"))
    focused = next(ws for ws in workspaces if ws["is_focused"])
    original_windows = json.loads(command("niri", "msg", "-j", "windows"))
    original_focus = next((w["id"] for w in original_windows if w["is_focused"]), None)
    alternate = next(ws for ws in workspaces if ws["output"] == focused["output"] and ws["id"] != focused["id"])
    stream = socket.socket(socket.AF_UNIX)
    stream.settimeout(5)
    stream.connect(environment["NIRI_SOCKET"])
    reader = stream.makefile("rb")
    def focus(workspace_id):
        niri_request(stream, reader, {"Action": {"FocusWorkspace": {"reference": {"Id": workspace_id}}}})
    def observe_workspace(expected=None):
        rows = niri_request(stream, reader, "Workspaces")["Workspaces"]
        row = next(ws for ws in rows if ws["is_focused"])
        if expected is not None and row["id"] != expected:
            raise RuntimeError(f"Workspace changed outside the injected workload: expected {expected}, got {row['id']}")
        return {"workspaceId": row["id"], "activeWindowId": row.get("active_window_id")}
    def observe_idle():
        return observe_workspace(focused["id"])
    rescan_codes = []
    def rescan(_index):
        # Same NetworkManager rescan for both modes; each shell reacts through its own path.
        # Exit codes are kept: NetworkManager may reject a scan that follows another too closely.
        result = subprocess.run(["nmcli", "device", "wifi", "rescan"], timeout=15, capture_output=True, check=False)
        rescan_codes.append(result.returncode)
    args.output.mkdir(parents=True, exist_ok=True)
    report = {"capturedAt": datetime.now(timezone.utc).isoformat(), "layout": "ii", "runs": [],
              "originalWorkspaceId": focused["id"], "alternateWorkspaceId": alternate["id"],
              "output": focused["output"], "logicalCpus": os.cpu_count(),
              "rounds": args.rounds, "durationSeconds": args.duration, "warmupSeconds": args.warmup,
              "scenarios": ["idle", "workspaceSwitching", "wifiRescan"],
              "nativeDomains": [name.strip() for name in args.native_domains.split(",") if name.strip()],
              "commonServices": "Quickshell audio, battery, Bluetooth, media and power profiles remain identical in both modes.",
              "workspaceVariants": variants,
              "qmlProfilerEnabled": args.qml_profile,
              "notes": ["CPU 100% means one fully occupied core; main-thread CPU is counted separately.",
                        "GPU usage is per-shell DRM client engine time, not whole-device utilization.",
                        "Driver-reported resident VRAM/GTT can include shared buffers; do not add them to RSS.",
                        "Process-tree CPU includes reaped helper ticks; sampled process starts can miss short-lived helpers.",
                        "spawnsByName counts child processes seen at 0.5 s sampling; very short helpers can be missed.",
                        "nativeApplyCalls (Rust) counts coalesced publications; fallbackEventsRead (QML) counts raw Niri events. The two are not the same unit.",
                        "Context switches are not scheduler wakeups; no frame-time or latency claims."]}
    if args.workspace_only:
        report["scenarios"] = ["workspaceSwitching"]
    report["windowCount"] = len(original_windows)
    report["outputs"] = json.loads(command("niri", "msg", "-j", "outputs"))
    child, fixture, qml_profiler, media_fixture = None, None, None, None
    try:
        command("systemctl", "--user", "stop", "inir.service")
        if shell_pids():
            raise RuntimeError(f"A shell survived service stop: {shell_pids()}")
        if not remove_orphan_swayidle():
            raise RuntimeError("Unowned helpers appeared after stopping the original shell")
        if args.media_fixture:
            media_log = (args.output / "media-fixture.log").open("w")
            media_fixture = subprocess.Popen([sys.executable, str(ROOT / "scripts/fixtures/media-player.py")],
                env=environment, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=media_log, text=True)
            ready, _, _ = select.select([media_fixture.stdout], [], [], 5)
            if not ready or media_fixture.stdout.readline().strip() != "ready":
                raise RuntimeError("MPRIS fixture failed to start; see media-fixture.log")
            report["mediaFixture"] = {"busName": "org.mpris.MediaPlayer2.inir_perf_fixture",
                                      "synthetic": True, "producesAudio": False}
        if args.title_churn or args.fixture_only:
            fixture_env = environment.copy()
            fixture_env["QT_QPA_PLATFORM"] = "wayland"
            fixture = subprocess.Popen([str(fixture_binary)], env=fixture_env, stdin=subprocess.PIPE,
                                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, text=True)
            for _ in range(30):
                fixture_windows = json.loads(command("niri", "msg", "-j", "windows"))
                fixture_window = next((w for w in fixture_windows if w["pid"] == fixture.pid), None)
                if fixture_window:
                    break
                if fixture.poll() is not None:
                    raise RuntimeError("Title fixture failed to map a real Wayland window")
                time.sleep(0.2)
            else:
                raise RuntimeError("Title fixture did not appear in Niri")
            if args.title_churn:
                report["scenarios"].append("titleChurn")
                report["titleChangesPerSecond"] = 20
            report["titleFixtureWindowId"] = fixture_window["id"]
            report["windowCount"] = len(fixture_windows)
            if args.fixture_workspace == "alternate":
                niri_request(stream, reader, {"Action": {"MoveWindowToWorkspace": {
                    "window_id": fixture_window["id"], "reference": {"Id": alternate["id"]}, "focus": False}}})
            report["fixtureWorkspace"] = args.fixture_workspace
        for round_index in range(args.rounds):
            modes = ("rust", "qml") if round_index % 2 == 0 else ("qml", "rust")
            for mode in modes:
                require_unlocked(f"before {mode} round {round_index + 1}")
                focus(focused["id"])
                env = environment.copy()
                env.update(INIR_RUST_BACKEND="1" if mode == "rust" else "0",
                           INIR_NATIVE_BRIDGE="1" if mode == "rust" else "0",
                           INIR_NATIVE_DOMAINS=args.native_domains, INIR_DISABLE_HOT_RELOAD="1",
                           INIR_BENCHMARK="1",
                           QML_IMPORT_PATH=str(ROOT / "rust/build/qml"), INIR_LOG="error")
                name = f"{round_index + 1}-{mode}"
                log = (args.output / f"{name}.log").open("w")
                shell_cmd = list(original_cmd)
                if args.qml_profile:
                    shell_cmd.extend(["--debug", "3768", "--waitfordebug"])
                child = subprocess.Popen(shell_cmd, env=env, stdout=log, stderr=subprocess.STDOUT)
                print(f"Starting {name}, PID {child.pid}; warming up {args.warmup}s", flush=True)
                if args.qml_profile:
                    profiler_log = (args.output / f"{name}-profiler.log").open("w")
                    qml_profiler = subprocess.Popen([
                        "/usr/lib/qt6/bin/qmlprofiler", "--attach", "127.0.0.1", "--port", "3768",
                        "--interactive", "--verbose", "--record", "on", "--include",
                        "javascript,binding,handlingsignal,animations,scenegraph,painting,inputevents,creating,pixmapcache",
                    ], stdin=subprocess.PIPE, stdout=profiler_log, stderr=subprocess.STDOUT, text=True)
                    time.sleep(2)
                    if qml_profiler.poll() is not None:
                        raise RuntimeError("QML profiler failed to attach; see profiler log")
                for attempt in range(15):
                    try:
                        ipc(child.pid, "backend", "prepareBenchmark")
                        break
                    except subprocess.SubprocessError:
                        time.sleep(0.5)
                else:
                    raise RuntimeError("Benchmark IPC did not become available")
                niri_request(stream, reader, {"Action": {"PowerOnMonitors": {}}})
                time.sleep(args.warmup)
                if child.poll() is not None:
                    raise RuntimeError(f"{name} exited: see {log.name}")
                # Startup services can change focus. Settle on the same output
                # and workspace after warm-up so idle presentation is comparable.
                focus(focused["id"])
                time.sleep(2)
                # Retry readiness briefly, but reject a silent fallback or duplicates.
                last_error = None
                for attempt in range(15):
                    # Re-issue the focus each attempt: a shell that missed the first
                    # WorkspaceActivated event would otherwise stay stale until restart.
                    focus(focused["id"])
                    time.sleep(1)
                    try:
                        before = ownership(child.pid, mode)
                        assert before["niri"]["focusedWorkspaceId"] == str(focused["id"]), before
                        break
                    except (AssertionError, subprocess.SubprocessError, ValueError) as error:
                        last_error = error
                else:
                    raise RuntimeError(f"Ownership failed for {name}: {last_error}")
                print(f"Verified {mode} ownership", flush=True)
                if media_fixture:
                    assert before["ui"]["mediaPlaying"] and media_fixture.poll() is None, before
                if args.qml_profile:
                    finish_qml_trace(qml_profiler, args.output / f"{name}-startup.qtd")
                ipc(child.pid, "dev", "close")
                time.sleep(2)
                if not args.workspace_only:
                    print(f"Capturing idle for {name}", flush=True)
                    if qml_profiler:
                        begin_qml_trace(qml_profiler)
                idle = measure(child.pid, args.duration, observer=observe_idle) if not args.workspace_only else None
                if qml_profiler and idle:
                    idle["qmlTrace"] = finish_qml_trace(qml_profiler, args.output / f"{name}-idle.qtd")
                require_unlocked(f"during {name} idle")
                print(f"Capturing workspace activity for {name}", flush=True)
                workspace_variants = {}
                # Counterbalance ablation order too; baseline is captured first
                # in half the launches and last in the other half.
                run_variants = variants if len(report["runs"]) % 2 == 0 else list(reversed(variants))
                for variant in run_variants:
                    presentation = json.loads(ipc(child.pid, "backend", "benchmarkPresentation", variant))
                    focus(focused["id"])
                    time.sleep(3)
                    active_before = ownership(child.pid, mode)
                    print(f"Workspace variant {variant} for {name}", flush=True)
                    if qml_profiler:
                        begin_qml_trace(qml_profiler)
                    capture = measure(child.pid, args.duration,
                                      lambda index: focus(alternate["id"] if index % 2 == 0 else focused["id"]),
                                      observer=observe_workspace)
                    if qml_profiler:
                        capture["qmlTrace"] = finish_qml_trace(qml_profiler, args.output / f"{name}-{variant}.qtd")
                    capture["presentation"] = presentation
                    capture["ownershipBefore"] = active_before
                    capture["ownershipAfter"] = ownership(child.pid, mode)
                    if media_fixture:
                        assert capture["ownershipBefore"]["ui"]["mediaPlaying"] and capture["ownershipAfter"]["ui"]["mediaPlaying"], capture
                    workspace_variants[variant] = capture
                windows = workspace_variants.get("baseline", next(iter(workspace_variants.values())))
                ipc(child.pid, "backend", "benchmarkPresentation", "baseline")
                focus(focused["id"])
                time.sleep(0.25)
                after = ownership(child.pid, mode)
                key = lambda state: state["niri"]["nativeApplyCalls"] if mode == "rust" else state["niri"]["fallback"]["eventsRead"]
                if key(after) <= key(active_before):
                    raise RuntimeError("Workspace workload did not reach the measured Niri owner")
                require_unlocked(f"during {name} workspace activity")
                if not args.workspace_only:
                    print(f"Capturing Wi-Fi rescans for {name}", flush=True)
                rescan_codes.clear()
                wifi = measure(child.pid, args.duration, rescan, interval=5, observer=observe_idle) if not args.workspace_only else None
                if wifi:
                    wifi["rescanExitCodes"] = list(rescan_codes)
                after_wifi = ownership(child.pid, mode)
                run = {"name": name, "mode": mode, "pid": child.pid,
                       "ownershipBefore": before, "ownershipAfter": after_wifi,
                       "niriEventsDuringActivity": key(after) - key(active_before),
                       "idle": idle, "workspaceSwitches": windows, "wifiRescans": wifi,
                       "workspaceVariants": workspace_variants}
                if fixture and args.title_churn:
                    focus(focused["id"])
                    time.sleep(2)
                    title_before = ipc(child.pid, "backend", "windowTitle", str(fixture_window["id"]))
                    title_ownership_before = ownership(child.pid, mode)
                    if qml_profiler:
                        begin_qml_trace(qml_profiler)
                    fixture.stdin.write("start\n")
                    fixture.stdin.flush()
                    print(f"Capturing title churn for {name} (20 changes/s)", flush=True)
                    run["titleChurn"] = measure(child.pid, args.duration, observer=observe_idle)
                    if qml_profiler:
                        run["titleChurn"]["qmlTrace"] = finish_qml_trace(qml_profiler, args.output / f"{name}-title.qtd")
                    fixture.stdin.write("stop\n")
                    fixture.stdin.flush()
                    time.sleep(0.2)
                    title_after = ipc(child.pid, "backend", "windowTitle", str(fixture_window["id"]))
                    assert title_after.startswith("INIR title fixture: ") and title_after != title_before, (title_before, title_after)
                    run["titleBefore"], run["titleAfter"] = title_before, title_after
                    run["ownershipAfter"] = ownership(child.pid, mode)
                    run["titleChurn"]["ownershipBefore"] = title_ownership_before
                    run["titleChurn"]["ownershipAfter"] = run["ownershipAfter"]
                    require_unlocked(f"during {name} title churn")
                report["runs"].append(run)
                (args.output / "results.json").write_text(json.dumps(report, indent=2) + "\n")
                print(f"Saved {name}: activity CPU {windows['summary']['cpuPercent']['mean']:.2f}% "
                      f"(main {windows['summary']['mainThreadCpuPercent']['mean']:.2f}%)", flush=True)
                remember_shell_helpers(child.pid)
                if qml_profiler:
                    qml_profiler.stdin.write("q\n")
                    qml_profiler.stdin.flush()
                    qml_profiler.wait(timeout=15)
                    qml_profiler = None
                    profiler_log.close()
                child.terminate()
                child.wait(timeout=15)
                child = None
                log.close()
                time.sleep(1)
                if shell_pids():
                    raise RuntimeError(f"Shell remained between runs: {shell_pids()}")
                if not remove_orphan_swayidle():
                    raise RuntimeError("A helper survived cleanup; exclusive measurement is no longer guaranteed")
    finally:
        try:
            if media_fixture is not None:
                media_fixture.stdin.close()
                try:
                    media_fixture.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    media_fixture.terminate()
                    media_fixture.wait(timeout=5)
            if qml_profiler is not None and qml_profiler.poll() is None:
                qml_profiler.terminate()
                qml_profiler.wait(timeout=15)
            if child is not None and child.poll() is None:
                remember_shell_helpers(child.pid)
                child.terminate()
                try:
                    child.wait(timeout=15)
                except subprocess.TimeoutExpired:
                    child.kill()
                    child.wait()
            if fixture:
                fixture.stdin.close()
                try:
                    fixture.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    fixture.terminate()
                    try:
                        fixture.wait(timeout=5)
                    except subprocess.TimeoutExpired:
                        fixture.kill()
                        fixture.wait()
            focus(focused["id"])
            if original_focus is not None:
                niri_request(stream, reader, {"Action": {"FocusWindow": {"id": original_focus}}})
        finally:
            reader.close()
            stream.close()
            remove_orphan_swayidle()
            command("systemctl", "--user", "start", "inir.service")
            print("Restored inir.service and original workspace", flush=True)


if __name__ == "__main__":
    main()
