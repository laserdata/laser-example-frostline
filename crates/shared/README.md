# Frostline shared

The shared crate is the vocabulary every other crate speaks. It holds the domain, the event on the wire, the three team policies, the codecs, the measurement types, settings, connection setup, output, lifecycle, topology, and the run file. No reader or producer logic lives here.

## Start here

- `FleetEvent` (`src/event.rs`): the one record type on the `changes` topic. A change carries before and after state and the changed fields. Telemetry carries a reading. A checkpoint closes a window on one partition.
- `RolePolicy` (`src/policy/mod.rs`): each team's filter next to the hand-written predicate that checks it. Every filter starts with the checkpoint branch, which the server decides from a header before it decodes anything.
- `Codec` (`src/codec/mod.rs`): encode, decode, typed headers, and filter construction for JSON, CBOR, Avro, and Protobuf.
- `Aggregator` and `Totals` (`src/measure`): join window manifests and receipts, and report savings only for windows every reader completed.
- `Settings` (`src/config/mod.rs`): every `FROSTLINE_*` variable, parsed once and validated.

## Surfaces

| module | owns |
| --- | --- |
| `domain` | truck ids, temperatures, battery, regions, cargo, bands, and the truck state diff |
| `event` | `FleetEvent`, its bodies, validation, and the header diagnosis |
| `policy` | food safety, maintenance, regional, and the food safety A/B policy |
| `codec` | four payload formats, typed headers, writer schema registration |
| `measure` | window manifests, receipts, rolling digests, the aggregator, and reduction math |
| `config`, `knobs` | settings and their environment variables |
| `connect` | `LaserFactory`, one connection story for Laser Stack and LaserData Cloud |
| `topology` | the run stream, its two topics, retention, and the source identity |
| `runfile` | `run.json` and the local producer lock |
| `output`, `telemetry`, `lifecycle` | narration, tracing, and graceful shutdown |
| `testkit` | the integration test setup, behind the `testkit` feature |

## Tests

Unit tests sit next to each module and need no server. The policy tests compare every filter with its hand-written predicate on scenario events and on 500 generated events. `tests/integration.rs` runs against the test stack. Run `cargo test -p frostline-shared`, then `cargo test -p frostline-shared --features integration`.

Measurement requires contiguous complete windows. Late reports for the completed prefix do not recreate pending windows. Receipts must agree with the manifest on selected counts, bytes, and digests. A checkpoint for another open window fails without clearing its counters.

The run file records each group-owned filter binding. Multiple workers use separate groups and persist their partition ownership.
