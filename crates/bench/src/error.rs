use frostline_demo::DemoError;
use frostline_producer::ProducerError;
use frostline_shared::ConfigError;
use laser_sdk::iggy::prelude::IggyError;
use laser_sdk::prelude::LaserError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum BenchError {
    #[error(transparent)]
    Consumer(#[from] frostline_consumers::ConsumerError),
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error(transparent)]
    Demo(#[from] DemoError),
    #[error(transparent)]
    Laser(#[from] LaserError),
    #[error(transparent)]
    Iggy(#[from] IggyError),
    #[error(transparent)]
    Producer(#[from] ProducerError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error("unknown profile '{0}', run `frostline-bench list`")]
    UnknownProfile(String),
    #[error("{tool} is not installed. {install}")]
    MissingTool {
        tool: &'static str,
        install: &'static str,
    },
    #[error("the local runtime is not running, start it with `just up-local`")]
    NoLocalRuntime,
    #[error("the trial is invalid: {0}")]
    Invalid(String),
    #[error(
        "usage: frostline-bench list | bench <profile> [--out DIR] | profile-cpu <iggy|plane> [--seconds N] | profile-memory <iggy|plane> [--seconds N]"
    )]
    Usage,
}

impl BenchError {
    /// 2 for a problem the user fixes before running, 1 for a failed or invalid trial.
    pub fn exit_code(&self) -> u8 {
        match self {
            BenchError::Usage
            | BenchError::UnknownProfile(_)
            | BenchError::MissingTool { .. }
            | BenchError::NoLocalRuntime => 2,
            _ => 1,
        }
    }
}
