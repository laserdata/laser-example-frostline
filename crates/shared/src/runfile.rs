use crate::codec::Codec;
use crate::config::{Catalog, Settings};
use crate::names::RunId;
use crate::topology::SourceIdentity;
use laser_sdk::filters::FilterBinding;
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};
use thiserror::Error;

const LOCK_SUFFIX: &str = "lock";

/// Everything a standalone process needs to join an existing run. It never holds a credential.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct RunFile {
    pub run_id: RunId,
    pub settings_digest: String,
    pub generator_version: String,
    pub host: String,
    pub partitions: u32,
    #[serde(default = "default_workers")]
    pub workers_per_role: u8,
    /// Records per checkpoint window. Every reader bounds its outstanding
    /// pages from this value, not from its own settings.
    pub checkpoint_records: u64,
    pub codec: Codec,
    pub catalog: Catalog,
    pub source: SourceIdentity,
    pub schema_ids: Vec<u32>,
    pub groups: Vec<GroupRecord>,
}

/// One reader group of the run, its filter digest, and the exact saved binding.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct GroupRecord {
    pub group: String,
    pub digest: String,
    pub binding: Option<FilterBinding>,
}

#[derive(Debug, Error)]
pub enum RunFileError {
    #[error("run file {path} could not be accessed: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("run file {path} is not valid: {source}")]
    Json {
        path: PathBuf,
        source: serde_json::Error,
    },
    #[error("another producer holds the run, remove {0} if no producer is running")]
    Locked(PathBuf),
    #[error("the run file expects {field} {expected}, these settings give {got}")]
    Incompatible {
        field: &'static str,
        expected: String,
        got: String,
    },
    #[error("the run file has no group {0}")]
    UnknownGroup(String),
}

impl RunFile {
    pub fn write(&self, path: &Path) -> Result<(), RunFileError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|source| io(path, source))?;
        }
        let bytes = serde_json::to_vec_pretty(self).map_err(|source| RunFileError::Json {
            path: path.to_owned(),
            source,
        })?;
        fs::write(path, bytes).map_err(|source| io(path, source))
    }

    pub fn read(path: &Path) -> Result<Self, RunFileError> {
        let bytes = fs::read(path).map_err(|source| io(path, source))?;
        serde_json::from_slice(&bytes).map_err(|source| RunFileError::Json {
            path: path.to_owned(),
            source,
        })
    }

    /// Claim the run on this machine until cleanup. The file is not a distributed lease.
    pub fn lock(path: &Path) -> Result<(), RunFileError> {
        let lock = path.with_extension(LOCK_SUFFIX);
        match OpenOptions::new().write(true).create_new(true).open(&lock) {
            Ok(_) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                Err(RunFileError::Locked(lock))
            }
            Err(source) => Err(io(&lock, source)),
        }
    }

    pub fn ensure_compatible(&self, settings: &Settings) -> Result<(), RunFileError> {
        let checks = [
            (
                "settings_digest",
                self.settings_digest.clone(),
                settings.digest(),
            ),
            (
                "workers_per_role",
                self.workers_per_role.to_string(),
                settings.workers_per_role.to_string(),
            ),
            (
                "partitions",
                self.partitions.to_string(),
                settings.partitions.to_string(),
            ),
            ("codec", self.codec.to_string(), settings.codec.to_string()),
            (
                "catalog",
                self.catalog.to_string(),
                settings.catalog.to_string(),
            ),
        ];
        for (field, expected, got) in checks {
            if expected != got {
                return Err(RunFileError::Incompatible {
                    field,
                    expected,
                    got,
                });
            }
        }
        Ok(())
    }

    pub fn reader_group(&self, name: &str, worker: u8) -> Result<&GroupRecord, RunFileError> {
        if self.workers_per_role > 1 {
            self.group(&format!("{name}-worker-{worker}"))
        } else {
            self.group(name)
        }
    }

    pub fn group(&self, name: &str) -> Result<&GroupRecord, RunFileError> {
        self.groups
            .iter()
            .find(|record| record.group == name)
            .ok_or_else(|| RunFileError::UnknownGroup(name.to_owned()))
    }
}

const fn default_workers() -> u8 {
    1
}

fn io(path: &Path, source: std::io::Error) -> RunFileError {
    RunFileError::Io {
        path: path.to_owned(),
        source,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Mode;

    #[test]
    fn given_a_run_file_when_written_and_read_then_should_round_trip_without_secrets() {
        let dir = std::env::temp_dir().join(format!("frostline-runfile-{}", std::process::id()));
        let path = dir.join("run.json");
        run_file().write(&path).expect("run file writes");
        assert_eq!(RunFile::read(&path).expect("run file reads"), run_file());
        let text = fs::read_to_string(&path).expect("run file text");
        assert!(!text.contains('@') && !text.contains("password"));
        fs::remove_dir_all(dir).expect("temp dir removes");
    }

    #[test]
    fn given_a_held_lock_when_claimed_again_then_should_refuse() {
        let path = std::env::temp_dir().join(format!("frostline-lock-{}.json", std::process::id()));
        RunFile::lock(&path).expect("first claim succeeds");
        assert!(matches!(RunFile::lock(&path), Err(RunFileError::Locked(_))));
        fs::remove_file(path.with_extension(LOCK_SUFFIX)).expect("cleanup releases the claim");
    }

    #[test]
    fn given_other_partitions_when_checked_then_should_name_the_field() {
        let settings = Settings {
            partitions: 8,
            ..Settings::defaults(Mode::Finite)
        };
        assert!(matches!(
            run_file().ensure_compatible(&settings),
            Err(RunFileError::Incompatible {
                field: "settings_digest",
                ..
            })
        ));
        run_file()
            .ensure_compatible(&Settings::defaults(Mode::Finite))
            .expect("defaults match");
    }

    fn run_file() -> RunFile {
        RunFile {
            run_id: "0badc0de".parse().expect("run id"),
            settings_digest: Settings::defaults(Mode::Finite).digest(),
            generator_version: "1".to_owned(),
            host: "127.0.0.1".to_owned(),
            partitions: 4,
            workers_per_role: 1,
            checkpoint_records: 2_000,
            codec: Codec::Json,
            catalog: Catalog::Managed,
            source: SourceIdentity {
                stream_id: 1,
                stream_created_at_micros: 2,
                topic_id: 3,
                topic_created_at_micros: 4,
            },
            schema_ids: Vec::new(),
            groups: vec![GroupRecord {
                group: "maintenance".to_owned(),
                digest: "abc".to_owned(),
                binding: None,
            }],
        }
    }
}
