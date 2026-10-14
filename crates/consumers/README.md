# Frostline consumers

The consumers crate runs one reader per team. Each reader asks the server for its slice of the `changes` topic, handles every record it receives, and publishes a receipt to `reports` at every checkpoint. The same loop also runs as a full-feed baseline that downloads every record and filters in the application, so a comparison run can prove both paths select the same records.

## Start here

- `run` (`src/lib.rs`): builds the reader for one group with its configured policy, or the full-feed baseline.
- `Progress` (`src/progress.rs`): what a reader does with each record. A checkpoint publishes its receipt before the reader acknowledges anything.
- `drive` (`src/reader.rs`): the filtered page loop. A persisted window receipt releases the processed prefix through its checkpoint. Later records in the page remain pending.
- `drive_baseline` (`src/baseline.rs`): the ordinary consumer loop that reads everything.
- `SafetyHandler`, `MaintenanceHandler`, `RegionalHandler`: the team logic. Handlers are synchronous and never block on the network.

## Groups

| group | team | filter |
| --- | --- | --- |
| `food-safety` | Food safety | trucks entering the unsafe band, and refrigerated trucks leaving |
| `food-safety-current` | Food safety A/B | every update of a truck whose current band is unsafe |
| `maintenance` | Maintenance | reefer faults at error severity or above, headers only |
| `north-pharma` | Regional operations | telemetry from north pharma and frozen trucks, and their changes at 10 tonnes or more |

## Run it

```text
frostline-consumers --manifest runs/<run-id>/run.json --group food-safety [--baseline] [--worker 2] [--attempt 2] [--live] [--hold] [--replay]
```

`--worker n` with `FROSTLINE_WORKERS_PER_ROLE=N` reads the partitions `p` where `p % N == n - 1`. Workers split the partitions between themselves instead of joining one consumer group, so no rebalance can hand a partition over inside a window. Each worker owns a separate group policy and reads only its assigned partition subset. `--attempt` marks a restart after a crash, so its receipts are told apart from the first try. `--hold` keeps the connections open after the last checkpoint until stdin closes. The bench uses it to read the socket counters first.

## Tests

Unit tests cover the handlers, the bounded tables, the narration sampler, and the worker split. `tests/integration.rs` runs the producer and all three teams against a fresh server, then checks every receipt against the manifest and the baseline digest against the filtered one. Two more cases prove the failure paths: a payload that doesn't decode stops the reader before any acknowledgment, and a reader that can't publish its receipt never acknowledges the checkpoint. Run `cargo test -p frostline-consumers`, then `cargo test -p frostline-consumers --features integration`.

## Latency measurement

Filtered readers time one bounded read round. Ordinary readers time explicit native batch polls, without guessing from buffered record duration. Summaries include p50, p99, p99.9, call count, and examined records per call. Infinite runs switch from exact samples to a fixed histogram after 65,536 calls. The summary reports the histogram error bound.

Normal readers resume stored progress. The benchmark explicitly uses `--replay` for both timed and traced passes. Repeated checkpoints publish identical receipts, and duplicate business records do not reach handlers twice.

The reader resumes stored final checkpoints, validates its worker index against the run file and uses the public bounded-round API. Its outstanding-page bound follows the receipt window.

Use `--version` to print the binary name and release version without starting the application.
