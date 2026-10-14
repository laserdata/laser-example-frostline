# SDK map

Every Laser SDK surface Frostline uses, where it is used, and the test that runs it against a real server.

## Filters

| surface | used in | proved by |
| --- | --- | --- |
| `FilterExpr::pred`, `pred_as` with `Coerce::Number`, `header`, `all`, `any` | `crates/shared/src/policy/` | policy unit tests, 500 generated events per team against a hand-written predicate |
| `CmpOp::In` with `TypedValue::List`, `CmpOp::Contains` | `policy/safety.rs`, `policy/regional.rs` | same |
| `ConsumerFilter::json`, `cbor`, `avro`, `protobuf`, `headers_only` | `codec/mod.rs`, `policy/mod.rs` | codec unit tests, `just codecs` |
| `FilterExpr::header_text`, `TextMatch::{Glob, Contains}`, `case_insensitive` | `demo/src/mixed.rs` | the mixed log assertions in `given_the_managed_story_when_run_then_should_receive_less_than_the_feed_every_team` |
| `ConsumerFilter::with_mismatch_policy(RecordPolicy::Pass)`, foreign records skipped by default | `demo/src/mixed.rs` | same |
| `group.filter().preview(..).explain(true)` | `demo/src/mixed.rs` | same |
| `group.filter().test` | `demo/src/provision.rs` | every demo run and the end to end tests |
| `topic.consumer_group(name).create().filter(definition).build()` | `provision.rs`, `walkthrough.rs` | the managed story |
| `group.filter().preview` | `walkthrough.rs` | same |
| `group.filter().set_revision_enabled`, `FilterErrorReason::RevisionDisabled` | `walkthrough.rs` | the pause and resume steps of the managed story |
| Administrative `FilterMutation::Unbind` and `Drop` with stable operation ids | `demo/src/cleanup.rs` | every finite run ends with them |

## Filtered reads

| surface | used in | proved by |
| --- | --- | --- |
| `topic.consumer_group_id(id).reader()` with `start`, `count`, `max_reply_bytes`, `local_guard`, `idle_interval` | `consumers/src/lib.rs` | `consumers/tests/integration.rs` |
| `reader().partition(id)` | `consumers/src/lib.rs` | the multiworker story assigns independent worker groups their partition subsets |
| `read_round`, `examined_in_round`, `ack_through`, `close` | `consumers/src/reader.rs` | receipts match the producer manifests in every test |
| `next_record`, `try_next_page`, `ack`, `close` | `demo/src/walkthrough.rs` | the managed story |
| `data_connections_opened` | `consumers/src/reader.rs`, read every round | the connection-opening latency series in each receipt |

## Ordinary streaming

| surface | used in | proved by |
| --- | --- | --- |
| `stream().topic().producer()` with `batch_length` | `producer/src/lib.rs`, `shared/src/reports.rs` | `producer/tests/integration.rs` |
| `consumer(name, partition)`, `start_at(ConsumerStart::First)`, `commit_policy(CommitPolicy::Disabled)`, `allow_replay`, `next_within`, `shutdown` | `shared/src/reports.rs` | the receipt reader of every report |
| native `ensure_consumer_group`, `join_consumer_group`, `poll_messages`, `store_consumer_offset`, `leave_consumer_group` | `consumers/src/poll.rs` | the compare story, where each full-feed reader receives the whole feed |

## Schemas and codecs

| surface | used in | proved by |
| --- | --- | --- |
| `schemas().register`, `get`, `drop` | `shared/src/codec/schemas.rs` | `just codecs` |
| `CompiledSchema` | `codec/schemas.rs` | Avro round trips in the codec unit tests |
| `agdx.ct` and `agdx.sid` headers | `codec/mod.rs` | `given_a_business_record_when_headers_are_built_then_should_carry_typed_values` |

## Connection and capabilities

| surface | used in | proved by |
| --- | --- | --- |
| `LaserFactory` with `Capabilities::OPEN` | `shared/src/connect.rs` | every test connects through it |
| `refresh_capabilities`, `filters.native`, `filters.catalog`, codec capabilities | `demo/src/doctor.rs` | `just doctor` |

The benchmark payload predicates use `FilterExpr::text` in `bench/src/selectivity/cases.rs`. The showcase loop uses the public bounded-round API, so it counts examined records even when a round delivers nothing.
