# Frostline bench codec_cbor_1m

fleet_1m encoded as CBOR. 3 repetitions, each a fresh run of the same seeded dataset: 1000000 records, 490.58 MB of payload. Every repetition selected the same records with the same payload bytes.

```sh
target/release/frostline-bench bench codec_cbor_1m --poll-records 1000
```

| group | matched | payload received | payload full feed | payload avoided | wire received | wire full feed | wire avoided |
| --- | --- | --- | --- | --- | --- | --- | --- |
| food-safety | 1973 | 1.06 MB | 490.58 MB | **99.8%** | 2.78 MB | 644.93 MB | **99.6%** |
| food-safety-current | 30961 | 16.39 MB | 490.58 MB | **96.7%** | 24.50 MB | 644.93 MB | **96.2%** |
| maintenance | 9448 | 3.63 MB | 490.58 MB | **99.3%** | 7.56 MB | 644.93 MB | **98.8%** |
| north-pharma | 92635 | 43.63 MB | 490.58 MB | **91.1%** | 61.35 MB | 644.93 MB | **90.5%** |
| all groups | | 64.71 MB | 1.96 GB | **96.7%** | 96.19 MB | 2.58 GB | **96.3%** |

Payload is the record payloads a reader was handed. TCP receive and send bytes are counted separately from successful socket syscalls under strace. The main reduction column uses received bytes, including records, headers, framing, checkpoints, login, and metadata. TCP/IP headers and retransmissions are excluded.

| group | reader CPU filtered | reader CPU full feed | peak RSS filtered | peak RSS full feed | TCP connections filtered | TCP connections full feed |
| --- | --- | --- | --- | --- | --- | --- |
| food-safety | 0.09 s | 2.85 s | 11.97 MB | 13.50 MB | 3 | 1 |
| food-safety-current | 0.20 s | 2.86 s | 11.99 MB | 14.16 MB | 3 | 1 |
| maintenance | 0.09 s | 2.86 s | 11.87 MB | 13.46 MB | 3 | 1 |
| north-pharma | 0.39 s | 2.91 s | 12.03 MB | 13.53 MB | 3 | 1 |

| group | bounded round p50 filtered | p99 | p99.9 | samples | ordinary poll p50 | p99 | p99.9 | samples |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| food-safety | 6.23 ms | 18.86 ms | 38.29 ms | 897 | 379 us | 1.60 ms | 3.08 ms | 1002 |
| food-safety-current | 6.18 ms | 7.36 ms | 19.67 ms | 1000 | 363 us | 1.41 ms | 2.49 ms | 1002 |
| maintenance | 538 us | 915 us | 3.26 ms | 1000 | 348 us | 1.51 ms | 2.66 ms | 1002 |
| north-pharma | 6.16 ms | 7.47 ms | 9.55 ms | 1002 | 390 us | 1.72 ms | 2.59 ms | 1002 |

Filtered latency times one bounded reader round. It can poll several assigned partitions and includes automatic progress stores for empty pages. Ordinary latency times an explicit native poll, with no buffered-delivery threshold. Idle sleep, handlers, and explicit application acknowledgments are outside this timer. Automatic acknowledgments of empty filtered pages remain inside the filtered round. These are client-observed call durations, not isolated server execution latency. Percentiles use nearest rank, then the median across repetitions. Up to 65,536 samples per reader are exact. Longer runs use bounded histograms with at most 3.125% relative quantile error, recorded in result.json. Small sample sets do not establish a stable p99.9 tail.

Publishing ran at 276311 records per second. The filtered read took 6.86 s and 24.35 s of iggy-server CPU, with iggy-server at 577.97 MB peak RSS. The full-feed read took 3.35 s and 3.84 s of iggy-server CPU, at 603.74 MB peak RSS. iggy-server examined 164271 records per CPU second while filtering.

| binary | version |
| --- | --- |
| frostline-bench | 0.1.0 |
| frostline-consumers | 0.1.0 |
| iggy-server | 0.9.2-ld |
| plane | 0.21.0 |
