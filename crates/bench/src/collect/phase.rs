use super::metrics;
use super::perf::PerfCapture;
use super::process::{self, ProcessSample};
use crate::error::BenchError;
use crate::runtime::RuntimeProcess;
use crate::trial::ServerUsage;
use serde::Serialize;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const SAMPLE_INTERVAL: Duration = Duration::from_millis(100);

/// Sample only the active phase, without retaining a growing sample list.
pub struct PhaseMonitor {
    stop: Sender<()>,
    metrics_after: PathBuf,
    perf: Option<PerfCapture>,
    worker: Option<JoinHandle<Result<Vec<ServerUsage>, BenchError>>>,
}

#[derive(Serialize)]
struct Observation<'a> {
    elapsed_seconds: f64,
    name: &'a str,
    pid: u32,
    sample: ProcessSample,
    pss_bytes: Option<u64>,
}

impl PhaseMonitor {
    pub fn start(runtime: &[RuntimeProcess], ticks: u64, path: &Path) -> Result<Self, BenchError> {
        metrics::snapshot(&path.with_extension("before.prom"))?;
        let tracked = runtime
            .iter()
            .map(|runtime| {
                let sample = read_same_process(runtime.pid, None)?;
                Ok((runtime.pid, runtime.binary.name.clone(), sample))
            })
            .collect::<Result<Vec<_>, BenchError>>()?;
        let server = runtime
            .iter()
            .find(|runtime| runtime.binary.name == "iggy-server")
            .ok_or(BenchError::NoLocalRuntime)?;
        let perf = PerfCapture::start(server.pid, path)?;
        let file = File::create(path)?;
        let (stop, stopped) = mpsc::channel();
        let worker = thread::spawn(move || sample(tracked, ticks, file, stopped));
        Ok(Self {
            metrics_after: path.with_extension("after.prom"),
            stop,
            worker: Some(worker),
            perf,
        })
    }

    pub async fn finish(mut self) -> Result<Vec<ServerUsage>, BenchError> {
        let _ = self.stop.send(());
        let worker = self
            .worker
            .take()
            .expect("the phase monitor owns its worker");
        let result = tokio::task::spawn_blocking(move || worker.join())
            .await
            .map_err(|error| BenchError::Invalid(format!("the phase monitor failed: {error}")))?
            .map_err(|_| BenchError::Invalid("the phase monitor panicked".to_owned()))?;
        if let Some(perf) = self.perf.take() {
            tokio::task::spawn_blocking(move || perf.finish())
                .await
                .map_err(|error| {
                    BenchError::Invalid(format!("the phase profiler failed: {error}"))
                })??;
        }
        metrics::snapshot(&self.metrics_after)?;
        result
    }
}

impl Drop for PhaseMonitor {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn sample(
    tracked: Vec<(u32, String, ProcessSample)>,
    ticks: u64,
    mut file: File,
    stopped: Receiver<()>,
) -> Result<Vec<ServerUsage>, BenchError> {
    let started = Instant::now();
    let mut peaks: Vec<u64> = tracked
        .iter()
        .map(|(_, _, before)| before.rss_bytes)
        .collect();
    let mut pss_peaks = vec![0; tracked.len()];
    let mut final_samples = Vec::new();
    let mut observations = 0;
    let mut finished = false;
    loop {
        final_samples.clear();
        for (index, (pid, name, before)) in tracked.iter().enumerate() {
            let after = read_same_process(*pid, Some(before.start_ticks))?;
            let pss_bytes = process::pss_bytes(*pid);
            peaks[index] = peaks[index].max(after.rss_bytes);
            pss_peaks[index] = pss_peaks[index].max(pss_bytes.unwrap_or(0));
            writeln!(
                file,
                "{}",
                serde_json::to_string(&Observation {
                    elapsed_seconds: started.elapsed().as_secs_f64(),
                    name,
                    pid: *pid,
                    sample: after,
                    pss_bytes,
                })?
            )?;
            final_samples.push(after);
        }
        observations += 1;
        file.flush()?;
        if finished {
            break;
        }
        finished = stopped.recv_timeout(SAMPLE_INTERVAL) != Err(mpsc::RecvTimeoutError::Timeout);
    }
    tracked
        .into_iter()
        .zip(final_samples)
        .enumerate()
        .map(|(index, ((pid, name, before), after))| {
            let cpu_ticks = after
                .cpu_ticks
                .checked_sub(before.cpu_ticks)
                .ok_or_else(|| {
                    BenchError::Invalid(format!("the CPU counter decreased for {name}"))
                })?;
            Ok(ServerUsage {
                name,
                cpu_seconds: cpu_ticks as f64 / ticks.max(1) as f64,
                rss_bytes: after.rss_bytes,
                peak_rss_bytes: peaks[index],
                lifetime_peak_rss_bytes: after.peak_rss_bytes,
                peak_pss_bytes: pss_peaks[index],
                memory_samples: observations,
                memory_sample_interval_ms: SAMPLE_INTERVAL.as_millis() as u64,
                cgroup: process::cgroup_memory(pid),
            })
        })
        .collect()
}

fn read_same_process(pid: u32, start_ticks: Option<u64>) -> Result<ProcessSample, BenchError> {
    let sample = process::read(pid)
        .ok_or_else(|| BenchError::Invalid(format!("runtime process {pid} disappeared")))?;
    if start_ticks.is_some_and(|expected| expected != sample.start_ticks) {
        return Err(BenchError::Invalid(format!("runtime PID {pid} was reused")));
    }
    Ok(sample)
}
