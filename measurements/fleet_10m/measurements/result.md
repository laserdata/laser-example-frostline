# Frostline bench fleet_10m

Ten million JSON records over four partitions, saved filters. 1 repetitions, each a fresh run of the same seeded dataset: 10000000 records, 6.30 GB of payload. Every repetition selected the same records with the same payload bytes.

```sh
target/release/frostline-bench bench fleet_10m --poll-records 1000
```

| group | matched | payload received | payload full feed | payload avoided | wire received | wire full feed | wire avoided |
| --- | --- | --- | --- | --- | --- | --- | --- |
| food-safety | 19394 | 13.07 MB | 6.30 GB | **99.8%** | 28.54 MB | 7.85 GB | **99.6%** |
| food-safety-current | 302536 | 201.88 MB | 6.30 GB | **96.8%** | 280.78 MB | 7.85 GB | **96.4%** |
| maintenance | 92509 | 48.73 MB | 6.30 GB | **99.2%** | 86.11 MB | 7.85 GB | **98.9%** |
| north-pharma | 834808 | 513.97 MB | 6.30 GB | **91.8%** | 676.54 MB | 7.85 GB | **91.4%** |
| all groups | | 777.64 MB | 25.21 GB | **96.9%** | 1.07 GB | 31.38 GB | **96.6%** |

Payload is the record payloads a reader was handed. TCP receive and send bytes are counted separately from successful socket syscalls under strace. The main reduction column uses received bytes, including records, headers, framing, checkpoints, login, and metadata. TCP/IP headers and retransmissions are excluded.

| group | reader CPU filtered | reader CPU full feed | peak RSS filtered | peak RSS full feed | TCP connections filtered | TCP connections full feed |
| --- | --- | --- | --- | --- | --- | --- |
| food-safety | 0.74 s | 16.68 s | 11.32 MB | 15.70 MB | 3 | 1 |
| food-safety-current | 1.50 s | 16.67 s | 11.96 MB | 15.53 MB | 3 | 1 |
| maintenance | 0.86 s | 16.36 s | 11.31 MB | 15.59 MB | 3 | 1 |
| north-pharma | 2.81 s | 17.07 s | 12.13 MB | 16.15 MB | 3 | 1 |

| group | bounded round p50 filtered | p99 | p99.9 | samples | ordinary poll p50 | p99 | p99.9 | samples |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| food-safety | 3.00 ms | 9.34 ms | 13.76 ms | 8684 | 658 us | 2.21 ms | 3.32 ms | 10003 |
| food-safety-current | 2.91 ms | 4.21 ms | 9.55 ms | 9997 | 684 us | 1.88 ms | 3.13 ms | 10003 |
| maintenance | 1.45 ms | 3.43 ms | 6.53 ms | 9981 | 714 us | 1.79 ms | 3.04 ms | 10003 |
| north-pharma | 2.65 ms | 3.97 ms | 7.54 ms | 10003 | 629 us | 1.99 ms | 3.00 ms | 10003 |

Filtered latency times one bounded reader round. It can poll several assigned partitions and includes automatic progress stores for empty pages. Ordinary latency times an explicit native poll, with no buffered-delivery threshold. Idle sleep, handlers, and explicit application acknowledgments are outside this timer. Automatic acknowledgments of empty filtered pages remain inside the filtered round. These are client-observed call durations, not isolated server execution latency. Percentiles use nearest rank, then the median across repetitions. Up to 65,536 samples per reader are exact. Longer runs use bounded histograms with at most 3.125% relative quantile error, recorded in result.json. Small sample sets do not establish a stable p99.9 tail.

Publishing ran at 120844 records per second. The filtered read took 29.97 s and 99.50 s of iggy-server CPU, with iggy-server at 590.63 MB peak RSS. The full-feed read took 23.38 s and 31.32 s of iggy-server CPU, at 662.07 MB peak RSS. iggy-server examined 402010 records per CPU second while filtering.

| binary | version |
| --- | --- |
| frostline-bench | 0.1.0 |
| frostline-consumers | 0.1.0 |
| iggy-server | 0.9.2-ld |
| plane | 0.21.0 |
