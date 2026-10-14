use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Written by `scripts/stack-local start`.
pub const PIDS_FILE: &str = "runs/stack/pids.json";

/// The name and reported release version of a binary used by a measurement.
#[derive(Clone, Debug, Serialize)]
pub struct BinaryVersion {
    pub name: String,
    pub version: String,
}

/// A running process of the local runtime.
#[derive(Clone, Debug, Serialize)]
pub struct RuntimeProcess {
    pub pid: u32,
    pub binary: BinaryVersion,
    #[serde(skip)]
    path: PathBuf,
}

#[derive(Debug, Deserialize)]
struct Pids {
    iggy: u32,
    plane: u32,
}

/// The iggy-server and plane processes of the local runtime, in that order. Empty when it is not running.
pub fn local_runtime() -> Vec<RuntimeProcess> {
    let Ok(text) = fs::read_to_string(
        std::env::var("FROSTLINE_RUNTIME_PIDS").unwrap_or_else(|_| PIDS_FILE.to_owned()),
    ) else {
        return Vec::new();
    };
    let Ok(pids) = serde_json::from_str::<Pids>(&text) else {
        return Vec::new();
    };
    [("iggy-server", pids.iggy), ("plane", pids.plane)]
        .into_iter()
        .filter_map(|(name, pid)| {
            let path = fs::read_link(format!("/proc/{pid}/exe")).ok()?;
            if !path.file_name()?.to_string_lossy().starts_with(name) {
                return None;
            }
            let binary = binary(name, Path::new(&format!("/proc/{pid}/exe"))).ok()?;
            Some(RuntimeProcess { pid, binary, path })
        })
        .collect()
}

/// The runtime process named `name`, refusing a pid that now belongs to another program.
pub fn runtime_process(name: &str) -> Option<RuntimeProcess> {
    local_runtime().into_iter().find(|process| {
        process.binary.name == name
            && process
                .path
                .file_name()
                .is_some_and(|file| file.to_string_lossy().starts_with(name))
    })
}

pub fn binary(name: &str, path: &Path) -> io::Result<BinaryVersion> {
    let output = Command::new(path).arg("--version").output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "{name} could not report its version"
        )));
    }
    let output = String::from_utf8(output.stdout).map_err(io::Error::other)?;
    let version = output
        .split_whitespace()
        .find(|word| word.as_bytes().first().is_some_and(u8::is_ascii_digit))
        .ok_or_else(|| io::Error::other(format!("{name} returned no version")))?;
    Ok(BinaryVersion {
        name: name.to_owned(),
        version: version.split('+').next().unwrap_or(version).to_owned(),
    })
}
