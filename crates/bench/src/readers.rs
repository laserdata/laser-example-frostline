use crate::collect::syscalls::STRACE_ARGUMENTS;
use crate::error::BenchError;
use frostline_consumers::{ConsumerSummary, HOLD_MARKER, SUMMARY_MARKER};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::fs::File;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tokio::time::Instant;

const STRACE: &str = "strace";

/// One `frostline-consumers` process that reads its group and then holds its connections open.
pub struct ReaderProcess {
    pub group: String,
    pub baseline: bool,
    pub pid: u32,
    pub log: PathBuf,
    child: Child,
    stdin: Option<ChildStdin>,
    held: Option<oneshot::Receiver<Instant>>,
    output: JoinHandle<std::io::Result<()>>,
}

/// Where a reader's settings come from: the run file, and the variables every reader of a trial shares.
pub struct ReaderLaunch<'a> {
    pub binary: &'a Path,
    pub run_file: &'a Path,
    pub variables: &'a [(&'static str, String)],
    pub log_directory: &'a Path,
}

impl ReaderProcess {
    /// With `trace`, the reader runs under strace, which logs every socket syscall to that file.
    pub async fn spawn(
        launch: &ReaderLaunch<'_>,
        group: &str,
        baseline: bool,
        trace: Option<&Path>,
    ) -> Result<Self, BenchError> {
        let mut command = match trace {
            Some(trace) => {
                let mut traced = Command::new(STRACE);
                traced
                    .args(STRACE_ARGUMENTS)
                    .arg(trace)
                    .arg("--")
                    .arg(launch.binary);
                traced
            }
            None => Command::new(launch.binary),
        };
        command
            .arg("--manifest")
            .arg(launch.run_file)
            .args(["--group", group, "--hold"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        if baseline {
            command.arg("--baseline");
        }
        for (key, _) in
            std::env::vars_os().filter(|(key, _)| key.to_string_lossy().starts_with("FROSTLINE_"))
        {
            command.env_remove(key);
        }
        command.envs(launch.variables.iter().cloned());
        command.arg("--replay");
        let mut child = command.spawn()?;
        let pid = child
            .id()
            .ok_or_else(|| BenchError::Invalid(format!("reader {group} exited at start")))?;
        let name = if baseline {
            format!("{group}-full-feed")
        } else {
            group.to_owned()
        };
        let log: PathBuf = launch.log_directory.join(format!("{name}.log"));
        let (stdout, stderr) = (child.stdout.take(), child.stderr.take());
        let (reached, held) = oneshot::channel();
        let output = tokio::spawn(copy_output(stdout, stderr, log.clone(), reached));
        Ok(Self {
            group: group.to_owned(),
            baseline,
            pid,
            log,
            stdin: child.stdin.take(),
            child,
            held: Some(held),
            output,
        })
    }

    /// Wait until the reader finished reading and holds its sockets. Returns the moment it printed the marker.
    pub async fn wait_held(&mut self) -> Result<Instant, BenchError> {
        let held = self
            .held
            .take()
            .ok_or_else(|| BenchError::Invalid("a reader was awaited twice".to_owned()))?;
        match held.await {
            Ok(at) => Ok(at),
            Err(_) => {
                let _ = self.child.start_kill();
                let status = self.child.wait().await?;
                Err(BenchError::Invalid(format!(
                    "reader {} stopped before it finished reading, {status}",
                    self.group
                )))
            }
        }
    }

    /// Close stdin so the reader closes its connections and exits, and require a clean exit.
    pub async fn release(mut self) -> Result<(), BenchError> {
        if let Some(mut stdin) = self.stdin.take() {
            let _ = stdin.shutdown().await;
        }
        let status = self.child.wait().await?;
        self.output.await.map_err(|error| {
            BenchError::Invalid(format!("reader output task failed: {error}"))
        })??;
        if !status.success() {
            return Err(BenchError::Invalid(format!(
                "reader {} exited with {status}",
                self.group
            )));
        }
        Ok(())
    }
}

/// The summary a released reader printed last, read back from its log.
pub fn summary_from(log: &Path) -> Result<ConsumerSummary, BenchError> {
    let text = std::fs::read_to_string(log)?;
    let line = text
        .lines()
        .rev()
        .find_map(|line| line.strip_prefix(SUMMARY_MARKER))
        .ok_or_else(|| BenchError::Invalid(format!("{} holds no reader summary", log.display())))?;
    Ok(serde_json::from_str(line.trim())?)
}

// Both streams go to one log file, and the marker line tells the bench the reader is done.
async fn copy_output(
    stdout: Option<ChildStdout>,
    stderr: Option<ChildStderr>,
    log: PathBuf,
    reached: oneshot::Sender<Instant>,
) -> std::io::Result<()> {
    let mut file = File::create(&log).await?;
    let mut reached = Some(reached);
    let errors = stderr.map(|stderr| {
        let log = log.with_extension("stderr.log");
        tokio::spawn(async move {
            let mut file = File::create(log).await?;
            let mut lines = BufReader::new(stderr).lines();
            while let Some(line) = lines.next_line().await? {
                file.write_all(format!("{line}\n").as_bytes()).await?;
            }
            file.flush().await
        })
    });
    let stdout = stdout.ok_or_else(|| std::io::Error::other("reader stdout is missing"))?;
    let mut lines = BufReader::new(stdout).lines();
    while let Some(line) = lines.next_line().await? {
        file.write_all(format!("{line}\n").as_bytes()).await?;
        if line.contains(HOLD_MARKER)
            && let Some(sender) = reached.take()
        {
            let _ = sender.send(Instant::now());
        }
    }
    file.flush().await?;
    if let Some(errors) = errors {
        errors.await.map_err(std::io::Error::other)??;
    }
    Ok(())
}
