# Local performance investigations

The dated reports and JSON summaries record the conditions, measurements and
limitations of each investigation. Historical results are not a universal
performance or temperature guarantee; several native rollout gates remain open.

Raw Qt profiler captures (`*.qtd` / `*.qtd.gz`) are machine-local artifacts and
are ignored by Git. Existing captures are retained locally, not deleted. Reports
may name those captures as provenance; reproducing their analysis requires a
new capture or the original local files. The profiling and summary scripts live
under `scripts/`.

`compare-backends.py` is an explicitly disruptive live benchmark: it temporarily
replaces the running shell and restores it afterward. Do not run it during file
transfers or pairing. Its ownership checks distinguish the benchmarked native
domains from the Bluetooth controller, which is common to both backend modes.
`test-backend-comparison.py` exercises those guards without restarting services.
