#!/usr/bin/env python3
"""Summarize exclusive workspace ablations without treating samples as repeats."""
import argparse
from collections import defaultdict
import json
from pathlib import Path
import statistics


def weighted_metric(capture, metric):
    rows = [row for row in capture["samples"] if row.get(metric) is not None]
    return (sum(row[metric] * row["intervalSeconds"] for row in rows)
            / sum(row["intervalSeconds"] for row in rows)) if rows else None


def capture_metrics(run, capture):
    metrics = {key: weighted_metric(capture, key) for key in
               ("cpuPercent", "mainThreadCpuPercent", "gpuBusiestEnginePercent", "pssBytes")}
    groups = defaultdict(float)
    for thread in capture.get("threadCpu", []):
        name = thread["name"]
        if thread["tid"] == run["pid"]:
            group = "main"
        elif name == "inir-core":
            group = "rustExecutor"
        elif name.startswith("tokio-"):
            group = "rustBlocking"
        elif name == "QSGRenderThread":
            group = "qtRender"
        elif name.startswith("qs:"):
            group = "mesaWorkers"
        else:
            group = "other"
        groups[group] += thread["cpuPercent"]
    metrics.update({"threadCpu." + key: groups[key] for key in
                    ("main", "rustExecutor", "rustBlocking", "qtRender", "mesaWorkers", "other")})
    metrics["threadCpu.accounted"] = sum(groups.values())
    before, after = capture.get("ownershipBefore", {}), capture.get("ownershipAfter", {})
    if before.get("models") and after.get("models"):
        total_us = 0
        for domain in ("niri", "workspaces"):
            start, end = before["models"][domain], after["models"][domain]
            calls = end["applyCalls"] - start["applyCalls"]
            delta = end["applyTotalUs"] - start["applyTotalUs"]
            metrics[domain + ".applyCalls"] = calls
            metrics[domain + ".applyMeanUs"] = delta / calls if calls else None
            total_us += delta
        metrics["nativeApplyWallPercent"] = total_us / 1e6 / capture["durationSeconds"] * 100
    return metrics


def summary(rows):
    return {key: {"mean": statistics.fmean(values), "min": min(values), "max": max(values),
                  "runs": len(values)}
            for key in sorted({key for row in rows for key in row})
            if (values := [row[key] for row in rows if row.get(key) is not None])}


def analyze(report):
    captures = defaultdict(lambda: defaultdict(list))
    per_run = {}
    scenarios = defaultdict(lambda: defaultdict(list))
    for run in report["runs"]:
        per_run[run["name"]] = {}
        for variant, capture in run["workspaceVariants"].items():
            row = capture_metrics(run, capture)
            captures[variant][run["mode"]].append(row)
            per_run[run["name"]][variant] = row
        for scenario in ("idle", "wifiRescans", "titleChurn"):
            if run.get(scenario):
                scenarios[scenario][run["mode"]].append(capture_metrics(run, run[scenario]))
    variants = {}
    for variant, modes in captures.items():
        owners = {mode: summary(rows) for mode, rows in modes.items()}
        paired = []
        for round_index in range(1, report["rounds"] + 1):
            rust = per_run.get(f"{round_index}-rust", {}).get(variant)
            qml = per_run.get(f"{round_index}-qml", {}).get(variant)
            if rust and qml:
                paired.append({"round": round_index,
                               "cpuDifferencePoints": rust["cpuPercent"] - qml["cpuPercent"],
                               "mainDifferencePoints": rust["mainThreadCpuPercent"] - qml["mainThreadCpuPercent"],
                               "gpuDifferencePoints": rust["gpuBusiestEnginePercent"] - qml["gpuBusiestEnginePercent"]})
        variants[variant] = {"owners": owners, "pairedDifferences": paired}
    ablations = {}
    for mode in ("qml", "rust"):
        effects = defaultdict(list)
        for run, rows in per_run.items():
            if not run.endswith("-" + mode) or "baseline" not in rows:
                continue
            for variant, row in rows.items():
                if variant != "baseline":
                    effects[variant].append({
                        "cpuReductionPoints": rows["baseline"]["cpuPercent"] - row["cpuPercent"],
                        "cpuReductionPercent": (1 - row["cpuPercent"] / rows["baseline"]["cpuPercent"]) * 100,
                        "gpuReductionPoints": rows["baseline"]["gpuBusiestEnginePercent"] - row["gpuBusiestEnginePercent"],
                    })
        ablations[mode] = {variant: summary(rows) for variant, rows in effects.items()}
    return {"source": "results.json", "completedRuns": len(report["runs"]),
            "notes": ["Each fresh shell launch is a repeat; one-second samples are not independent repeats.",
                      "CPU percentages use one logical core as 100%.",
                      "Native apply wall time includes synchronous QML signal/binding work and is not isolated Rust CPU.",
                      "Thread totals can omit short-lived/new threads; inspect accounted versus process CPU.",
                      "At 100 Hz and 20 seconds, one CPU tick is 0.05 percentage points; zero is below sampling resolution.",
                      "Three pairs do not establish a statistically robust population difference."],
            "variants": variants, "ablations": ablations, "perRun": per_run,
            "otherScenarios": {scenario: {mode: summary(rows) for mode, rows in modes.items()}
                               for scenario, modes in scenarios.items()}}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("results", type=Path)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    report = json.loads(args.results.read_text())
    result = analyze(report)
    if args.output:
        args.output.write_text(json.dumps(result, indent=2) + "\n")
    print(f"Completed launches: {result['completedRuns']}; windows: {report['windowCount']}")
    print("Variant / owner                 CPU    main     GPU    render   Mesa    Rust")
    for variant, data in result["variants"].items():
        for mode in ("qml", "rust"):
            metrics = data["owners"].get(mode)
            if not metrics:
                continue
            values = [metrics[key]["mean"] for key in
                      ("cpuPercent", "mainThreadCpuPercent", "gpuBusiestEnginePercent",
                       "threadCpu.qtRender", "threadCpu.mesaWorkers", "threadCpu.rustExecutor")]
            print(f"{variant + ' / ' + mode:<30}" + " ".join(f"{value:7.2f}" for value in values))
    print("Paired baseline CPU differences (Rust minus QML, percentage points):")
    for row in result["variants"].get("baseline", {}).get("pairedDifferences", []):
        print(f"  round {row['round']}: {row['cpuDifferencePoints']:+.2f}")
    for scenario, owners in result["otherScenarios"].items():
        for mode, metrics in owners.items():
            print(f"{scenario} / {mode}: CPU {metrics['cpuPercent']['mean']:.2f}%, "
                  f"main {metrics['mainThreadCpuPercent']['mean']:.2f}%")


if __name__ == "__main__":
    main()
