use crate::doctor::Doctor;
use crate::mixed::MixedVerdicts;
use crate::reporter::Snapshot;
use frostline_consumers::ConsumerSummary;
use frostline_producer::ProducerSummary;
use frostline_shared::Settings;
use frostline_shared::knobs;
use frostline_shared::measure::{ByteSize, Reduction, Summary};
use frostline_shared::names::RunId;
use frostline_shared::output::{board, fact, phase, strong};
use serde::Serialize;
use std::path::Path;

mod latency;

const SCOPE: &str = "Payload only. These figures exclude checkpoints, reports, framing, and TLS. Replay benchmarks measure TCP socket bytes separately.";

/// Everything a finished run measured, saved as report.json and rendered as report.md.
#[derive(Debug, Serialize)]
pub struct FinalReport {
    pub run_id: RunId,
    pub reproduce: Reproduce,
    pub settings: Settings,
    pub settings_digest: String,
    pub codec: String,
    pub catalog: String,
    pub producer: ProducerLine,
    pub summary: Summary,
    pub readers: Vec<ReaderLine>,
    pub previews: Vec<String>,
    pub revision_walkthrough: Vec<String>,
    /// What each filter did with the mixed log, when the story ran it.
    pub mixed_log: Vec<MixedVerdicts>,
    pub mixed_log_lines: Vec<String>,
    pub expired_windows: u64,
    pub complete: bool,
}

/// The command and the settings that give the same records, matches, and byte counts again.
#[derive(Debug, Serialize)]
pub struct Reproduce {
    pub command: String,
    pub variables: Vec<(&'static str, String)>,
}

#[derive(Debug, Serialize)]
pub struct ProducerLine {
    pub records: u64,
    pub payload_bytes: u64,
    pub windows: u64,
    pub checkpoints: u64,
    pub requested_rate: u32,
    pub achieved_rate: f64,
    pub elapsed_seconds: f64,
}

#[derive(Debug, Serialize)]
pub struct ReaderLine {
    pub group: String,
    pub baseline: bool,
    pub worker: u8,
    pub summary: ConsumerSummary,
}

impl Reproduce {
    /// The dataset variables of `settings`, then every other setting the process was started with.
    pub fn of(settings: &Settings, command: &str) -> Self {
        let mut variables = settings.dataset_variables();
        for (key, value) in knobs::configured() {
            if key != "FROSTLINE_OUTPUT_DIRECTORY"
                && !variables.iter().any(|(known, _)| *known == key)
            {
                variables.push((key, value));
            }
        }
        Self {
            command: format!("cargo run --release -p frostline-demo -- {command}"),
            variables,
        }
    }
}

impl From<ProducerSummary> for ProducerLine {
    fn from(summary: ProducerSummary) -> Self {
        Self {
            records: summary.records,
            payload_bytes: summary.payload_bytes,
            windows: summary.windows,
            checkpoints: summary.checkpoints,
            requested_rate: summary.requested_rate,
            achieved_rate: summary.achieved_rate,
            elapsed_seconds: summary.elapsed.as_secs_f64(),
        }
    }
}

pub fn run_profile(settings: &Settings, doctor: &Doctor) {
    phase("run profile");
    fact("target", &doctor.target);
    for (label, value) in settings.summary() {
        fact(label, value);
    }
}

/// The live board: completed windows only, so a slow reader never shows up as savings.
pub fn board_lines(snapshot: &Snapshot) -> Vec<String> {
    let summary = &snapshot.summary;
    let mut lines = vec![format!(
        "window {} complete, source {} records, {}",
        summary
            .latest
            .map_or_else(|| "none".to_owned(), |window| window.to_string()),
        summary.source_records,
        ByteSize::from(summary.source_bytes)
    )];
    lines.push(format!(
        "{:<22}{:>10}{:>13}{:>13}{:>13}{:>9}",
        "group", "matched", "received", "full feed", "saved", "saved"
    ));
    for subscription in &summary.subscriptions {
        let name = if subscription.baseline {
            format!("{} (full feed)", subscription.group)
        } else {
            subscription.group.clone()
        };
        lines.push(format!(
            "{name:<22}{:>10}{:>13}{:>13}{:>13}{}",
            subscription.matches,
            ByteSize::from(subscription.received_bytes).to_string(),
            ByteSize::from(summary.source_bytes).to_string(),
            ByteSize::from(subscription.saved_bytes).to_string(),
            strong(format!("{:>9}", subscription.reduction.to_string()))
        ));
    }
    lines.push(format!(
        "{:<22}{:>10}{:>13}{:>13}{:>13}{}",
        "filtered groups",
        "",
        ByteSize::from(summary.filtered_received_bytes).to_string(),
        ByteSize::from(summary.filtered_baseline_bytes).to_string(),
        ByteSize::from(summary.filtered_saved_bytes).to_string(),
        strong(format!("{:>9}", summary.filtered_reduction.to_string()))
    ));
    let pending = snapshot.pending.as_ref().map_or_else(
        || "nothing pending".to_owned(),
        |(window, completion)| format!("window {window} is {completion:?}"),
    );
    lines.push(format!(
        "producer window {}, {pending}, {} windows expired",
        snapshot
            .producer_window
            .map_or_else(|| "none".to_owned(), |window| window.to_string()),
        snapshot.expired
    ));
    lines.push(SCOPE.to_owned());
    lines
}

pub fn print_final(report: &FinalReport) {
    phase("report");
    fact(
        "producer",
        format!(
            "{} records, {} windows, {:.0} per second achieved of {} requested",
            report.producer.records,
            report.producer.windows,
            report.producer.achieved_rate,
            report.producer.requested_rate
        ),
    );
    for reader in &report.readers {
        fact(&reader.group, &reader.summary.status);
    }
    board(&board_lines(&Snapshot {
        summary: report.summary.clone(),
        producer_window: report.summary.latest,
        pending: None,
        expired: report.expired_windows,
    }));
}

pub fn write(directory: &Path, report: &FinalReport) -> Result<(), std::io::Error> {
    std::fs::create_dir_all(directory)?;
    let json = serde_json::to_vec_pretty(report).map_err(std::io::Error::other)?;
    std::fs::write(directory.join("report.json"), json)?;
    std::fs::write(directory.join("report.md"), markdown(report))
}

fn markdown(report: &FinalReport) -> String {
    let summary = &report.summary;
    let mut text = format!(
        "# Frostline run {}\n\nCodec {}, catalog {}, settings digest `{}`.\n\n{} complete windows, {} source records, {} of source payload ({} bytes).\n\n",
        report.run_id,
        report.codec,
        report.catalog,
        report.settings_digest,
        summary.windows,
        summary.source_records,
        ByteSize::from(summary.source_bytes),
        summary.source_bytes
    );
    text.push_str("| group | full feed reader | matched | received | received bytes | saved | payload avoided |\n| --- | --- | --- | --- | --- | --- | --- |\n");
    for subscription in &summary.subscriptions {
        let avoided = match subscription.reduction {
            Reduction::Value(_) => format!("**{}**", subscription.reduction),
            Reduction::NotApplicable => subscription.reduction.to_string(),
        };
        text.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {avoided} |\n",
            subscription.group,
            if subscription.baseline { "yes" } else { "no" },
            subscription.matches,
            ByteSize::from(subscription.received_bytes),
            subscription.received_bytes,
            ByteSize::from(subscription.saved_bytes)
        ));
    }
    text.push_str(&format!(
        "\nThe filtered groups received {} where reading the full feed would move {}. The server kept **{}** of payload back, **{} less**. {SCOPE}\n",
        ByteSize::from(summary.filtered_received_bytes),
        ByteSize::from(summary.filtered_baseline_bytes),
        ByteSize::from(summary.filtered_saved_bytes),
        summary.filtered_reduction
    ));
    latency::append(&mut text, &report.readers);
    for line in report
        .previews
        .iter()
        .chain(&report.revision_walkthrough)
        .chain(&report.mixed_log_lines)
    {
        text.push_str(&format!("\n- {line}"));
    }
    text.push_str("\n\n## Reproduce\n\nThe same settings give the same records, matches, and byte counts. Digests differ because every record carries its run id.\n\n```sh\n");
    for (key, value) in &report.reproduce.variables {
        text.push_str(&format!("{key}={value} \\\n"));
    }
    text.push_str(&format!("{}\n```\n", report.reproduce.command));
    text
}
