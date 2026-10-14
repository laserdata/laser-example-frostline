# Frostline bench codec_protobuf_1m

fleet_1m encoded as Protobuf with a registered descriptor. 3 repetitions, each a fresh run of the same seeded dataset: 1000000 records, 185.74 MB of payload. Every repetition selected the same records with the same payload bytes.

```sh
target/release/frostline-bench bench codec_protobuf_1m --poll-records 1000
```

| group | matched | payload received | payload full feed | payload avoided | wire received | wire full feed | wire avoided |
| --- | --- | --- | --- | --- | --- | --- | --- |
| food-safety | 1973 | 433.23 KB | 185.74 MB | **99.8%** | 2.18 MB | 363.71 MB | **99.4%** |
| food-safety-current | 30961 | 6.25 MB | 185.74 MB | **96.6%** | 15.76 MB | 363.71 MB | **95.7%** |
| maintenance | 9448 | 1.59 MB | 185.74 MB | **99.1%** | 5.85 MB | 363.71 MB | **98.4%** |
| north-pharma | 92635 | 16.70 MB | 185.74 MB | **91.0%** | 37.88 MB | 363.71 MB | **89.6%** |
| all groups | | 24.96 MB | 742.95 MB | **96.6%** | 61.67 MB | 1.45 GB | **95.8%** |

Payload is the record payloads a reader was handed. TCP receive and send bytes are counted separately from successful socket syscalls under strace. The main reduction column uses received bytes, including records, headers, framing, checkpoints, login, and metadata. TCP/IP headers and retransmissions are excluded.

| group | reader CPU filtered | reader CPU full feed | peak RSS filtered | peak RSS full feed | TCP connections filtered | TCP connections full feed |
| --- | --- | --- | --- | --- | --- | --- |
| food-safety | 0.08 s | 1.18 s | 12.00 MB | 13.03 MB | 3 | 1 |
| food-safety-current | 0.14 s | 1.18 s | 12.06 MB | 12.98 MB | 3 | 1 |
| maintenance | 0.08 s | 1.18 s | 12.26 MB | 13.31 MB | 3 | 1 |
| north-pharma | 0.23 s | 1.23 s | 12.28 MB | 12.83 MB | 3 | 1 |

| group | bounded round p50 filtered | p99 | p99.9 | samples | ordinary poll p50 | p99 | p99.9 | samples |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| food-safety | 4.71 ms | 14.15 ms | 23.10 ms | 897 | 268 us | 1.18 ms | 1.92 ms | 1002 |
| food-safety-current | 4.71 ms | 11.58 ms | 14.66 ms | 1000 | 261 us | 1.09 ms | 1.91 ms | 1002 |
| maintenance | 503 us | 4.04 ms | 7.84 ms | 1000 | 261 us | 1.16 ms | 1.66 ms | 1002 |
| north-pharma | 4.70 ms | 6.98 ms | 7.61 ms | 1002 | 269 us | 708 us | 1.48 ms | 1002 |

Filtered latency times one bounded reader round. It can poll several assigned partitions and includes automatic progress stores for empty pages. Ordinary latency times an explicit native poll, with no buffered-delivery threshold. Idle sleep, handlers, and explicit application acknowledgments are outside this timer. Automatic acknowledgments of empty filtered pages remain inside the filtered round. These are client-observed call durations, not isolated server execution latency. Percentiles use nearest rank, then the median across repetitions. Up to 65,536 samples per reader are exact. Longer runs use bounded histograms with at most 3.125% relative quantile error, recorded in result.json. Small sample sets do not establish a stable p99.9 tail.

Publishing ran at 45112 records per second. The filtered read took 5.49 s and 18.93 s of iggy-server CPU, with iggy-server at 564.47 MB peak RSS. The full-feed read took 1.53 s and 2.07 s of iggy-server CPU, at 615.20 MB peak RSS. iggy-server examined 211305 records per CPU second while filtering.

| binary | version |
| --- | --- |
| frostline-bench | 0.1.0 |
| frostline-consumers | 0.1.0 |
| iggy-server | 0.9.2-ld |
| plane | 0.21.0 |
