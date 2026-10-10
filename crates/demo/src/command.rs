use crate::error::DemoError;
use frostline_shared::config::Mode;
use frostline_shared::output::{fact, phase};
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    Run(Mode),
    Doctor,
    Setup,
    Report { manifest: PathBuf },
    Cleanup { manifest: PathBuf },
    Help,
}

impl Command {
    pub fn from_args() -> Result<Self, DemoError> {
        Self::parse(std::env::args().skip(1))
    }

    pub fn parse(mut arguments: impl Iterator<Item = String>) -> Result<Self, DemoError> {
        let Some(command) = arguments.next() else {
            return Ok(Command::Run(Mode::Live));
        };
        let mut manifest = || -> Result<PathBuf, DemoError> {
            match (arguments.next().as_deref(), arguments.next()) {
                (Some("--manifest"), Some(path)) => Ok(PathBuf::from(path)),
                _ => Err(DemoError::Usage(format!(
                    "{command} needs --manifest <run.json>"
                ))),
            }
        };
        let parsed = match command.as_str() {
            "finite" => Command::Run(Mode::Finite),
            "live" => Command::Run(Mode::Live),
            "compare" => Command::Run(Mode::Compare),
            "codecs" => Command::Run(Mode::Codecs),
            "doctor" => Command::Doctor,
            "setup" => Command::Setup,
            "report" => Command::Report {
                manifest: manifest()?,
            },
            "cleanup" => Command::Cleanup {
                manifest: manifest()?,
            },
            "help" | "--help" | "-h" => Command::Help,
            other => return Err(DemoError::Usage(format!("unknown command '{other}'"))),
        };
        Ok(parsed)
    }
}

pub fn print_help() {
    phase("commands");
    fact(
        "live",
        "run the fleet until Ctrl+C with a live board (default)",
    );
    fact("finite", "run one finite story, report, and clean up");
    fact(
        "compare",
        "read the same window with filters and with full-feed readers",
    );
    fact(
        "codecs",
        "repeat the finite story in JSON, CBOR, Avro, and Protobuf",
    );
    fact(
        "doctor",
        "check the connection, filters, the catalog, and codecs",
    );
    fact(
        "setup",
        "create a run and write its run.json for standalone processes",
    );
    fact("report", "report --manifest <run.json> renders a saved run");
    fact(
        "cleanup",
        "cleanup --manifest <run.json> deletes only that run's resources",
    );
    fact("settings", "FROSTLINE_* variables, see docs/operations.md");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_no_arguments_when_parsed_then_should_run_live() {
        assert_eq!(parse(&[]).expect("parses"), Command::Run(Mode::Live));
    }

    #[test]
    fn given_each_command_when_parsed_then_should_map_to_its_variant() {
        assert_eq!(
            parse(&["finite"]).expect("parses"),
            Command::Run(Mode::Finite)
        );
        assert_eq!(
            parse(&["compare"]).expect("parses"),
            Command::Run(Mode::Compare)
        );
        assert_eq!(
            parse(&["codecs"]).expect("parses"),
            Command::Run(Mode::Codecs)
        );
        assert_eq!(parse(&["doctor"]).expect("parses"), Command::Doctor);
        assert_eq!(
            parse(&["report", "--manifest", "runs/a/run.json"]).expect("parses"),
            Command::Report {
                manifest: PathBuf::from("runs/a/run.json")
            }
        );
    }

    #[test]
    fn given_a_manifest_command_without_a_path_when_parsed_then_should_explain_usage() {
        assert!(matches!(parse(&["cleanup"]), Err(DemoError::Usage(_))));
        assert!(matches!(
            parse(&["cleanup", "--manifest"]),
            Err(DemoError::Usage(_))
        ));
    }

    #[test]
    fn given_an_unknown_command_when_parsed_then_should_fail() {
        assert!(matches!(parse(&["fly"]), Err(DemoError::Usage(_))));
    }

    fn parse(arguments: &[&str]) -> Result<Command, DemoError> {
        Command::parse(arguments.iter().map(|argument| (*argument).to_owned()))
    }
}
