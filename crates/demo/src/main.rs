#![forbid(unsafe_code)]

use frostline_demo::command::print_help;
use frostline_demo::scenario::{codecs, finite, live};
use frostline_demo::{Command, DemoError, doctor, standalone};
use frostline_shared::config::Mode;
use frostline_shared::{LaserFactory, Settings, init_tracing, output};
use std::process::ExitCode;
use tracing::error;

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
        Err(failure) => {
            error!("{failure}");
            ExitCode::from(failure.exit_code())
        }
    }
}

async fn run() -> Result<(), DemoError> {
    let command = Command::from_args()?;
    let mode = match &command {
        Command::Run(mode) => *mode,
        _ => Mode::Finite,
    };
    let settings = Settings::from_env(mode)?;
    init_tracing(settings.log_format);
    if command == Command::Help {
        print_help();
        return Ok(());
    }
    let factory = LaserFactory::from_env()?;
    match command {
        Command::Run(Mode::Finite) => finite(&settings, &factory, false).await.map(|_| ()),
        Command::Run(Mode::Compare) => finite(&settings, &factory, true).await.map(|_| ()),
        Command::Run(Mode::Live) => live(&settings, &factory).await.map(|_| ()),
        Command::Run(Mode::Codecs) => codecs(&settings, &factory).await,
        Command::Doctor => {
            let doctor = doctor::run(&factory).await?;
            doctor.print();
            doctor.require(&settings)
        }
        Command::Setup => standalone::setup(&settings, &factory).await,
        Command::Report { manifest } => standalone::report(&factory, &manifest).await,
        Command::Cleanup { manifest } => standalone::cleanup_run(&factory, &manifest).await,
        Command::Help => Ok(()),
    }
}
