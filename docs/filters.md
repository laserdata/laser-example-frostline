# Filters

Each team's selection lives in one file under `crates/shared/src/policy/`. A file holds two things. One is the filter the server runs. The other is a plain Rust predicate that says the same thing. The tests run both on the same events and they must agree, so a filter is never checked against itself.

## The checkpoint branch

Every team's filter is wrapped the same way:

```rust
FilterExpr::any([checkpoint_branch(), team_expression])
```

`checkpoint_branch` reads the `frostline.frame` header and accepts value 1. The server evaluates header children before payload children, and `any` stops at the first match. So a checkpoint is accepted without decoding its payload. This is how a reader that matches nothing still reaches the end of every window.

## Food safety

Food safety wants a truck that just turned unsafe, or a pharma or frozen truck that left the fleet.

```rust
all([
    kind == "change",
    any([
        all([
            change.op == "update",
            change.changed contains "temperature_band",
            change.before.temperature_band != "unsafe",
            change.after.temperature_band == "unsafe",
        ]),
        all([
            change.op == "delete",
            cargo in ["pharma", "frozen"],
        ]),
    ]),
])
```

The first branch needs change evidence. A truck that is already unsafe keeps sending battery and door updates. Its current band is unsafe in every one of them, but none of them is news. Asking for the band in `changed`, and for the old band to be something else, keeps only the moment the truck crossed the line.

### The A/B revision

`food-safety-current` owns a separate policy that drops the change evidence:

```rust
all([
    kind == "change",
    change.op == "update",
    change.after.temperature_band == "unsafe",
])
```

It selects every update of an unsafe truck. Run both and the report shows the cost of a looser filter in received bytes.

## Maintenance

Maintenance wants refrigeration faults at error severity or above.

```rust
ConsumerFilter::headers_only(all([
    header frostline.unit == "reefer",
    header frostline.severity >= 2,
]))
```

The filter reads only headers, so the server never decodes a payload for this team. That holds in every codec. Header values keep their type. The producer writes severity as a uint8, so the number 2 matches and the text "2" does not. Setup proves this against the server before any record exists.

## North pharma

Regional operations wants telemetry from pharma and frozen trucks in the north, and changes to those trucks when the declared load is 10 tonnes or more.

```rust
all([
    region == "north",
    cargo in ["pharma", "frozen"],
    any([
        kind == "telemetry",
        all([
            kind == "change",
            change.after.declared_weight_tonnes >= "10.00" as number,
        ]),
    ]),
])
```

The weight is a decimal string in the payload, such as "12.50". `Coerce::Number` makes the server compare it as a number. The tests put it on both sides of the boundary, 10.00 and 9.99.

## Text matching

Any payload field or header can be matched as text, by `equals`, `prefix`, `suffix`, `contains`, `glob`, or `regex`, optionally ignoring case:

```rust
FilterExpr::header_text("event.type", TextMatch::Glob, "depot.*.v1.*")
FilterExpr::header_text("event.type", TextMatch::Contains, ".V1.").case_insensitive()
FilterExpr::text("truck_id", TextMatch::Regex, r"^FR-00[0-9]$")
```

The server runs regex matching in bounded linear time for a compiled pattern. A filter permits four compiled glob or regex predicates, with 256 KiB per program. Rust and Python can verify regex locally. TypeScript sends patterns to the server and refuses regex local guards. See the benchmark tables for full-call costs, which include more than matching.

## Records the filter cannot judge

Two record policies decide what happens to a record outside the filter's terms. Both default to `reject`.

| policy | covers | `reject` | `pass` |
| --- | --- | --- | --- |
| `foreign_policy` | a record whose `agdx.ct` names another codec, or whose writer schema is missing or not listed | skipped, never decoded | delivered unevaluated |
| `mismatch_policy` | a field that exists with a type the predicate cannot compare, such as `region: 7` | skipped | delivered unevaluated |

A missing field is never a mismatch. A payload broken in the filter's own codec is a fault and follows the fault policy.

## One log, many kinds of events

Every producer can publish into one ordered log. Stamp a routing header such as `event.type` and the content type, then let each service select its subdomain with a header text match. A `headers_only` filter works across every codec and costs nanoseconds per record. Add payload predicates inside an `all` with the header match, so records of other kinds are rejected before any decode. The walkthrough shows this on five records in three codecs.

## Codecs

The same expressions run in every codec. JSON and CBOR payloads are read by field path. Avro and Protobuf payloads carry their writer schema id in the `agdx.sid` header, and the filter names the schema ids it accepts. Readers never change between codecs.

## Where filters live

Setup creates each consumer group with its own filter in the plane catalog. Readers join by group id and do not supply a filter. The `inline` profile keeps its existing command name and supplies definitions during setup. It requires the plane and skips the revision walkthrough. `just inline` runs that profile.
