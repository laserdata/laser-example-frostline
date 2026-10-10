use super::ReadOutcome;
use crate::collect::phase::PhaseMonitor;
use crate::collect::syscalls::{self, STRACE_ARGUMENTS, WireUsage};
use crate::error::BenchError;
use crate::runtime::RuntimeProcess;
use crate::trial::ServerUsage;
use serde::Serialize;
use std::fs;
use std::path::Path;
use tokio::process::Command;
use tokio::time::{Duration, timeout};

const DEADLINE: Duration = Duration::from_secs(900);

#[derive(Debug, Serialize)]
pub struct MeasuredRead {
    pub outcome: ReadOutcome,
    pub client_cpu_seconds: f64,
    pub client_peak_rss_bytes: u64,
    pub server: Vec<ServerUsage>,
}

pub async fn measure(
    binary: &Path,
    stream: &str,
    filtered: bool,
    directory: &Path,
    runtime: &[RuntimeProcess],
    ticks: u64,
    case: &str,
) -> Result<MeasuredRead, BenchError> {
    let name = if filtered { "filtered" } else { "full-feed" };
    let base = directory.join(format!("{stream}-{name}"));
    let resources = base.with_extension("resources.txt");
    let monitor = PhaseMonitor::start(runtime, ticks, &base.with_extension("memory.jsonl"))?;
    let output = timeout(
        DEADLINE,
        Command::new("/usr/bin/time")
            .args(["-f", "%U %S %M", "-o"])
            .arg(&resources)
            .arg(binary)
            .args(["read-selectivity", stream, name, case, "0"])
            .kill_on_drop(true)
            .output(),
    )
    .await
    .map_err(|_| {
        BenchError::Invalid("the selectivity reader exceeded its deadline".to_owned())
    })??;
    let server = monitor.finish().await?;
    let outcome = outcome(&output, name)?;
    let usage = fs::read_to_string(&resources)?;
    let values = usage
        .split_whitespace()
        .map(str::parse::<f64>)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| {
            BenchError::Invalid("GNU time returned invalid resource counters".to_owned())
        })?;
    if values.len() != 3
        || values
            .iter()
            .any(|value| !value.is_finite() || *value < 0.0)
    {
        return Err(BenchError::Invalid(
            "GNU time did not return CPU and RSS counters".to_owned(),
        ));
    }
    Ok(MeasuredRead {
        outcome,
        client_cpu_seconds: values[0] + values[1],
        client_peak_rss_bytes: (values[2] * 1024.0) as u64,
        server,
    })
}

pub async fn trace(
    binary: &Path,
    stream: &str,
    filtered: bool,
    directory: &Path,
    expected: &ReadOutcome,
    case: &str,
) -> Result<WireUsage, BenchError> {
    let name = if filtered { "filtered" } else { "full-feed" };
    let path = directory.join(format!("{stream}-{name}.strace"));
    let output = timeout(
        DEADLINE,
        Command::new("strace")
            .args(STRACE_ARGUMENTS)
            .arg(&path)
            .arg("--")
            .arg(binary)
            .args(["read-selectivity", stream, name, case, "0"])
            .kill_on_drop(true)
            .output(),
    )
    .await
    .map_err(|_| {
        BenchError::Invalid("the traced selectivity reader exceeded its deadline".to_owned())
    })??;
    let traced = outcome(&output, name)?;
    if (
        traced.matched,
        traced.received_records,
        traced.received_payload_bytes,
    ) != (
        expected.matched,
        expected.received_records,
        expected.received_payload_bytes,
    ) {
        return Err(BenchError::Invalid(
            "the traced pass received different records".to_owned(),
        ));
    }
    Ok(syscalls::read(&path)?)
}

fn outcome(output: &std::process::Output, name: &str) -> Result<ReadOutcome, BenchError> {
    let stdout = String::from_utf8_lossy(&output.stdout);
    if !output.status.success() {
        return Err(BenchError::Invalid(format!(
            "the {name} reader exited with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    let line = stdout
        .lines()
        .rev()
        .find(|line| line.starts_with('{'))
        .ok_or_else(|| BenchError::Invalid(format!("the {name} reader printed no outcome")))?;
    Ok(serde_json::from_str(line)?)
}
