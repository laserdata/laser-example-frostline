# Frostline: contributor guide

Frostline is a fictional refrigerated-delivery carrier built on the Laser SDK over Apache Iggy. Hundreds of trucks publish changes into one topic. Three teams read their own slice through consumer filters, and every run measures how much payload stayed on the server. The same binaries run against Laser Stack and LaserData Cloud. The repository also holds the benchmark and profiling crate for the filter path.

## Structure

```text
crates/
  shared/     settings, names, connection, domain, event envelope, policies, codecs, measurement, output, lifecycle, topology, run file, test kit
  producer/   seeded fleet, ordered per-partition publishing, checkpoints, window manifests
  consumers/  reader loop, food safety, maintenance, regional handlers, baseline reader, receipts
  demo/       composition root: doctor, provisioning, finite, compare, live, codecs, report, cleanup
  bench/      datasets, trials, collectors, profiler wrappers, bench reports
docs/         architecture, walkthrough, filters, measurement, operations, benchmarks, sdk map
schemas/      Avro schema, Protobuf source and descriptor set
scripts/      local runtime, runtime resolver, file size gate
```

Each crate owns its tests. Unit tests sit next to the code and need no server. Integration and end to end tests live under the crate's `tests/` directory behind the `integration` and `e2e` features, so `cargo test --workspace` stays server free. The native test stack lives in `frostline-shared` behind the `testkit` feature.

## Dependencies

`laser-sdk` comes from crates.io and brings `laser-wire` with it. The owner picks its version.

## Rules

- Never commit, push, or move a ref. Leave changes in the working tree.
- Never edit files outside this repository unless the task names the repository and the file. Never edit upstream Iggy core or the Iggy Node SDK.
- Never run `cargo install` or change the toolchain. If a tool is missing, stop and say so.
- Finish every edit of a task first, then run the verification order once.
- Never invent a hostname, port, route, version, or number.

## File layout

Every Rust file reads top to bottom in this order: one sorted `use` block, all constants, types with their inherent impls, trait impls, free functions public before private in first-use order, then `#[cfg(test)] mod tests` with test cases before their fixture helpers. No `use` below the top block. A reader never jumps upward inside one scope.

Target under 250 lines per file. Hard limit 300, tests included. `scripts/max-lines 300` enforces it. Split by responsibility, not by line count.

## Code

- `#![forbid(unsafe_code)]` in every crate. `lib.rs` starts with `#![doc = include_str!("../README.md")]`. No `//!` comments.
- Terse, self-documenting code. Comments only for a decision the code cannot show, one or two lines. Never narrate the next lines, never mention plans, tasks, or history.
- Domain newtypes over primitives. Enums over strings with `strum` and `serde(rename_all = "snake_case")`.
- Every error is a `thiserror` enum with meaningful variants.
- `.expect("meaningful message")`, never a bare `.unwrap()`.
- No trivial wrapper functions that forward one call.
- Runtime output goes through `tracing`. The only `println!` is the version banner before tracing starts.
- Log messages carry ids in both structured fields and the message text.
- No em dash, en dash, or semicolon in any comment, string, log line, error, or document.
- No customer names or real company data. Trucks, depots, and regions are invented.
- Wire op versions stay 1.

## Tests

- Names are `given_<state>_when_<action>_then_should_<outcome>`.
- Assert typed values or exact `serde_json::json!` values.
- A test never uses the implementation as its only oracle. Filter tests compare the compiled filter with a hand-written typed predicate.
- No fixed sleeps in integration tests. Use `eventually`. No `#[ignore]`.
- One stream per integration test.

## Prose

Docs, READMEs, comments, help text, log lines, and errors use plain direct English. Lead with the point. Procedural text is imperative, at most 20 words per sentence, one instruction per sentence. Descriptive text uses simple tenses, at most 25 words per sentence, at most six sentences per paragraph. Use the common word. Define a concept term once, in under ten words. Never hard-wrap a paragraph.

Never use: delve, dive into, navigate, underscore, bolster, foster, harness, leverage, unpack, pivotal, groundbreaking, cutting-edge, transformative, innovative, robust, comprehensive, seamless, intricate, nuanced, vibrant, holistic, testament, landscape, realm. Never write "It's not just X, it's Y", "Not only X but Y", or the bold-term-colon list format. No sweeping openings, no summary closings.

## Verification order

```text
cargo fmt --all
cargo sort --workspace
cargo machete --with-metadata
scripts/max-lines 300
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
cargo test --workspace --all-features --doc
just test-it
just e2e
```

`just ci` runs the whole sequence. Docs are part of every change. A crate change updates its README and the root README tables in the same change.

## Public information

Public documentation, examples and published measurement reports must use component versions. Do not disclose local machine paths, private repository names, build hashes or internal review references. Keep detailed build provenance in private review artifacts.
