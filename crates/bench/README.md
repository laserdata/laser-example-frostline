# frostline-bench

The benchmark and profiling crate. It measures real reader calls, process counters, and socket transfers. Reports derive reductions and medians from those measurements.

## What one profile does

A profile in `src/profiles.rs` fixes a seeded dataset. Each repetition is a fresh run of it:

1. Provision a run stream and its saved filters, as the demo does.
2. Publish every record, then stop the producer. Records never expire during the trial.
3. Start one `frostline-consumers` process per group with its filter. Each one reads to its last checkpoint and then holds its connections open.
4. While every reader holds, snapshot the kernel socket counters with `ss -tinpH` and read each reader's CPU and peak memory from /proc. Then release them.
5. Do the same with one full-feed reader per group, which downloads every record and filters in the application.
6. Check every window against the producer's manifests. A full-feed reader must receive exactly the source payload.

The result is refused when any window is missing, any digest differs, or two repetitions selected different records.

## Commands

```sh
cargo build --release --workspace
just up-local
target/release/frostline-bench list
just bench fleet_1m
just selectivity
just profile-memory iggy
just profile-cpu iggy
```

`just selectivity` is the exact-selectivity trial. It publishes 200,000 records of 1 KiB with a match header on every k-th record for 0, 1, 10, 100, and 1,000 per mille, reads each dataset with a headers-only filter and as the full feed, checks the exact match count both ways, and writes `selectivity.md` with the wire bytes of each.

`just up-pressure 1G` starts the local runtime inside a memory-capped cgroup, so a profile run afterwards shows the server's cgroup peak, its limit, and any OOM kill in `result.json`.

`just bench` writes `result.json`, `result.md`, and each reader's log under `runs/bench/<profile>/<time>/`. Server CPU and runtime versions need the local runtime from `just up-local`, because the bench finds its processes through `runs/stack/pids.json`.

`profile-memory` requires heaptrack and writes one /proc sample per second to `memory.jsonl`. It sends SIGINT and waits for the capture to flush. `profile-cpu` requires perf and flamegraph. A missing tool or failed capture fails the command. The bench never installs a profiler. Heaptrack cannot prove complete allocation coverage for a custom allocator. Use a separately labelled system-allocator build for heap analysis when the native binary uses mimalloc.

## Files

| file | role |
| --- | --- |
| `profiles.rs` | the profiles as constants |
| `trial/mod.rs` | one repetition: provision, publish, the two read phases, validation |
| `readers.rs` | a held `frostline-consumers` process and its log |
| `collect/process.rs` | CPU ticks, RSS, peak RSS, and PSS from /proc |
| `collect/sockets.rs` | kernel TCP counters from `ss` |
| `collect/mod.rs` | the socket ledger, which also catches sockets that closed early |
| `runtime.rs` | the local runtime processes and reported version of every binary |
| `report/mod.rs` | medians, the reproducibility check, `result.json` and `result.md` |
| `profile.rs` | perf and heaptrack wrappers and /proc memory sampling |
| `selectivity/mod.rs` | the exact-selectivity trial, both the runner and the traced reader child |

## Matched replay and streaming measurements

**Use matched source budgets when comparing latency.** Filtered latency measures a bounded reader round. Full-feed latency measures an ordinary native batch poll. Both readers decode delivered records and run the same domain handler. Their request and acknowledgment work differs. Results include p50, p99, p99.9, sample counts, and examined records per round.

```sh
scripts/replay-trial fleet_1m --nodelay --scan-records 1000 --out runs/matched-json
FROSTLINE_PROFILE_CPU=1 scripts/replay-trial fleet_1m --nodelay --scan-records 1000 --out runs/profiled-json
scripts/streaming-trial soak_5m --out runs/soak-five-minutes
scripts/streaming-trial soak_60m --out runs/soak-one-hour
systemd-run --user --scope -p MemoryMax=1G -p MemorySwapMax=0 scripts/streaming-trial pressure --soft-percent 60 --out runs/pressure-one-gib
```

Each command requires a new output directory. Replay trials start and stop their own native runtime. They record runtime versions and source/poll limits. `FROSTLINE_PROFILE_CPU=1` adds separate perf data, reports, and flamegraphs for publishing, filtered reads, and ordinary reads. Profiled runs include profiler overhead. Compare latency in runs without perf enabled.

Replay memory artifacts sample RSS and PSS every 100 ms for each active phase. The phase peak is the highest observed RSS sample. `lifetime_peak_rss_bytes` is the separate process high-water mark. Sampling can miss shorter peaks. Streaming trials write per-second CPU/RSS/PSS/cgroup samples. Pressure trials stop ingress at the configured soft threshold and require a complete drained report with no new OOM kills.

Latency storage stays bounded in infinite mode. It stores exact samples up to 65,536 calls, then uses a fixed histogram. The histogram reports its relative error bound. A p99.9 estimate from roughly 1,000 calls covers only one tail observation and needs longer trials for production decisions.

Timed and traced passes must deliver identical record counts and payload bytes. Timed trials reject duplicates. Isolated replay runs also save protected filter metrics before and after each phase. Exact client percentiles and bucketed server timings use separate boundaries. Use `scripts/replay-trial ... --nodelay` to apply Iggy's existing TCP option equally to filtered and ordinary readers. The trial records this choice.

## Additional controls

Use `--poll-records 128 --scan-records 128` together to test a smaller source budget. Keep the reported examined count visible because a filtered round can visit several partitions. Lower budgets can reduce call latency while increasing total CPU and request count.

Run `scripts/replay-trial selectivity --matrix --nodelay --out <new-directory>` for size, predicate, and skew cases. Each ordinary reader evaluates a typed application predicate. Results include separate fetch and complete fetch/process/ack percentiles. Run `scripts/fanout-trial --out <new-directory>` for three repetitions with 1, 3, and 8 independent readers.

`streaming-trial --workers 2` and `--workers 4` exercise partition-split workers. A soak must run for its configured duration. A pressure run must reach its chosen soft threshold and drain with no expired window or new OOM kill. Failures retain their reports.

Repetitions alternate filtered-first and ordinary-first order and record that choice. Role latency includes contention from four concurrent readers. Latency reports show p50, p99 and p99.9.

Use `--version` to print the binary name and release version without starting the application.

Filtered selectivity readers configure a group policy once and limit each round to 1,000 examined records. Ordinary polls use the same source-record limit.
