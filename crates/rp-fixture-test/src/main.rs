use std::{io::Write, path::PathBuf, process::ExitCode};

use clap::Parser;
use rp_core::{CommandResult, Finding, Severity, Status};
use rp_fixture_test::run_fixture;
use serde_json::json;

#[derive(Debug, Parser)]
#[command(
    name = "rp-fixture-test",
    about = "Materialize and validate one RP fixture overlay"
)]
struct Cli {
    #[arg(long)]
    baseline: PathBuf,
    #[arg(long)]
    overlay: PathBuf,
    #[arg(long)]
    json: bool,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match run_fixture(&cli.baseline, &cli.overlay) {
        Ok(outcome) => {
            let data = json!({
                "schema": "rp/cli-data/fixture-test/v1",
                "fixture": outcome.fixture,
                "overlay": outcome.overlay,
                "expectation_matched": outcome.expectation_matched,
                "observed_exit_code": outcome.observed_exit_code,
                "observed_error_codes": outcome.observed_error_codes,
            });
            if outcome.expectation_matched {
                CommandResult::new(
                    "rp-fixture-test",
                    if outcome.observed_exit_code == 0 {
                        Status::Ok
                    } else {
                        Status::Invalid
                    },
                    None,
                    Vec::new(),
                    Some(data),
                )
            } else {
                CommandResult::new(
                    "rp-fixture-test",
                    Status::InternalError,
                    None,
                    vec![Finding::new(
                        "RP_E_INTERNAL_INVARIANT",
                        "internal_error",
                        Severity::Error,
                        "observed Findings did not match the overlay descriptor",
                        None,
                        "",
                    )],
                    Some(data),
                )
            }
        }
        Err(error) => CommandResult::new(
            "rp-fixture-test",
            Status::InternalError,
            None,
            vec![Finding::new(
                "RP_E_INTERNAL_INVARIANT",
                "internal_error",
                Severity::Error,
                error.to_string(),
                None,
                "",
            )],
            None,
        ),
    };
    let exit_code = result.exit_code;
    if cli.json {
        let stdout = std::io::stdout();
        let mut output = stdout.lock();
        if serde_json::to_writer(&mut output, &result).is_err() || writeln!(output).is_err() {
            return ExitCode::from(4);
        }
    } else if result.status == Status::Ok || result.status == Status::Invalid {
        println!("rp-fixture-test: expectation matched");
    } else {
        eprintln!("rp-fixture-test: expectation mismatch or harness failure");
    }
    ExitCode::from(exit_code)
}
