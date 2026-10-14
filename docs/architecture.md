# Architecture

## Boundaries

Frostline is one producer, one reader per team, and one composition root. Every process connects through the same `LaserFactory`. They never call each other. A record on a topic is the only interface.

Each run owns one stream, `frostline-<run-id>`, with two topics. `changes` holds the fleet's records. `reports` holds window manifests from the producer and receipts from the readers. The reports topic exists only to measure the example.

## The record

Every record on `changes` is a `FleetEvent`. It is a CDC change with before and after state and the list of changed fields, a telemetry reading, or a checkpoint that closes a window on one partition. The producer also stamps typed headers:

| header | type | meaning |
| --- | --- | --- |
| `frostline.frame` | uint8 | 0 business, 1 checkpoint |
| `frostline.unit` | string | `reefer`, `battery`, `door`, `engine` |
| `frostline.severity` | uint8 | 0 info, 1 warning, 2 error, 3 critical |
| `agdx.ct` | uint8 | content type |
| `agdx.sid` | uint32 | writer schema id, Avro and Protobuf only |

## Ordering

A truck lands on one partition through an FNV-1a hash of its id, so its changes stay in order. The producer keeps one publishing task per partition with a bounded queue. Nothing is dropped. A full queue slows the producer down.

## Windows and checkpoints

Every 2,000 records the producer closes a window. It waits until the server confirms the window's records, appends one checkpoint to each partition, waits for those too, and only then publishes the window manifest. Every team's filter accepts checkpoints through a header branch that the server decides before it decodes anything. That is how a reader that matches nothing still proves it reached the end of a window.

## Receipts and the report

At each checkpoint a reader publishes a receipt: what it received in that partition and window, and a digest of the records it selected. It acknowledges the checkpoint only after the receipt is out. The reporter joins manifests and receipts. A window counts only when every reader sent a receipt for every partition and each digest matches the manifest.

## Local and hosted

| concern | Laser Stack or the local runtime | LaserData Cloud |
| --- | --- | --- |
| consumer filters | served by the Iggy fork | served by the hosted Iggy |
| saved filters and group bindings | the local plane | the hosted plane |
| connection | `iggy:laser@127.0.0.1:8090` | deployment credentials, TLS attached by the SDK |

`just doctor` checks what the server offers before a run. A missing capability stops the run with a clear sentence. There is no fallback that downloads everything and filters in the application.
