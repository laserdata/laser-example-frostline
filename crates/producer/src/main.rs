#![forbid(unsafe_code)]

use frostline_producer::ProducerError;
use frostline_shared::config::Mode;
use frostline_shared::output::{self, fact};
use frostline_shared::runfile::{RunFile, RunFileError};
use frostline_shared::{
    ConfigError, LaserFactory, ServiceHandle, Settings, init_tracing, shutdown_signal,
};
use std::path::PathBuf;
use std::process::ExitCode;
use thiserror::Error;
use tracing::error;

#[derive(Debug, Error)]
enum Error {
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error(transparent)]
    RunFile(#[from] RunFileError),
    #[error(transparent)]
    Producer(#[from] ProducerError),
    #[error("usage: frostline-producer --manifest <run.json> [--live]")]
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
        Err(Error::Usage) => {
            error!("{}", Error::Usage);
            ExitCode::from(2)
        }
        Err(failure) => {
            error!("the producer failed. {failure}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), Error> {
    let mut arguments = std::env::args().skip(1);
    let mut manifest = None;
    let mut mode = Mode::Finite;
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--manifest" => manifest = Some(PathBuf::from(arguments.next().ok_or(Error::Usage)?)),
            "--live" => mode = Mode::Live,
            _ => return Err(Error::Usage),
        }
    }
    let manifest = manifest.ok_or(Error::Usage)?;
    let settings = Settings::from_env(mode)?;
    init_tracing(settings.log_format);
    let run_file = RunFile::read(&manifest)?;
    run_file.ensure_compatible(&settings)?;
    RunFile::lock(&manifest)?;
    let factory = LaserFactory::from_env()?;
    let service = ServiceHandle::new("producer");
    let watch = service.watch();
    let producing = frostline_producer::run(&settings, &factory, &run_file, watch, None);
    tokio::pin!(producing);
    let summary = tokio::select! {
        summary = &mut producing => summary?,
        () = shutdown_signal() => {
            service.cancel();
            producing.await?
        }
    };
    fact("records", summary.records);
    fact("payload", format!("{} bytes", summary.payload_bytes));
    fact("windows", summary.windows);
    fact(
        "rate",
        format!(
            "{:.0} per second achieved, {} requested",
            summary.achieved_rate, summary.requested_rate
        ),
    );
    Ok(())
}
