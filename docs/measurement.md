# Measurement

Frostline shows savings without letting consumer lag look like savings. This page says what is counted, where each number comes from, and what is left out.

## Three numbers that never mix

| number | source | what it can claim |
| --- | --- | --- |
| payload received | the exact payload buffers a reader was handed | this reader received this many payload bytes |
| full feed | the producer's confirmed window manifests, or a real full-feed reader with `just compare` | how much payload a team avoided compared with reading everything |
| TCP receive and send bytes | successful socket syscall counts in a separate strace pass, with ss snapshots as a cross-check | measured application traffic, excluding TCP/IP headers and retransmissions |

Payload leaves out framing, batch headers, user headers, TLS, acknowledgments, login, previews, schema lookups, and the reports topic. It is exact and repeatable, but it is not a network total. Every size is the length of the encoded buffer that was published or received. Nothing is decoded and encoded again to get a length.

## Formulas

For one completed window:

- `B` is the business payload bytes the producer confirmed, and `N` is the business record count.
- `D` is the payload bytes one group received, redeliveries included.
- `M` is the records that group selected, each counted once.
- `K` is the number of filtered groups in the aggregate.

A group avoided `1 - D / B` of the payload, and its selectivity is `M / N`. The aggregate compares every filtered group with reading the full feed once each, so it avoided `1 - sum(D) / (K * B)`.

Workers of one group are one subscription. The A/B group is its own subscription, so it counts in the aggregate. Two groups that select the same record each count that delivery.

When `B` is 0 the report prints "not applicable". A window that is not complete never prints a percentage. A negative figure, which a redelivery storm can cause, is printed as it is.

## Windows

Every 2,000 records the producer closes a window:

1. It waits until the server confirms every record of the window.
2. It appends one checkpoint to every partition, including a partition that had no records in the window.
3. It waits until the server confirms every checkpoint.
4. It publishes a window manifest to the reports topic.

The manifest holds, for each partition, the record count, the payload bytes, the checkpoint bytes, and, for each group, the matches, their bytes, and their digest. The producer finds the expected matches with the same plain predicates the policy tests use.

Checkpoints are counted in their own column. They never hide inside the business figure.

## Receipts

Each reader keeps counters and a rolling digest per partition and window. The digest is SHA-256 over the event id, the sequence, and the encoded payload of each selected record, each part prefixed with its length, so the same records in another order give another digest.

At a checkpoint the reader publishes its receipt, then acknowledges the processed prefix through the checkpoint. Later records in the same page remain pending. A receipt holds the received records and bytes, the matches and their bytes, the digest, the faults, and the duplicates.

A redelivered record adds to received bytes and to the duplicate count. It never adds a second match or reaches the handler again. Once published, a receipt stays unchanged so a repeated checkpoint can publish the identical receipt. Retries after that checkpoint count in the reader summary, outside the closed-window receipt. Performance trials reject redeliveries rather than treating their payload savings as a clean comparison.

Receipts are at least once. The reporter keys each one by group, full-feed flag, and partition inside its window. An identical repeat changes nothing. A repeat with different content marks the window as conflicting.

## Completion

A window is complete when its manifest is in and every subscription sent a receipt for every partition, with a digest that equals the manifest's. Complete windows fold into the totals in order. One open window holds back every later window, so a slow reader shows up as pending and never as savings.

| state | meaning |
| --- | --- |
| complete | every receipt is in and every digest matches |
| pending | some receipts or the manifest are missing |
| conflicting | two different receipts arrived for one slot |
| mismatched | a digest differs from the manifest, which fails a finite run |

With `just compare` each full-feed reader must produce the same digest as the manifest too. So the filtered and the full-feed path provably selected the same records.

## Bounded memory

The reporter holds at most 120 open windows by default. Past that cap it drops the oldest open window and adds it to an expired count. The windows after it can fold again, and the report marks the run as incomplete. Nothing is completed silently.

## Broker counters

The server's filter metrics count examined bytes including frame headers, and matched payload bytes. They mix every read, preview, and retry on the server, and they need the `manage_servers` permission. The isolated replay runner saves them before and after each timed phase as diagnostics. These snapshots run outside the CPU and latency intervals. The example itself never needs them, and their ratio is never reported as savings.

A filtered reader stores progress through a checkpoint only after it publishes that window's receipt. A checkpoint inside a page leaves later records pending. The reader does not retain delivered payloads between pages. On resume, the reader checks its durable checkpoint and recognizes a final checkpoint before waiting for new records.

## Latency boundaries

The filtered timer covers one bounded reader round. It can include more than one partition or owner read. The ordinary timer covers one native batch poll. Reports include p50, p99, p99.9, sample count, and examined records per call. Small sample counts do not establish stable extreme tails.

Server histograms separately time the filtered handler and synchronous evaluation chunks. Their fixed buckets give percentile bounds. They do not measure ordinary server polls. Raw server cost is compared through phase CPU and RSS/PSS, with raw client latency measured directly. Profiled timings remain separate from unprofiled claims.

The receipt cache retains the most recently closed window per partition. A checkpoint from an older window stops the reader before publishing a conflicting receipt or acknowledging it. Rebuild the application state through an explicit replay when recovery needs older receipts.

The main wire-reduction column compares received bytes. Sent control traffic is stored separately. The ten-million-record run received 1,071,963,143 bytes with filters and 31,382,126,136 without them. Including sends gives 1,105,444,573 versus 31,399,422,660 bytes, a 96.5% reduction in total TCP application traffic.
