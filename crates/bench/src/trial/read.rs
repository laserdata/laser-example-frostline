use super::{ReadPhase, ReaderUsage, ReaderWire, TrialContext};
use crate::collect::SocketLedger;
use crate::collect::phase::PhaseMonitor;
use crate::collect::process;
use crate::collect::sockets;
use crate::collect::syscalls;
use crate::error::BenchError;
use crate::readers::{ReaderLaunch, ReaderProcess, summary_from};
use frostline_consumers::ConsumerSummary;
use frostline_consumers::latency::LatencySummary;
use frostline_shared::Settings;
use frostline_shared::knobs;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::watch;
use tokio::time::{Instant, interval};

const SOCKET_SAMPLE_INTERVAL: Duration = Duration::from_millis(500);

// Every reader holds its sockets after its last checkpoint, so the final snapshot sees every counter.
pub async fn read(
    context: &TrialContext<'_>,
    settings: &Settings,
    launch: &ReaderLaunch<'_>,
    groups: &[&str],
    baseline: bool,
) -> Result<ReadPhase, BenchError> {
    let log_directory = launch.log_directory;
    fs::create_dir_all(log_directory)?;
    let monitor = PhaseMonitor::start(
        &context.runtime,
        context.ticks_per_second,
        &log_directory.join(if baseline {
            "full-feed-memory.jsonl"
        } else {
            "filtered-memory.jsonl"
        }),
    )?;
    let started = Instant::now();
    let mut readers = Vec::new();
    for group in groups {
        readers.push(ReaderProcess::spawn(launch, group, baseline, None).await?);
    }
    let pids: Vec<u32> = readers.iter().map(|reader| reader.pid).collect();
    let ledger = Arc::new(Mutex::new(SocketLedger::default()));
    let (stop, stopped) = watch::channel(false);
    let sampler = tokio::spawn(sample_sockets(ledger.clone(), pids.clone(), stopped));
    let mut held = Vec::new();
    for reader in &mut readers {
        held.push(
            tokio::time::timeout(settings.drain_timeout, reader.wait_held())
                .await
                .map_err(|_| {
                    BenchError::Invalid("a reader did not finish within its deadline".to_owned())
                })??,
        );
    }
    let _ = stop.send(true);
    sampler
        .await
        .map_err(|error| BenchError::Invalid(format!("the socket sampler failed: {error}")))??;
    let lines = sockets::snapshot().await?;
    let server = monitor.finish().await?;
    let mut usages = {
        let mut ledger = ledger
            .lock()
            .expect("the socket ledger lock is not poisoned");
        ledger.record(&lines, &pids);
        readers
            .iter()
            .zip(&held)
            .map(|(reader, at)| {
                let sample = process::read(reader.pid).ok_or_else(|| {
                    BenchError::Invalid(format!("reader {} vanished while held", reader.group))
                })?;
                Ok(ReaderUsage {
                    group: reader.group.clone(),
                    baseline: reader.baseline,
                    wall_seconds: (*at - started).as_secs_f64(),
                    usage: ledger.usage(reader.pid, sample, context.ticks_per_second),
                    fetch_latency: LatencySummary::default(),
                    delivery: ConsumerSummary::default(),
                })
            })
            .collect::<Result<Vec<_>, BenchError>>()?
    };
    for (usage, reader) in usages.iter_mut().zip(readers) {
        let log = reader.log.clone();
        reader.release().await?;
        let summary = summary_from(&log)?;
        if summary.duplicates != 0 {
            return Err(BenchError::Invalid(
                "the measured reader redelivered records".to_owned(),
            ));
        }
        usage.fetch_latency = summary.fetch_latency;
        usage.delivery = summary;
    }
    let wall_seconds = usages
        .iter()
        .map(|usage| usage.wall_seconds)
        .fold(0.0, f64::max);
    Ok(ReadPhase {
        wall_seconds,
        readers: usages,
        server,
    })
}

// Socket snapshots can miss short-lived connections. A traced pass counts successful socket transfers.
pub async fn trace(
    settings: &Settings,
    launch: &ReaderLaunch<'_>,
    groups: &[&str],
    baseline: bool,
    measured: &ReadPhase,
) -> Result<Vec<ReaderWire>, BenchError> {
    let log_directory = launch.log_directory;
    let traces: Vec<PathBuf> = groups
        .iter()
        .map(|group| {
            log_directory.join(format!(
                "{group}{}.strace",
                if baseline { "-full-feed" } else { "" }
            ))
        })
        .collect();
    let mut readers = Vec::new();
    for (group, trace) in groups.iter().zip(&traces) {
        readers.push(ReaderProcess::spawn(launch, group, baseline, Some(trace)).await?);
    }
    for reader in &mut readers {
        tokio::time::timeout(settings.drain_timeout, reader.wait_held())
            .await
            .map_err(|_| {
                BenchError::Invalid("a traced reader did not finish within its deadline".to_owned())
            })??;
    }
    let mut wires = Vec::new();
    for (reader, trace) in readers.into_iter().zip(&traces) {
        let (group, baseline, log) = (reader.group.clone(), reader.baseline, reader.log.clone());
        reader.release().await?;
        let delivery = summary_from(&log)?;
        let expected = measured
            .readers
            .iter()
            .find(|usage| usage.group == group)
            .ok_or_else(|| BenchError::Invalid(format!("no measured reader for {group}")))?;
        if counters(&delivery) != counters(&expected.delivery) {
            return Err(BenchError::Invalid(format!(
                "the traced {group} reader delivered different counts or bytes"
            )));
        }
        wires.push(ReaderWire {
            group,
            baseline,
            wire: syscalls::read(trace)?,
            delivery,
        });
    }
    Ok(wires)
}

async fn sample_sockets(
    ledger: Arc<Mutex<SocketLedger>>,
    pids: Vec<u32>,
    mut stopped: watch::Receiver<bool>,
) -> Result<(), BenchError> {
    let mut ticks = interval(SOCKET_SAMPLE_INTERVAL);
    loop {
        tokio::select! {
            _ = stopped.changed() => return Ok(()),
            _ = ticks.tick() => {
                let lines = sockets::snapshot().await?;
                ledger.lock().expect("the socket ledger lock is not poisoned").record(&lines, &pids);
            }
        }
    }
}

// Readers take partitions, codec, and catalog from the run file. These are the knobs that change how they read.
pub(super) fn reader_variables(settings: &Settings) -> Vec<(&'static str, String)> {
    vec![
        (knobs::POLL_RECORDS, settings.poll_records.to_string()),
        (knobs::REPLY_BYTES, settings.reply_bytes.to_string()),
        (
            knobs::IDLE_INTERVAL_MS,
            settings.idle_interval.as_millis().to_string(),
        ),
        (knobs::LOCAL_GUARD, settings.local_guard.to_string()),
        (
            knobs::SAMPLED_EVENTS,
            settings.sampled_events_per_role_per_second.to_string(),
        ),
        (knobs::FLEET_SIZE, settings.fleet_size.to_string()),
        (knobs::LOG_FORMAT, settings.log_format.to_string()),
    ]
}

fn counters(summary: &ConsumerSummary) -> [u64; 5] {
    [
        summary.receipts,
        summary.matches,
        summary.received_records,
        summary.received_bytes,
        summary.duplicates,
    ]
}
