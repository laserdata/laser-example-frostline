# Frostline producer

The producer simulates the carrier and publishes every fact once to the run's `changes` topic. Trucks move, drain batteries, load and deliver, and sometimes lose refrigeration. Each window of records ends with one checkpoint per partition, and the producer then publishes the window manifest to `reports`.

## Start here

- `run` (`src/lib.rs`): the whole loop. It paces records, closes windows, and drains on shutdown.
- `Fleet` (`src/fleet.rs`): the seeded simulation. The same seed and settings give the same events.
- `Incidents` (`src/incident.rs`): a failure starts, keeps a truck unsafe for several updates, then recovers.
- `WindowTracker` (`src/windows.rs`): counts confirmed records and what each group's predicate selects.
- `PartitionPublisher` (`src/publish.rs`): one ordered task per partition with bounded batches and a bounded queue.

## Ordering and truth

A truck always lands on the same partition through `partition_for`, an FNV-1a hash of its id, so its changes stay in order. Records count toward a manifest only after the server confirmed their batch. At a window boundary the producer confirms the records, then the checkpoints, and only then publishes the manifest.

## Run it

```text
frostline-producer --manifest runs/<run-id>/run.json
```

`frostline-demo setup` writes the run file. The demo usually runs the producer in process.

## Tests

Unit tests cover determinism, event validity, the change mix, retirement, incident timing, partition spread, pacing, and window counting. `tests/integration.rs` publishes a finite run against the test stack and checks the manifests against what an ordinary reader reads back. Run `cargo test -p frostline-producer`, then `cargo test -p frostline-producer --features integration`.

Pacing keeps cumulative deadlines with a configurable catch-up bound. A producer cannot restart into a populated run. A finite duration stop always publishes a final checkpoint, including an empty closing window.

Use `--version` to print the binary name and release version without starting the application.
