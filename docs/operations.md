# Operations

## The runtime

Frostline needs an Iggy server that serves consumer filters and a plane that keeps saved filters and group bindings. There are two ways to get them.

**Laser Stack.** Clone [laser-stack](https://github.com/laserdata/laser-stack) beside Frostline first. `just up` runs the sibling `laser-stack` and waits until it is healthy. `just down` stops it and keeps its data. The release must carry consumer filters.

**Local runtime.** `just up-local` starts `iggy-server` and `plane` as native processes under `runs/stack/`. The server listens on 127.0.0.1:8090 with the user `iggy` and the password `laser`. The plane talks to it over `runs/stack/plane.sock`. `just down-local` stops both and keeps their data.

`scripts/resolve-runtime` picks each binary in this order:

1. `FROSTLINE_IGGY_SERVER` or `FROSTLINE_PLANE`, when set.
2. A local release build supplied in the development workspace.
3. A local debug build supplied in the development workspace.

`just test-it` and `just e2e` start their own native runtime. Supply compatible binaries with `FROSTLINE_IGGY_SERVER` and `FROSTLINE_PLANE`. A public reader can run the demo with Laser Stack, which uses the published `laserdatainc/iggy-server` and `laserdatainc/laser-plane` images. Locally built binaries must match the SDK revision this checkout uses.

## LaserData Cloud

Point the same binaries at a hosted deployment with its connection string:

```sh
LASER_CONNECTION_STRING=user:pwd@your-host.laserdata.cloud just doctor
```

The SDK attaches TLS. `just doctor` checks that the deployment serves consumer filters, the catalog, and the codec you picked before anything is created.

## Commands

| command | runs |
| --- | --- |
| `just doctor` | `frostline-demo doctor` |
| `just demo-once` | `frostline-demo finite` |
| `just compare` | `frostline-demo compare` |
| `just demo` | `frostline-demo live` |
| `just codecs` | `frostline-demo codecs` |
| `just inline` | `frostline-demo finite` with `FROSTLINE_CATALOG=inline` |
| `just report <run.json>` | `frostline-demo report --manifest <run.json>` |
| `just cleanup <run.json>` | `frostline-demo cleanup --manifest <run.json>` |
| `just scale <records>` | the finite story at full speed over `<records>` records, release build |
| `just soak <rate>` | the live story at `<rate>` records per second until Ctrl+C, release build |
| `just bench <profile>` | `frostline-bench bench <profile>` in a release build |
| `just selectivity` | the exact-selectivity trial, see [benchmarks](benchmarks.md) |
| `just up-pressure <limit>` | the local runtime inside a cgroup capped at `<limit>` of memory |
| `just profile-cpu <target>` | the same profile under `perf` |
| `just profile-memory <target>` | the same profile under `heaptrack` |

## Standalone processes

Each part also runs on its own against one run file. This is how you put the producer and each team on different machines:

```sh
cargo run -p frostline-demo -- setup
cargo run -p frostline-consumers -- --manifest runs/<run-id>/run.json --group maintenance
cargo run -p frostline-producer -- --manifest runs/<run-id>/run.json
cargo run -p frostline-demo -- report --manifest runs/<run-id>/run.json
cargo run -p frostline-demo -- cleanup --manifest runs/<run-id>/run.json
```

A reader takes `--baseline` to read the full feed, `--worker <n>` to select its configured partition share, `--replay` to replay from the start, and `--live` to keep reading after the last checkpoint. The group names are `food-safety`, `food-safety-current`, `maintenance`, and `north-pharma`.

Set the same `FROSTLINE_WORKERS_PER_ROLE` for setup and every worker process. The run file fixes the worker count. An out-of-range worker is refused. Completed finite readers recognize their stored final checkpoint after a restart.

Readers take the partitions, the codec, and the catalog from the run file. The producer refuses a run file whose values differ from its own settings, and it holds a lock on the run file so a second producer cannot publish into the same run. The claim stays until cleanup. A producer restart into a populated changes topic is refused. Start a new run instead of resetting counters.

## Settings

Every setting is a `FROSTLINE_*` environment variable read once at startup. There is no configuration file. A bad value stops the process with the variable name and the reason.

| variable | default | accepted |
| --- | --- | --- |
| `FROSTLINE_CATALOG` | `managed` | `managed` or `inline`, both require the plane |
| `FROSTLINE_CODEC` | `json` | `json`, `cbor`, `avro`, `protobuf` |
| `FROSTLINE_SEED` | `1312` | any unsigned number |
| `FROSTLINE_FLEET_SIZE` | `800` | 1 to 100000 |
| `FROSTLINE_PARTITIONS` | `4` | 1 to 64 |
| `FROSTLINE_RATE_PER_SECOND` | `1000` finite, `250` live | 1 to 1000000 |
| `FROSTLINE_RATE_CATCH_UP_RECORDS` | `64` | 1 through 1000, bounded catch-up after late timer wakes |
| `FROSTLINE_TOTAL_RECORDS` | `24000` | positive, finite modes only |
| `FROSTLINE_DURATION_SECONDS` | `0` | seconds, 0 means until Ctrl+C |
| `FROSTLINE_CHECKPOINT_RECORDS` | `2000` | records per window, positive |
| `FROSTLINE_CHANGE_PERCENT` | `80` | share of CDC changes, 0 to 100 |
| `FROSTLINE_INCIDENT_ONSET_PER_MILLE` | `3` | 0 to 1000 |
| `FROSTLINE_INCIDENT_UPDATES` | `5..30` | `min..max`, with 1 at most min at most max |
| `FROSTLINE_DIAGNOSTICS_SAMPLES` | `32` | 0 to 1024 |
| `FROSTLINE_BATCH_RECORDS` | `100` | 1 to 1000 |
| `FROSTLINE_BATCH_BYTES` | `262144` | 1024 to 8388608 |
| `FROSTLINE_BATCH_LINGER_MS` | `5` | 0 to 1000 |
| `FROSTLINE_PUBLISH_QUEUE_RECORDS` | `2000` | positive |
| `FROSTLINE_POLL_RECORDS` | `100` | 1 to 1000 |
| `FROSTLINE_REPLY_BYTES` | `1048576` | 1024 to 8388608 |
| `FROSTLINE_IDLE_INTERVAL_MS` | `100` | positive |
| `FROSTLINE_WORKERS_PER_ROLE` | `1` | 1 to 8 |
| `FROSTLINE_LOCAL_GUARD` | on finite, off live | `true`, `false`, `1`, `0`, `yes`, `no`, `on`, `off` |
| `FROSTLINE_MAX_PENDING_WINDOWS` | `120` | positive |
| `FROSTLINE_BOARD_INTERVAL_SECONDS` | `4` | positive |
| `FROSTLINE_SAMPLED_EVENTS_PER_ROLE_PER_SECOND` | `2` | 0 to 100 |
| `FROSTLINE_CHANGES_EXPIRY_SECONDS` | `0` finite, `3600` live | 0 means never |
| `FROSTLINE_REPORTS_EXPIRY_SECONDS` | `86400` | 0 means never |
| `FROSTLINE_DRAIN_TIMEOUT_SECONDS` | `30` | positive |
| `FROSTLINE_READINESS_TIMEOUT_SECONDS` | `60` | positive |
| `FROSTLINE_LOG_FORMAT` | `pretty` | `pretty` or `json` |
| `FROSTLINE_OUTPUT_DIRECTORY` | `runs/<run-id>` | a writable path |
| `FROSTLINE_KEEP_RUN` | `false` | a flag like the local guard, on keeps the run for inspection |

The local runtime reads a few more:

| variable | default | used by |
| --- | --- | --- |
| `FROSTLINE_IGGY_SERVER`, `FROSTLINE_PLANE` | unset | `scripts/resolve-runtime` |
| `FROSTLINE_FILTERS_MAX_CONCURRENT` | `16` | `scripts/stack-local`, filtered reads in flight per shard |

The mode comes from the command, never from a variable.

## Logs

Everything goes through `tracing` on standard output. `RUST_LOG` is taken as written. `NO_COLOR` turns color off, and a nonzero `CLICOLOR_FORCE` turns it on. Per-record logs are off by default. Errors, expired windows, and a changed source are never sampled away. No log, report, or run file holds a password, a token, or a full connection string.

## Inspecting a kept run

Set `FROSTLINE_KEEP_RUN=true` and the finite story leaves its stream, filters, and bindings in place. The run file lists every id. Against a LaserData Cloud deployment, the Console shows the stream, the saved filters with their revisions and bound groups, and offers a filter workbench with sample tests and bounded previews. Against Laser Stack or a local runtime, inspect the run with the Laser SDK or the Iggy CLI. Delete the run afterwards with `just cleanup runs/<run-id>/run.json`.

Connection settings also follow the Laser SDK conventions. `LASER_CONNECTION_STRING` supplies the complete endpoint and credentials. Alternatively use `LASER_SERVER`, `LASER_TOKEN`, `LASER_USERNAME`, `LASER_PASSWORD` and `LASER_NO_TLS` as described by `crates/shared/src/connect.rs`. Local development disables TLS. Hosted connections use TLS.

A filtered reader bounds outstanding record-bearing pages per partition. The SDK default is 1024 and is configurable. Frostline sets its bound from the checkpoint window so it can retain the complete window until its receipt is published. Empty pages do not consume separate slots. Acknowledgments release the handled prefix. The page cap does not limit topic size or records in the run.

### Local runtime and measurement overrides

| Variable | Script default |
| --- | --- |
| `FROSTLINE_HEALTH_PORT` | `8089` |
| `FROSTLINE_HEAPTRACK_OUTPUT` | `unset` |
| `FROSTLINE_HTTP_PORT` | `3000` |
| `FROSTLINE_IGGY_LD_PRELOAD` | `unset` |
| `FROSTLINE_IGGY_PASSWORD` | `laser` |
| `FROSTLINE_IGGY_USERNAME` | `iggy` |
| `FROSTLINE_PLANE_SOCKET` | `$state/plane.sock` |
| `FROSTLINE_ROUND_RECORDS` | `1000` |
| `FROSTLINE_SCAN_RECORDS` | `1000` |
| `FROSTLINE_STACK_DIR` | `$root/runs/stack` |
| `FROSTLINE_TCP_PORT` | `8090` |
