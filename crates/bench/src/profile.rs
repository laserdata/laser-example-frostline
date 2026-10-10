use crate::collect::process;
use crate::error::BenchError;
use crate::runtime::{RuntimeProcess, runtime_process};
use serde::Serialize;
use std::fs;
use std::io::{ErrorKind, Write};
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;
use tokio::process::Command;
use tokio::time::{Instant, interval};

const PERF_FREQUENCY: &str = "99";
const PERF_STAT_EVENTS: &str = "cycles,instructions,cache-misses,context-switches";
const PERF_INSTALL: &str = "Install the perf build for your running kernel, see the tools section of docs/benchmarks.md. Frostline never installs profilers or changes kernel settings.";
const HEAPTRACK_INSTALL: &str = "Install heaptrack, see the tools section of docs/benchmarks.md. The /proc memory samples in memory.jsonl are still written.";
const MEMORY_SAMPLE_INTERVAL: Duration = Duration::from_secs(1);

/// One per-second memory reading of a profiled process.
#[derive(Debug, Serialize)]
struct MemorySample {
    elapsed_seconds: f64,
    rss_bytes: u64,
    pss_bytes: Option<u64>,
    peak_rss_bytes: u64,
}

/// Sample `target` with perf for `seconds`, count hardware events, and render a flamegraph when the tool exists.
pub async fn cpu(target: &str, seconds: u64, directory: &Path) -> Result<(), BenchError> {
    let process = resolve(target)?;
    require("perf", PERF_INSTALL).await?;
    require(
        "flamegraph",
        "Install flamegraph to render the captured CPU profile.",
    )
    .await?;
    fs::create_dir_all(directory)?;
    let pid = process.pid.to_string();
    let data = directory.join("perf.data");
    let duration = seconds.to_string();
    let record = [
        "record",
        "-F",
        PERF_FREQUENCY,
        "--call-graph",
        "dwarf,16384",
        "-p",
        &pid,
        "-o",
        &data.to_string_lossy(),
        "--",
        "sleep",
        &duration,
    ]
    .map(str::to_owned);
    let stat_file = directory.join("perf-stat.txt");
    let stat = [
        "stat",
        "-e",
        PERF_STAT_EVENTS,
        "-p",
        &pid,
        "-o",
        &stat_file.to_string_lossy(),
        "--",
        "sleep",
        &duration,
    ]
    .map(str::to_owned);
    tokio::try_join!(
        run_logged("perf", &record, directory),
        run_logged("perf", &stat, directory)
    )?;
    let flamegraph = [
        "--perfdata".to_owned(),
        data.to_string_lossy().into_owned(),
        "-o".to_owned(),
        directory
            .join("flamegraph.svg")
            .to_string_lossy()
            .into_owned(),
    ];
    run_logged("flamegraph", &flamegraph, directory).await?;
    fs::write(
        directory.join("target.json"),
        serde_json::to_vec_pretty(&process)?,
    )?;
    Ok(())
}

/// Sample RSS, PSS, and peak RSS of `target` every second for `seconds`, and attach heaptrack when it exists.
pub async fn memory(target: &str, seconds: u64, directory: &Path) -> Result<(), BenchError> {
    let process = resolve(target)?;
    fs::create_dir_all(directory)?;
    fs::write(
        directory.join("target.json"),
        serde_json::to_vec_pretty(&process)?,
    )?;
    require("heaptrack", HEAPTRACK_INSTALL).await?;
    require(
        "gdb",
        "Attaching heaptrack requires GDB. The replay-trial --heaptrack preload mode does not.",
    )
    .await?;
    let mut heaptrack = Command::new("heaptrack")
        .args(["--record-only", "-p", &process.pid.to_string(), "-o"])
        .arg(directory.join("heaptrack"))
        .kill_on_drop(true)
        .stdout(Stdio::from(fs::File::create(
            directory.join("heaptrack.stdout"),
        )?))
        .stderr(Stdio::from(fs::File::create(
            directory.join("heaptrack.stderr"),
        )?))
        .spawn()?;
    let mut file = fs::File::create(directory.join("memory.jsonl"))?;
    let started = Instant::now();
    let mut ticks = interval(MEMORY_SAMPLE_INTERVAL);
    while started.elapsed() < Duration::from_secs(seconds) {
        ticks.tick().await;
        let sample = process::read(process.pid)
            .ok_or_else(|| BenchError::Invalid(format!("{target} exited while profiled")))?;
        let line = MemorySample {
            elapsed_seconds: started.elapsed().as_secs_f64(),
            rss_bytes: sample.rss_bytes,
            pss_bytes: process::pss_bytes(process.pid),
            peak_rss_bytes: sample.peak_rss_bytes,
        };
        writeln!(file, "{}", serde_json::to_string(&line)?)?;
    }
    {
        let pid = heaptrack
            .id()
            .ok_or_else(|| BenchError::Invalid("heaptrack already exited".to_owned()))?;
        let signal = Command::new("kill")
            .args(["-INT", &pid.to_string()])
            .status()
            .await?;
        if !signal.success() {
            return Err(BenchError::Invalid(
                "heaptrack could not be stopped cleanly".to_owned(),
            ));
        }
        let status = tokio::time::timeout(Duration::from_secs(30), heaptrack.wait())
            .await
            .map_err(|_| {
                BenchError::Invalid("heaptrack did not flush within 30 seconds".to_owned())
            })??;
        if !status.success() {
            return Err(BenchError::Invalid(format!(
                "heaptrack exited with {status}"
            )));
        }
    }
    Ok(())
}

fn resolve(target: &str) -> Result<RuntimeProcess, BenchError> {
    match target {
        "iggy" | "iggy-server" => runtime_process("iggy-server"),
        "plane" => runtime_process("plane"),
        _ => return Err(BenchError::Usage),
    }
    .ok_or(BenchError::NoLocalRuntime)
}

pub async fn require(tool: &'static str, install: &'static str) -> Result<(), BenchError> {
    match Command::new(tool).arg("--version").output().await {
        Err(error) if error.kind() == ErrorKind::NotFound => {
            Err(BenchError::MissingTool { tool, install })
        }
        Err(error) => Err(error.into()),
        Ok(output) if output.status.success() => Ok(()),
        Ok(output) => Err(BenchError::Invalid(format!(
            "{tool} --version exited with {}",
            output.status
        ))),
    }
}

// The exact command goes next to its output, so the artifact says how it was made.
async fn run_logged(tool: &str, arguments: &[String], directory: &Path) -> Result<(), BenchError> {
    let mut commands = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(directory.join("commands.txt"))?;
    writeln!(commands, "{tool} {}", arguments.join(" "))?;
    let status = Command::new(tool).args(arguments).status().await?;
    if !status.success() {
        return Err(BenchError::Invalid(format!("{tool} exited with {status}")));
    }
    Ok(())
}
