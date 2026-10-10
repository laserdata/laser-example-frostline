use crate::error::BenchError;
use std::fs::File;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const FLUSH_DEADLINE: Duration = Duration::from_secs(30);

/// An optional capture of exactly one benchmark phase.
pub struct PerfCapture {
    child: Child,
    data: PathBuf,
}

impl PerfCapture {
    pub fn start(pid: u32, artifact: &Path) -> Result<Option<Self>, BenchError> {
        match std::env::var("FROSTLINE_PROFILE_CPU").as_deref() {
            Err(std::env::VarError::NotPresent) | Ok("0" | "false") => return Ok(None),
            Ok("1" | "true") => {}
            _ => {
                return Err(BenchError::Invalid(
                    "FROSTLINE_PROFILE_CPU must be 0, 1, false, or true".to_owned(),
                ));
            }
        }
        let data = artifact.with_extension("perf.data");
        let log = File::create(artifact.with_extension("perf.log"))?;
        let child = Command::new("perf")
            .args([
                "record",
                "-F",
                "99",
                "--call-graph",
                "dwarf,16384",
                "-p",
                &pid.to_string(),
                "-o",
            ])
            .arg(&data)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(log)
            .spawn()?;
        Ok(Some(Self { child, data }))
    }

    pub fn finish(mut self) -> Result<(), BenchError> {
        let status = Command::new("kill")
            .args(["-INT", &self.child.id().to_string()])
            .status()?;
        if !status.success() {
            return Err(BenchError::Invalid(
                "perf could not receive its flush signal".to_owned(),
            ));
        }
        let started = Instant::now();
        let status = loop {
            if let Some(status) = self.child.try_wait()? {
                break status;
            }
            if started.elapsed() >= FLUSH_DEADLINE {
                return Err(BenchError::Invalid(
                    "perf did not flush within 30 seconds".to_owned(),
                ));
            }
            thread::sleep(Duration::from_millis(50));
        };
        if !status.success() && status.signal() != Some(2) {
            return Err(BenchError::Invalid(format!("perf exited with {status}")));
        }
        let report = Command::new("perf")
            .args(["report", "--stdio", "--no-children", "-g", "none", "-i"])
            .arg(&self.data)
            .output()?;
        if !report.status.success() {
            return Err(BenchError::Invalid(
                "perf could not read the phase capture".to_owned(),
            ));
        }
        std::fs::write(self.data.with_extension("report.txt"), report.stdout)?;
        let render = Command::new("flamegraph")
            .arg("--perfdata")
            .arg(&self.data)
            .arg("-o")
            .arg(self.data.with_extension("svg"))
            .output()?;
        std::fs::write(self.data.with_extension("flamegraph.log"), &render.stderr)?;
        if !render.status.success() {
            return Err(BenchError::Invalid(
                "the phase flamegraph could not be rendered".to_owned(),
            ));
        }
        Ok(())
    }
}

impl Drop for PerfCapture {
    fn drop(&mut self) {
        if matches!(self.child.try_wait(), Ok(None)) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}
