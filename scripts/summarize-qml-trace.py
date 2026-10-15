#!/usr/bin/env python3
"""Extract event counts and inclusive wall times from Qt's XML QML trace format."""
import argparse
from collections import defaultdict
import gzip
import json
from pathlib import Path
import xml.etree.ElementTree as ET


def summarize(path):
    opener = gzip.open if path.suffix == ".gz" else open
    with opener(path, "rb") as source:
        root = ET.parse(source).getroot()
    events = {event.attrib["index"]: {child.tag: child.text or "" for child in event}
              for event in root.findall("./eventData/event")}
    by_event = defaultdict(lambda: {"count": 0, "inclusiveNs": 0, "maxNs": 0})
    by_type = defaultdict(lambda: {"count": 0, "inclusiveNs": 0})
    for span in root.findall("./profilerDataModel/range"):
        event_index = span.attrib["eventIndex"]
        duration = max(0, int(span.attrib.get("duration", "0")))
        event = events[event_index]
        row = by_event[event_index]
        row["count"] += 1
        row["inclusiveNs"] += duration
        row["maxNs"] = max(row["maxNs"], duration)
        kind = event.get("type", "unknown")
        by_type[kind]["count"] += 1
        by_type[kind]["inclusiveNs"] += duration
    rows = [{"index": index, **events[index], **counts,
             "inclusiveMs": counts["inclusiveNs"] / 1e6,
             "maxMs": counts["maxNs"] / 1e6} for index, counts in by_event.items()]
    return {"file": path.name, "trace": root.attrib, "events": len(events),
            "notes": ["Trace durations are instrumented wall time, not CPU samples.",
                      "Inclusive event times can contain nested events; do not add them as independent CPU time."],
            "byType": dict(by_type),
            "eventsByDuration": sorted(rows, key=lambda row: row["inclusiveNs"], reverse=True)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("traces", type=Path, nargs="+")
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    result = [summarize(path) for path in args.traces]
    if args.output:
        args.output.write_text(json.dumps(result, indent=2) + "\n")
    for trace in result:
        print(trace["file"], trace["trace"])
        print("Types:", {key: row["count"] for key, row in trace["byType"].items()})
        for row in trace["eventsByDuration"][:15]:
            print(f"{row['inclusiveMs']:9.2f} ms  {row['count']:6}  {row.get('type', ''):15} "
                  f"{row.get('filename', row.get('displayname', ''))}:{row.get('line', '')} "
                  f"{row.get('details', '')[:90]}")


if __name__ == "__main__":
    main()
