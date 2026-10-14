# Frostline bench fleet_1m

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
