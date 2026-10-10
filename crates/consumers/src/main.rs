#![forbid(unsafe_code)]

use frostline_consumers::hold::Hold;
use frostline_consumers::{ConsumerError, HOLD_MARKER, ReaderSpec, SUMMARY_MARKER};
use frostline_shared::config::Mode;
use frostline_shared::output::{self, fact};
use frostline_shared::runfile::{RunFile, RunFileError};
use frostline_shared::{
    ConfigError, LaserFactory, ServiceHandle, Settings, init_tracing, shutdown_signal,
};
use std::path::PathBuf;
use std::process::ExitCode;
use thiserror::Error;
use tokio::io::AsyncReadExt;
use tokio::sync::oneshot;
use tracing::error;

#[derive(Debug, Error)]
enum Error {
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error(transparent)]
    RunFile(#[from] RunFileError),
    #[error(transparent)]
    Consumer(#[from] ConsumerError),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(
        "usage: frostline-consumers --manifest <run.json> --group <group> [--baseline] [--worker N] [--attempt N] [--live] [--hold] [--replay]"
    )]
    Usage,
}

#[tokio::main]
async fn main() -> ExitCode {
    if std::env::args().len() == 2
        && std::env::args()
            .nth(1)
            .is_some_and(|arg| arg == "--version" || arg == "-V")
    {
        println!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
    }
    println!("{}", output::version_banner(env!("CARGO_PKG_VERSION")));
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(failure @ (Error::Usage | Error::Consumer(ConsumerError::UnknownGroup(_)))) => {
            error!("{failure}");
            ExitCode::from(2)
        }
        Err(failure) => {
            error!("the consumer failed. {failure}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), Error> {
    let mut arguments = std::env::args().skip(1);
    let (mut manifest, mut group, mut mode) = (None, None, Mode::Finite);
    let (mut baseline, mut worker, mut attempt, mut held) = (false, 1u8, 1u32, false);
    let mut replay = false;
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--manifest" => manifest = Some(PathBuf::from(arguments.next().ok_or(Error::Usage)?)),
            "--group" => group = Some(arguments.next().ok_or(Error::Usage)?),
            "--baseline" => baseline = true,
            "--replay" => replay = true,
            "--worker" => {
                worker = arguments
                    .next()
                    .and_then(|value| value.parse().ok())
                    .ok_or(Error::Usage)?
            }
            "--attempt" => {
                attempt = arguments
                    .next()
                    .and_then(|value| value.parse().ok())
                    .ok_or(Error::Usage)?
            }
            "--live" => mode = Mode::Live,
            "--hold" => held = true,
            _ => return Err(Error::Usage),
        }
    }
    let (manifest, group) = (manifest.ok_or(Error::Usage)?, group.ok_or(Error::Usage)?);
    let settings = Settings::from_env(mode)?;
    init_tracing(settings.log_format);
    let run_file = RunFile::read(&manifest)?;
    let factory = LaserFactory::from_env()?;
    let spec = ReaderSpec {
        group,
        baseline,
        worker,
        attempt,
        replay,
    };
    let service = ServiceHandle::new("consumer");
    let hold = held.then(hold_until_stdin_closes);
    let reading =
        frostline_consumers::run(&settings, &factory, &run_file, spec, service.watch(), hold);
    tokio::pin!(reading);
    let summary = tokio::select! {
        summary = &mut reading => summary?,
        () = shutdown_signal() => {
            service.cancel();
            reading.await?
        }
    };
    fact("receipts", summary.receipts);
    fact("matches", summary.matches);
    fact(
        "received",
        format!(
            "{} bytes in {} records",
            summary.received_bytes, summary.received_records
        ),
    );
    let latency = summary.fetch_latency;
    fact(
        "latency",
        format!(
            "{} fetches, p50 {} us, p95 {} us, p99 {} us, max {} us",
            latency.fetches, latency.p50_us, latency.p95_us, latency.p99_us, latency.max_us
        ),
    );
    fact("status", &summary.status);
    println!("{SUMMARY_MARKER} {}", serde_json::to_string(&summary)?);
    Ok(())
}

// The bench reads this process's socket counters after the marker, then closes stdin to release it.
fn hold_until_stdin_closes() -> Hold {
    let (reached, reached_signal) = oneshot::channel();
    let (released, release) = oneshot::channel();
    tokio::spawn(async move {
        if reached_signal.await.is_ok() {
            println!("{HOLD_MARKER}");
            let mut ignored = Vec::new();
            let _ = tokio::io::stdin().read_to_end(&mut ignored).await;
            let _ = released.send(());
        }
    });
    Hold { reached, release }
}
