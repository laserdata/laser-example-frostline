# Frostline demo

The demo is the composition root. It checks the server, creates a run, starts one reader per group, publishes the fleet, collects receipts, and prints what each team received against the full feed. `just demo` runs until Ctrl+C. `just demo-once` runs the finite story. `just compare` adds a full-feed reader per team and proves both read paths select the same records.

## Start here

- `scenario::finite` (`src/scenario/finite.rs`): the finite story from setup to cleanup in one function.
- `Session::start` (`src/scenario/mod.rs`): doctor, provisioning, the reporter, and the reader tasks.
- `provision` (`src/provision.rs`): the run stream, writer schemas, sample tests, saved filters, and bound groups.
- `Reporter` (`src/reporter.rs`): follows `reports` and keeps the aggregate for the board.
- `walkthrough` (`src/walkthrough.rs`): partition previews, then a replay group policy paused and resumed.
- `mixed` (`src/mixed.rs`): one log with events in three codecs, and four filters that show what gets skipped, selected, or handed over unevaluated.

## Commands

| command | does |
| --- | --- |
| `live` | runs the fleet until Ctrl+C with the live board (default) |
| `finite` | one finite story, report, cleanup |
| `compare` | the finite story plus a full-feed reader per team |
| `codecs` | the finite story once per codec, with equal matches required |
| `doctor` | checks filters, the catalog, and codecs |
| `setup` | creates a run for standalone processes |
| `report --manifest <run.json>` | renders a saved run |
| `cleanup --manifest <run.json>` | deletes only that run's resources |

Exit code 2 means a usage or capability problem to fix before a run. Exit code 1 means a run failed or its measurement is incomplete.

## Tests

Unit tests cover command parsing and the capability checks. `tests/e2e.rs` runs seven stories against a fresh server: managed, compare, inline, the same settings twice with identical totals, more windows than the reporter holds, three workers per group, and a team that matches nothing. Run `cargo test -p frostline-demo`, then `cargo test -p frostline-demo --features e2e`.

The JSON and Markdown reports include every reader's p50, p99, p99.9, call count, and examined records per call. Filtered rounds and ordinary polls have separate labels. Payload savings exclude checkpoints, reports, framing, and TLS. Use replay benchmarks for separate TCP socket-byte measurements.

Use `--version` to print the binary name and release version without starting the application.
