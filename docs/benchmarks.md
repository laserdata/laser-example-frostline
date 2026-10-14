# Benchmarks

Reference measurements retain the workload, correctness checks and binary versions. Saved filters confirm the local catalog version before cached reuse. Every result records binary versions, input counts, correctness checks, and separate timing and socket traces.

**The ten-million-record run saved 96.5% of total TCP application traffic.** Filtering moved work from readers to Iggy. Combined measured CPU was 107.46 seconds with filters and 98.15 seconds with ordinary readers, about 9.5% more.

These historical results predate the group-owned SDK 0.5.1 migration. They are local loopback measurements from release Iggy and release clients, with a debug plane. They do not establish capacity or latency on another deployment. Builds and other load tests did not overlap the measured phases.

## Ten million records

Source: `measurements/fleet_10m/measurements/result.json` and `result.md`. One repetition reads ten million JSON records across four partitions with four saved filters.

| Metric | Filtered | Ordinary |
| --- | ---: | ---: |
| Delivered payload, MB | 777.64 | 25,210.16 |
| TCP application receive bytes | 1,071,963,143 | 31,382,126,136 |
| TCP application send bytes | 33,481,430 | 17,296,524 |
| TCP application bytes, both directions | 1,105,444,573 | 31,399,422,660 |
| Iggy CPU, seconds | 99.50 | 31.32 |
| Plane CPU, seconds | 2.05 | 0.05 |
| Reader CPU, seconds | 5.91 | 66.78 |
| Combined CPU, seconds | 107.46 | 98.15 |
| Read phase wall time, seconds | 29.97 | 23.38 |
| Sampled Iggy peak RSS, MB | 590.63 | 662.07 |

Payload transfer fell by 96.9%. Socket counts exclude TCP/IP headers and retransmissions. Combined CPU includes Iggy, plane, and the four readers. It excludes the reporter.

## Client latency

All latency values below are milliseconds, measured with four concurrent readers per arm. Filtered measurements cover bounded reader rounds, which can visit several partitions and acknowledge empty pages. Ordinary measurements cover one native poll. Handlers and explicit application acknowledgments are outside both timers. First-call setup stays in the percentiles.

| Role | Filtered p50 | p99 | p99.9 | Calls | Ordinary p50 | p99 | p99.9 | Calls |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| food-safety | 3.000 | 9.343 | 13.760 | 8,684 | 0.658 | 2.213 | 3.322 | 10,003 |
| food-safety-current | 2.906 | 4.212 | 9.551 | 9,997 | 0.684 | 1.881 | 3.129 | 10,003 |
| maintenance | 1.453 | 3.429 | 6.525 | 9,981 | 0.714 | 1.787 | 3.044 | 10,003 |
| north-pharma | 2.648 | 3.969 | 7.540 | 10,003 | 0.629 | 1.986 | 2.999 | 10,003 |

Filtered rounds examined about 999 to 1,151 records on average. Ordinary polls returned about 999 records. The JSON includes exact counts and maxima. Filtering does not promise the same per-call latency as ordinary delivery.

## Codec costs

Each row is the median of three million-record repetitions. CPU values are seconds. Latency values are milliseconds. The payload role is food-safety-current, and maintenance uses headers only.

| Codec | Iggy CPU filtered | Iggy CPU ordinary | Payload p50 | Header p50 | Ordinary payload-role p50 |
| --- | ---: | ---: | ---: | ---: | ---: |
| JSON | 8.60 | 3.02 | 1.960 | 0.585 | 0.414 |
| CBOR | 24.35 | 3.84 | 6.176 | 0.538 | 0.363 |
| Protobuf | 18.93 | 2.07 | 4.710 | 0.503 | 0.261 |
| Avro | 15.62 | 5.39 | 3.866 | 0.444 | 0.288 |

The older trials always ran filtering first, so the server heap was warmer for the ordinary arm. Peak RSS is a sequential-run observation, not an isolated allocation comparison. New repetitions alternate arm order and record `filtered_first` in JSON.

All codecs selected the same logical records. Encoded sizes differ by codec. Repetitions within one codec agreed on selected counts and payload bytes. Each codec directory under `measurements/` contains every role, p50/p99/p99.9, memory, CPU, and traffic tables.

## Cache correctness and cost

The broker caches resolved definitions, but confirms the local catalog version before reusing one. Watch delivery alone cannot guarantee that the next request sees an applied change. A changed version invalidates old entries. Current grants are checked again after confirmation, and a plane restart uses a new version token.

In the million-record run in `measurements/fleet_1m`, Iggy CPU was 8.60 seconds, plane CPU was 0.18 seconds, and filtered wall time was 2.47 seconds. Ordinary Iggy CPU was 3.02 seconds and wall time was 2.14 seconds. Confirmations remain real sidecar work, even when a definition is cached.

## Profiles and memory evidence

CPU profiles separate publishing, filtered reads and ordinary reads. Profiled timings are diagnostic and remain outside the latency tables.

Long-running validation checks contiguous receipt windows and memory stability. Reproduce it with the streaming commands below.

The recorded 1 GiB pressure run had swap disabled. Ingress stopped at 127.53 seconds when usage reached 644,349,952 bytes against a 644,245,094-byte soft threshold. All 104 windows and 207,616 records drained, with zero expired windows and no OOM kill. Artifacts are `measurements/{soak-5m,pressure-1g}`.

## Profiles and commands

| profile | records | partitions | codec | catalog | repetitions |
| --- | --- | --- | --- | --- | --- |
| `smoke` | 20,000 | 2 | JSON | saved | 1 |
| `fleet_1m` | 1,000,000 | 4 | JSON | saved | 3 |
| `fleet_10m` | 10,000,000 | 4 | JSON | saved | 1 |
| `inline_1m` | 1,000,000 | 4 | JSON | inline | 3 |
| `codec_cbor_1m` | 1,000,000 | 4 | CBOR | saved | 3 |
| `codec_avro_1m` | 1,000,000 | 4 | Avro | saved | 3 |
| `codec_protobuf_1m` | 1,000,000 | 4 | Protobuf | saved | 3 |

```sh
scripts/replay-trial fleet_1m --nodelay --out runs/fleet-1m
scripts/replay-trial selectivity --matrix --nodelay --out runs/matrix
scripts/fanout-trial --repetitions 3 --out runs/fanout
scripts/streaming-trial soak_60m --workers 4 --out runs/soak-60m
systemd-run --user --scope -p MemoryMax=1G -p MemorySwapMax=0 scripts/streaming-trial pressure --soft-percent 60 --out runs/pressure
scripts/summarize-trials runs/fleet-1m runs/matrix --out runs/summary.md
```

The pressure command needs systemd user scopes with memory control. Each script starts its own Iggy and plane and writes `result.json`. Replay also writes `result.md` and records binary versions. Streaming writes JSON and binary versions. Fanout writes JSON and records phase usage, without a binary-version table. Use a fresh directory per run, and run nothing else heavy during the timed phases. `just bench <profile>` and `just selectivity` run the same trials against an already running `just up-local` stack, and `just up-pressure 1G` starts that stack inside a memory-capped cgroup.

## Profiling

```sh
just profile-cpu iggy 30
just profile-memory iggy 30
FROSTLINE_PROFILE_CPU=1 scripts/replay-trial fleet_1m --nodelay --out runs/profiled
```

`profile-cpu` samples a runtime process with `perf record` and `perf stat` for the given seconds and renders `flamegraph.svg`. `profile-memory` writes one /proc sample per second to `memory.jsonl` and attaches heaptrack. The profiled replay captures every phase separately. Both write the exact commands next to their output. A profiled run is slower than an unprofiled one and is never used for latency figures.

## Tools

The bench measures with standard Linux tools. It never installs them and never changes kernel settings. A missing tool stops the command that needs it with a sentence that names it.

| tool | used by | needed for |
| --- | --- | --- |
| `ss` from iproute2 | `just bench` | kernel TCP counters of each reader process |
| `strace` | `just bench` | exact socket bytes of each reader, from its syscalls |
| `getconf` | `just bench` | the clock tick unit of /proc CPU times |
| `/usr/bin/time` | `just selectivity` | CPU and peak memory of each child reader |
| `perf` | `just profile-cpu` | CPU samples, call graphs, and hardware counters |
| `flamegraph` | `just profile-cpu` | an SVG flamegraph from `perf.data` |
| `heaptrack` | `just profile-memory` | allocation profiles |

### Install

On Ubuntu and Debian-based systems:

```sh
sudo apt install iproute2 strace heaptrack heaptrack-gui time
cargo install flamegraph
```

`perf` has to match the running kernel. Newer kernels ship it as `linux-perf`, older ones inside `linux-tools-<kernel>`. Try the first and fall back to the second:

```sh
sudo apt install linux-perf || sudo apt install linux-tools-common linux-tools-$(uname -r)
perf --version
```

On Fedora:

```sh
sudo dnf install iproute strace perf heaptrack time
cargo install flamegraph
```

On Arch:

```sh
sudo pacman -S iproute2 strace perf heaptrack time
cargo install flamegraph
```

### Kernel settings

Two settings decide what a normal user may profile. Check them first:

```sh
cat /proc/sys/kernel/perf_event_paranoid /proc/sys/kernel/yama/ptrace_scope
```

`perf_event_paranoid` at 2, the usual default, lets `perf` sample your own processes in user space only. That is enough for the flamegraphs of iggy-server, plane, and the Frostline binaries. Kernel frames and some hardware counters need 1 or lower, until the next reboot:

```sh
sudo sysctl kernel.perf_event_paranoid=1
```

`ptrace_scope` at 1 lets a process trace only its own children. The bench starts every reader it traces, so `strace` works as is. Attaching `heaptrack` to an iggy-server that is already running needs 0, until the next reboot:

```sh
sudo sysctl kernel.yama.ptrace_scope=0
```

Put both back afterwards with the values the first command printed.

## Reference comparison

One million JSON records over four partitions, saved filters. 3 repetitions, each a fresh run of the same seeded dataset: 1000000 records, 625.73 MB of payload. Every repetition selected the same records with the same payload bytes.

```sh
target/release/frostline-bench bench fleet_1m --poll-records 1000
```

| group | matched | payload received | payload full feed | payload avoided | wire received | wire full feed | wire avoided |
| --- | --- | --- | --- | --- | --- | --- | --- |
| food-safety | 1973 | 1.32 MB | 625.73 MB | **99.8%** | 3.05 MB | 780.10 MB | **99.6%** |
| food-safety-current | 30961 | 20.53 MB | 625.73 MB | **96.7%** | 28.64 MB | 780.10 MB | **96.3%** |
| maintenance | 9448 | 4.93 MB | 625.73 MB | **99.2%** | 8.87 MB | 780.10 MB | **98.9%** |
| north-pharma | 92635 | 56.50 MB | 625.73 MB | **91.0%** | 74.24 MB | 780.10 MB | **90.5%** |
| all groups | | 83.28 MB | 2.50 GB | **96.7%** | 114.81 MB | 3.12 GB | **96.3%** |

Payload is the record payloads a reader was handed. TCP receive and send bytes are counted separately from successful socket syscalls under strace. The main reduction column uses received bytes, including records, headers, framing, checkpoints, login, and metadata. TCP/IP headers and retransmissions are excluded.

| group | reader CPU filtered | reader CPU full feed | peak RSS filtered | peak RSS full feed | TCP connections filtered | TCP connections full feed |
| --- | --- | --- | --- | --- | --- | --- |
| food-safety | 0.07 s | 1.63 s | 12.14 MB | 14.94 MB | 3 | 1 |
| food-safety-current | 0.14 s | 1.65 s | 12.04 MB | 13.95 MB | 3 | 1 |
| maintenance | 0.08 s | 1.65 s | 11.76 MB | 14.89 MB | 3 | 1 |
| north-pharma | 0.28 s | 1.71 s | 11.91 MB | 13.85 MB | 3 | 1 |

| group | bounded round p50 filtered | p99 | p99.9 | samples | ordinary poll p50 | p99 | p99.9 | samples |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| food-safety | 2.18 ms | 6.59 ms | 33.16 ms | 897 | 400 us | 1.47 ms | 2.37 ms | 1002 |
| food-safety-current | 1.96 ms | 2.97 ms | 5.05 ms | 1000 | 414 us | 1.53 ms | 2.50 ms | 1002 |
| maintenance | 585 us | 1.01 ms | 2.28 ms | 1000 | 416 us | 1.32 ms | 2.02 ms | 1002 |
| north-pharma | 2.03 ms | 3.16 ms | 4.47 ms | 1002 | 422 us | 1.28 ms | 2.07 ms | 1002 |

Filtered latency times one bounded reader round. It can poll several assigned partitions and includes automatic progress stores for empty pages. Ordinary latency times an explicit native poll, with no buffered-delivery threshold. Idle sleep, handlers, and explicit application acknowledgments are outside this timer. Automatic acknowledgments of empty filtered pages remain inside the filtered round. These are client-observed call durations, not isolated server execution latency. Percentiles use nearest rank, then the median across repetitions. Up to 65,536 samples per reader are exact. Longer runs use bounded histograms with at most 3.125% relative quantile error, recorded in result.json. Small sample sets do not establish a stable p99.9 tail.

Publishing ran at 383133 records per second. The filtered read took 2.47 s and 8.60 s of iggy-server CPU, with iggy-server at 585.14 MB peak RSS. The full-feed read took 2.14 s and 3.02 s of iggy-server CPU, at 638.92 MB peak RSS. iggy-server examined 465116 records per CPU second while filtering.

| binary | version |
| --- | --- |
| frostline-bench | 0.1.0 |
| frostline-consumers | 0.1.0 |
| iggy-server | 0.9.2-ld |
| plane | 0.21.0 |
