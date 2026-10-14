use crate::config::LogFormat;
use tracing::Metadata;
use tracing_subscriber::filter::filter_fn;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Layer};

const NARRATION_TARGET: &str = "frostline_shared::output";
const DEFAULT_FILTER: &str = "info,iggy=warn";

/// Narration prints bare lines, application events print with their fields. An explicit RUST_LOG wins.
pub fn init_tracing(format: LogFormat) {
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(DEFAULT_FILTER));
    if format == LogFormat::Json {
        let _ = tracing_subscriber::registry()
            .with(tracing_subscriber::fmt::layer().json().with_filter(filter))
            .try_init();
        return;
    }
    let ansi = crate::output::ansi_enabled();
    let narration = tracing_subscriber::fmt::layer()
        .with_ansi(ansi)
        .with_ansi_sanitization(false)
        .without_time()
        .with_level(false)
        .with_target(false)
        .with_filter(filter_fn(is_narration))
        .with_filter(filter.clone());
    let application = tracing_subscriber::fmt::layer()
        .with_ansi(ansi)
        .with_filter(filter_fn(|metadata| !is_narration(metadata)))
        .with_filter(filter);
    let _ = tracing_subscriber::registry()
        .with(narration)
        .with(application)
        .try_init();
}

fn is_narration(metadata: &Metadata<'_>) -> bool {
    metadata.target() == NARRATION_TARGET
}
