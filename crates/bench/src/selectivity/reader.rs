use super::{IDLE, MATCH_HEADER, MATCHED, ReadOutcome, SENTINEL, TOPIC};
use crate::error::BenchError;
use frostline_consumers::latency::Latency;
use frostline_consumers::poll::{PollReader, PollStart};
use frostline_shared::LaserFactory;
use frostline_shared::codec::Headers;
use laser_sdk::filters::FilteredStart;
use laser_sdk::iggy::prelude::HeaderKey;
use std::time::Instant;

/// The child side: read the dataset to its sentinel and print what arrived as one JSON line.
pub async fn read(
    factory: &LaserFactory,
    stream: &str,
    filtered: bool,
    case: super::cases::Case,
    identity: &str,
) -> Result<ReadOutcome, BenchError> {
    let laser = factory.connect(stream).await?;
    let mut outcome = ReadOutcome::default();
    let mut latency = Latency::default();
    let mut cycle = Latency::default();
    if filtered {
        let group = laser
            .stream(stream)
            .topic(TOPIC)
            .consumer_group(format!("bench-filtered-{identity}"));
        let configured = group.create().filter(case.filter()).build().await?;
        let binding = configured.filter.expect("requested benchmark policy");
        let mut reader = group
            .reader()?
            .start(FilteredStart::First)
            .count(1000)
            .max_examined(1000)
            .build()
            .await?;
        loop {
            let started = Instant::now();
            let (fetched, more) = reader.read_round().await?;
            latency.record(started.elapsed(), reader.examined_in_round());
            let Some(page) = fetched else {
                cycle.record(started.elapsed(), reader.examined_in_round());
                if more {
                    continue;
                }
                tokio::time::sleep(IDLE).await;
                continue;
            };
            let mut finished = false;
            for record in &page.records {
                let marker = marker(&record.message.user_headers_map()?.unwrap_or_default());
                if marker == Some(SENTINEL) {
                    finished = true;
                    break;
                }
                outcome.matched += u64::from(marker == Some(MATCHED));
                outcome.received_records += 1;
                outcome.received_payload_bytes += record.message.payload.len() as u64;
            }
            reader.ack_page(&page).await?;
            cycle.record(started.elapsed(), reader.examined_in_round());
            if finished {
                break;
            }
        }
        reader.close().await?;
        frostline_demo::cleanup::retire_binding(&laser, &binding).await?;
    } else {
        let mut reader = PollReader::new(
            &laser,
            stream,
            TOPIC,
            &format!("bench-full-feed-{identity}"),
            1,
            1000,
            PollStart::First,
        )
        .await?;
        loop {
            let started = Instant::now();
            let fetched = reader.poll().await?;
            latency.record(started.elapsed(), u64::from(fetched.count));
            let mut finished = false;
            let mut last_offset = None;
            for message in fetched.messages {
                last_offset = Some(message.header.offset);
                let value = marker(&message.user_headers_map()?.unwrap_or_default());
                if value == Some(SENTINEL) {
                    finished = true;
                    break;
                }
                let matched = super::oracle::matches(
                    case.predicate,
                    &message.payload,
                    value == Some(MATCHED),
                )?;
                outcome.matched += u64::from(matched);
                outcome.received_records += 1;
                outcome.received_payload_bytes += message.payload.len() as u64;
            }
            // The filtered child stores its progress with every page, so the ordinary child stores
            // its progress with every batch. Both trials then pay the same offset store per fetch.
            if let Some(offset) = last_offset {
                reader.commit(0, offset).await?;
            }
            cycle.record(started.elapsed(), u64::from(fetched.count));
            if finished {
                break;
            }
            if fetched.count == 0 {
                tokio::time::sleep(IDLE).await;
            }
        }
        reader.close().await?;
    }
    laser.close().await?;
    outcome.fetch_latency = latency.summary();
    outcome.fetch_and_ack_latency = cycle.summary();
    Ok(outcome)
}

fn marker(headers: &Headers) -> Option<u8> {
    let key = HeaderKey::try_from(MATCH_HEADER).ok()?;
    u8::try_from(headers.get(&key)?).ok()
}
