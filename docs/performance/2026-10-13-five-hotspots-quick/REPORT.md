# Rejected title-workload attempt

This attempt is not an accepted A/B result. The native shell passed ownership
checks, including a ready native keyboard worker and no Python keyboard probe or
monitor. The run then rejected the title workload because the compositor's
focused workspace was 2 when the workload expected 1. The reason for that focus
change was not established. The managed QML shell and original workspace were
restored by the harness.

The raw Rust shell log is retained in `1-rust.log`. There is no completed pair
and no resource-performance conclusion from this attempt. The subsequent
[short pair](../2026-10-13-five-hotspots-quick-pair/results.json) (raw data; see [the 2026-10-14 verification](../2026-10-14-chatgpt-verify/REPORT.md)) omits title churn;
title-only workspace no-op behavior remains covered by the compiled model test.
