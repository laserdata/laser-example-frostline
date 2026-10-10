#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]

pub mod codec;
pub mod config;
pub mod connect;
pub mod domain;
pub mod event;
pub mod fixtures;
pub mod knobs;
pub mod lifecycle;
pub mod measure;
pub mod names;
pub mod output;
pub mod policy;
pub mod reports;
pub mod runfile;
pub mod telemetry;
pub mod topology;

#[cfg(feature = "testkit")]
pub mod testkit;

pub use config::{Catalog, LogFormat, Mode, Settings};
pub use connect::{ConnectionTarget, LaserFactory};
pub use knobs::ConfigError;
pub use lifecycle::{ServiceHandle, ShutdownWatch, eventually, shutdown_signal};
pub use telemetry::init_tracing;
