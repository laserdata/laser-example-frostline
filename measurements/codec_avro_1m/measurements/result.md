# Frostline bench codec_avro_1m

fleet_1m encoded as Avro with a registered writer schema. 3 repetitions, each a fresh run of the same seeded dataset: 1000000 records, 164.81 MB of payload. Every repetition selected the same records with the same payload bytes.

```sh
target/release/frostline-bench bench codec_avro_1m --poll-records 1000
```

| group | matched | payload received | payload full feed | payload avoided | wire received | wire full feed | wire avoided |
| --- | --- | --- | --- | --- | --- | --- | --- |
| food-safety | 1973 | 384.53 KB | 164.81 MB | **99.8%** | 2.13 MB | 341.47 MB | **99.4%** |
| food-safety-current | 30961 | 5.50 MB | 164.81 MB | **96.7%** | 14.49 MB | 341.47 MB | **95.8%** |
| maintenance | 9448 | 1.46 MB | 164.81 MB | **99.1%** | 5.63 MB | 341.47 MB | **98.4%** |
| north-pharma | 92635 | 14.91 MB | 164.81 MB | **91.0%** | 34.96 MB | 341.47 MB | **89.8%** |
| all groups | | 22.26 MB | 659.24 MB | **96.6%** | 57.21 MB | 1.37 GB | **95.8%** |

Payload is the record payloads a reader was handed. TCP receive and send bytes are counted separately from successful socket syscalls under strace. The main reduction column uses received bytes, including records, headers, framing, checkpoints, login, and metadata. TCP/IP headers and retransmissions are excluded.

| group | reader CPU filtered | reader CPU full feed | peak RSS filtered | peak RSS full feed | TCP connections filtered | TCP connections full feed |
| --- | --- | --- | --- | --- | --- | --- |
| food-safety | 0.10 s | 5.08 s | 12.53 MB | 13.50 MB | 3 | 1 |
| food-safety-current | 0.30 s | 5.14 s | 12.98 MB | 13.60 MB | 3 | 1 |
| maintenance | 0.11 s | 5.05 s | 12.82 MB | 13.71 MB | 3 | 1 |
| north-pharma | 0.63 s | 5.25 s | 12.76 MB | 13.30 MB | 3 | 1 |

| group | bounded round p50 filtered | p99 | p99.9 | samples | ordinary poll p50 | p99 | p99.9 | samples |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| food-safety | 3.89 ms | 11.54 ms | 35.14 ms | 897 | 291 us | 1.48 ms | 2.87 ms | 1002 |
| food-safety-current | 3.87 ms | 6.00 ms | 8.88 ms | 1000 | 288 us | 1.52 ms | 2.58 ms | 1002 |
| maintenance | 444 us | 778 us | 1.63 ms | 1000 | 259 us | 1.31 ms | 2.38 ms | 1002 |
| north-pharma | 3.83 ms | 4.78 ms | 6.92 ms | 1002 | 289 us | 1.26 ms | 3.04 ms | 1002 |

Filtered latency times one bounded reader round. It can poll several assigned partitions and includes automatic progress stores for empty pages. Ordinary latency times an explicit native poll, with no buffered-delivery threshold. Idle sleep, handlers, and explicit application acknowledgments are outside this timer. Automatic acknowledgments of empty filtered pages remain inside the filtered round. These are client-observed call durations, not isolated server execution latency. Percentiles use nearest rank, then the median across repetitions. Up to 65,536 samples per reader are exact. Longer runs use bounded histograms with at most 3.125% relative quantile error, recorded in result.json. Small sample sets do not establish a stable p99.9 tail.

Publishing ran at 59487 records per second. The filtered read took 4.68 s and 15.62 s of iggy-server CPU, with iggy-server at 552.16 MB peak RSS. The full-feed read took 5.69 s and 5.39 s of iggy-server CPU, at 594.85 MB peak RSS. iggy-server examined 256082 records per CPU second while filtering.

| binary | version |
| --- | --- |
| frostline-bench | 0.1.0 |
| frostline-consumers | 0.1.0 |
| iggy-server | 0.9.2-ld |
| plane | 0.21.0 |
