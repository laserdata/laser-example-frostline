# Walkthrough: one finite run

Run `just up` or `just up-local`, then `just demo-once`. Each section below matches one phase of the output and names the code behind it.

## run profile

`doctor::run` checks what the server offers and stops with one sentence per missing capability. Then `report::run_profile` prints the target and every setting the run uses. The local guard is on in finite runs, so the SDK checks every delivered record against the filter too.

## setup

`provision::provision` creates the run stream and its two topics and reads the retention back from the server. Then it tries each team's filter on scenario events before any record exists:

```text
Filter food-safety reads payload and headers. It selects a truck turning unsafe, rejects a battery update of a truck already unsafe, selects a frozen truck leaving the fleet.
Filter maintenance reads headers only. It selects a reefer fault, rejects the same fault with severity written as the text "2".
```

The second line shows that header values keep their type. A uint8 `2` matches. The text `"2"` does not.

Each group owns a separate filter saved during setup, including the food safety A/B group. The run file `runs/<run-id>/run.json` records every id.

## readers and publish

`Session::start` starts the reporter and one reader per group. With `just compare` it also starts a full-feed reader for each team. `frostline_producer::run` publishes the fleet. While it runs, each reader prints a few sampled business lines per second:

```text
Truck FR-042 went from safe to unsafe at 12.0 C. Food safety received partition 2 offset 814.
Fault E17 on the reefer of FR-042, severity error. Maintenance received partition 1 offset 233 and the server never decoded it.
```

The offset is the record's original position in the partition. The server skipped every record in between, and the reader never downloaded them.

## drain

The readers stop when every partition delivered its last checkpoint. The reporter waits until every window is complete.

## catalog

`walkthrough::previews` previews the food safety filter on each partition. A preview examines stored records, stores nothing, and joins no group:

```text
Preview of partition 0: examined 1000, matched 5, stopped at budget.
```

In managed mode, `walkthrough::pause_and_resume` creates a short-lived replay group with its own food safety policy and reads one record. It pauses the revision, acknowledges that record, and tries the next read. Then it resumes and reads once more:

```text
Paused revision 1. The record at partition 0 offset 28 already reached the reader and was still acknowledged.
The next read was refused with revision_disabled.
Resumed revision 1. Reading continued at offset 96.
```

A pause stops new reads. It never takes back a record a reader already holds.

## one log, many kinds of events

`mixed::mixed_log` publishes five records into a `mixed` topic of the run: a north pharma reading in JSON, a depot event in CBOR, an invoice in Protobuf, a reading whose `region` is the number 7, and a depot event in JSON. Each carries an `event.type` header and its `agdx.ct` content type. Then it previews four filters over that one partition:

```text
Filter north-pharma over the mixed log selected [0] and skipped [1, 2, 3, 4].
Filter north-pharma with mismatch pass over the mixed log selected [0] and skipped [1, 2, 4], and handed over unevaluated [3 (type_mismatch)].
Filter depot v1 by glob over the mixed log selected [1, 4] and skipped [0, 2, 3].
Filter every v1 event ignoring case over the mixed log selected [0, 1, 3, 4] and skipped [2].
```

The JSON team filter never decodes the CBOR and Protobuf records, it skips them, and the partition never stalls. The reading with a numeric region is skipped by default, and handed over marked unevaluated when the filter asks for edge cases. Header text filters pick a subdomain across all three codecs.

## report

`report::print_final` prints the producer's achieved rate, what each team handled, and the board. Every figure is payload bytes over completed windows. `report.json` and `report.md` land in the run directory.

## cleanup

`cleanup::cleanup` releases each binding, deletes the run's filters, drops its writer schemas, and deletes the run stream. A deleted filter keeps its id and name reserved in the catalog, and a dropped schema id stays reserved too. Running cleanup again on the same run file is safe. Set `FROSTLINE_KEEP_RUN=1` to keep everything for inspection.
