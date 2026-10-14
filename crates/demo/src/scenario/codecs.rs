use super::finite;
use crate::error::DemoError;
use frostline_shared::codec::Codec;
use frostline_shared::config::Mode;
use frostline_shared::measure::ByteSize;
use frostline_shared::output::{board, phase};
use frostline_shared::{LaserFactory, Settings};
use strum::IntoEnumIterator;

const CODEC_RECORDS: u64 = 4000;
const CODEC_WINDOW: u64 = 1000;

/// The same seeded story once per codec. Matches must be equal, bytes differ with the format.
pub async fn codecs(settings: &Settings, factory: &LaserFactory) -> Result<(), DemoError> {
    let mut rows = vec![format!(
        "{:<10}{:>14}{:>14}{:>14}{:>10}",
        "codec", "source", "received", "saved", "avoided"
    )];
    let mut reference: Option<Vec<(String, u64)>> = None;
    for codec in Codec::iter() {
        phase(&format!("codec {codec}"));
        let run = Settings {
            mode: Mode::Finite,
            codec,
            total_records: settings.total_records.min(CODEC_RECORDS),
            checkpoint_records: CODEC_WINDOW,
            ..settings.clone()
        };
        let report = finite(&run, factory, false).await?;
        let matches: Vec<(String, u64)> = report
            .summary
            .subscriptions
            .iter()
            .map(|subscription| (subscription.group.clone(), subscription.matches))
            .collect();
        if let Some(expected) = &reference
            && *expected != matches
        {
            let (group, actual, expected) = matches
                .iter()
                .zip(expected)
                .find_map(|((group, actual), (expected_group, expected))| {
                    (group != expected_group || actual != expected)
                        .then_some((group, actual, expected))
                })
                .ok_or_else(|| {
                    DemoError::Incomplete(format!(
                        "{codec} and JSON reported different subscription counts"
                    ))
                })?;
            return Err(DemoError::Incomplete(format!(
                "{codec} selected {actual} records for {group}, JSON selected {expected}"
            )));
        }
        reference.get_or_insert(matches);
        rows.push(format!(
            "{:<10}{:>14}{:>14}{:>14}{:>10}",
            codec.to_string(),
            ByteSize::from(report.summary.source_bytes).to_string(),
            ByteSize::from(report.summary.filtered_received_bytes).to_string(),
            ByteSize::from(report.summary.filtered_saved_bytes).to_string(),
            report.summary.filtered_reduction.to_string()
        ));
    }
    phase("codecs");
    board(&rows);
    Ok(())
}
