use frostline_consumers::ConsumerError;
use frostline_producer::ProducerError;
use frostline_shared::ConfigError;
use frostline_shared::codec::CodecError;
use frostline_shared::measure::AggregateError;
use frostline_shared::runfile::RunFileError;
use frostline_shared::topology::TopologyError;
use laser_sdk::prelude::LaserError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DemoError {
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error(transparent)]
    Laser(#[from] LaserError),
    #[error(transparent)]
    Topology(#[from] TopologyError),
    #[error(transparent)]
    Codec(#[from] CodecError),
    #[error(transparent)]
    RunFile(#[from] RunFileError),
    #[error(transparent)]
    Aggregate(#[from] AggregateError),
    #[error(transparent)]
    Producer(#[from] ProducerError),
    #[error(transparent)]
    Consumer(#[from] ConsumerError),
    #[error("The server cannot run this demo. {0}")]
    Preflight(String),
    #[error("the measurement is incomplete: {0}")]
    Incomplete(String),
    #[error("the report could not be written: {0}")]
    Io(#[from] std::io::Error),
    #[error("{0}\nrun `frostline-demo help` for usage")]
    Usage(String),
}

impl DemoError {
    /// 2 for a usage or capability problem the user can fix before a run, 1 for a failed run.
    pub fn exit_code(&self) -> u8 {
        match self {
            DemoError::Usage(_) | DemoError::Preflight(_) => 2,
            _ => 1,
        }
    }
}
