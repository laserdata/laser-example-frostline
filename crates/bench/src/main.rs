#![forbid(unsafe_code)]

use frostline_bench::collect::process;
use frostline_bench::report::BenchResult;
use frostline_bench::runtime::{self, binary};
use frostline_bench::trial::{self, TrialContext};
use frostline_bench::{BenchError, Profile, profile, profiles, selectivity};
use frostline_shared::config::LogFormat;
use frostline_shared::output::{self, fact, phase};
use frostline_shared::{LaserFactory, init_tracing};
use std::env;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::error;

const CONSUMER_BINARY: &str = "frostline-consumers";
const DEFAULT_PROFILE_SECONDS: u64 = 30;
const OUTPUT_ROOT: &str = "runs/bench";
const STRACE_INSTALL: &str = "Install strace, see the tools section of docs/benchmarks.md. The bench counts each reader's socket bytes from its syscalls.";

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
    init_tracing(LogFormat::Pretty);
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(failure) => {
            error!("the bench failed. {failure}");
            ExitCode::from(failure.exit_code())
        }
    }
}

async fn run() -> Result<(), BenchError> {
    let arguments: Vec<String> = env::args().skip(1).collect();
    let words: Vec<&str> = arguments.iter().map(String::as_str).collect();
    match words.as_slice() {
        ["list"] => {
            phase("profiles");
            for profile in &profiles::ALL {
                fact(profile.name, profile.description);
            }
            Ok(())
        }
        ["bench", name, rest @ ..] => {
            let mut profile = *Profile::by_name(name)
                .ok_or_else(|| BenchError::UnknownProfile((*name).to_owned()))?;
            if let Some(count) = option(rest, "--poll-records") {
                profile.poll_records = count.parse().map_err(|_| BenchError::Usage)?;
                if !(1..=1000).contains(&profile.poll_records) {
                    return Err(BenchError::Usage);
                }
            }
            bench(&profile, option(rest, "--out").map(PathBuf::from)).await
        }
        ["selectivity", rest @ ..] => {
            let directory = option(rest, "--out")
                .map_or_else(|| output_directory("selectivity"), PathBuf::from);
            profile::require("strace", STRACE_INSTALL).await?;
            let factory = LaserFactory::from_env()?;
            let rows = selectivity::run(
                &factory,
                &directory,
                process::ticks_per_second()?,
                rest.contains(&"--matrix"),
            )
            .await?;
            phase("selectivity");
            for row in &rows {
                fact(
                    &format!("{} per mille", row.per_mille),
                    format!("{} of the full feed wire avoided", row.wire_reduction),
                );
            }
            fact("written", directory.join("selectivity.md").display());
            Ok(())
        }
        ["prepare-selectivity", stream, case] => {
            let factory = LaserFactory::from_env()?;
            let case = selectivity::cases::Case::find(case).ok_or(BenchError::Usage)?;
            selectivity::dataset::publish(&factory, stream, case).await?;
            println!("{}", serde_json::to_string(&case)?);
            Ok(())
        }
        ["read-selectivity", stream, mode, case, identity] => {
            let factory = LaserFactory::from_env()?;
            let outcome = selectivity::read(
                &factory,
                stream,
                *mode == "filtered",
                selectivity::cases::Case::find(case).ok_or(BenchError::Usage)?,
                identity,
            )
            .await?;
            println!("{}", serde_json::to_string(&outcome)?);
            Ok(())
        }
        ["profile-cpu", target, rest @ ..] => {
            profile::cpu(
                target,
                seconds(rest)?,
                &output_directory(&format!("cpu-{target}")),
            )
            .await
        }
        ["profile-memory", target, rest @ ..] => {
            profile::memory(
                target,
                seconds(rest)?,
                &output_directory(&format!("memory-{target}")),
            )
            .await
        }
        _ => Err(BenchError::Usage),
    }
}

async fn bench(profile: &Profile, out: Option<PathBuf>) -> Result<(), BenchError> {
    let directory = out.unwrap_or_else(|| output_directory(profile.name));
    let consumer_binary = env::current_exe()?.with_file_name(CONSUMER_BINARY);
    if !consumer_binary.exists() {
        return Err(BenchError::Invalid(format!(
            "{} is missing, build it with `cargo build --release --workspace`",
            consumer_binary.display()
        )));
    }
    profile::require("strace", STRACE_INSTALL).await?;
    let runtime = runtime::local_runtime();
    if runtime.is_empty() {
        return Err(BenchError::NoLocalRuntime);
    }
    let mut binaries = vec![
        binary("frostline-bench", &env::current_exe()?)?,
        binary(CONSUMER_BINARY, &consumer_binary)?,
    ];
    binaries.extend(runtime.iter().map(|process| process.binary.clone()));
    let factory = LaserFactory::from_env()?;
    let context = TrialContext {
        profile,
        factory: &factory,
        directory: directory.clone(),
        consumer_binary,
        runtime,
        ticks_per_second: process::ticks_per_second()?,
    };
    let mut repetitions = Vec::new();
    for index in 1..=profile.repetitions {
        repetitions.push(trial::repetition(&context, index).await?);
    }
    let dataset = profile.settings(directory.clone()).dataset_variables();
    let result = BenchResult::new(profile, dataset, binaries, repetitions)?;
    result.write(&directory)?;
    phase("result");
    for line in result
        .markdown()
        .lines()
        .filter(|line| line.starts_with('|'))
    {
        tracing::info!("{line}");
    }
    fact("written", directory.join("result.md").display());
    Ok(())
}

fn option<'a>(rest: &[&'a str], name: &str) -> Option<&'a str> {
    rest.windows(2)
        .find(|pair| pair[0] == name)
        .map(|pair| pair[1])
}

fn seconds(rest: &[&str]) -> Result<u64, BenchError> {
    option(rest, "--seconds").map_or(Ok(DEFAULT_PROFILE_SECONDS), |value| {
        value.parse().map_err(|_| BenchError::Usage)
    })
}

fn output_directory(name: &str) -> PathBuf {
    let started = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs());
    PathBuf::from(OUTPUT_ROOT)
        .join(name)
        .join(started.to_string())
}
