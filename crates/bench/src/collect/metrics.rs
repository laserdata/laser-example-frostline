use crate::error::BenchError;
use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Runtime-owned snapshots are outside the timed phase. Direct remote runs omit them.
pub fn snapshot(path: &Path) -> Result<(), BenchError> {
    if env::var_os("FROSTLINE_HTTP_PORT").is_none() {
        return Ok(());
    }
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../scripts/runtime-metrics");
    let result = Command::new("python3").arg(script).arg(path).output()?;
    if !result.status.success() {
        return Err(BenchError::Invalid(format!(
            "filter metrics snapshot failed: {}",
            String::from_utf8_lossy(&result.stderr)
        )));
    }
    Ok(())
}
