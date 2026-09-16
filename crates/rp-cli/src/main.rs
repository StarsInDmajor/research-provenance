use std::{collections::BTreeSet, ffi::OsString, io::Write, path::PathBuf, process::ExitCode};

use clap::{Args, Parser, Subcommand, error::ErrorKind};
use rp_core::{
    AccessLevel, CommandResult, ExecutionBudget, ExportRequest, Finding, FreshnessStatus,
    InitError, NavigationError, ProjectLimits, ProjectLoadError, QueryOptions, Severity, Status,
    initialize_project_with_budget, validate_project_with_budget,
};
use serde_json::json;
#[cfg(test)]
mod execution_tests;

#[derive(Debug, Parser)]
#[command(name = "rp", about = "Research Provenance Workbench", version)]
struct Cli {
    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    command: RpCommand,
}

#[derive(Debug, Subcommand)]
enum RpCommand {
    Init(ProjectOnly),
    Validate(ProjectOnly),
    Overview(ProjectAsOf),
    Show(ShowArgs),
    History(HistoryArgs),
    Diff(DiffArgs),
    Query(QueryArgs),
    Thread(ThreadArgs),
    Chain(ChainArgs),
    Access(AccessArgs),
    Export(ExportArgs),
    Snapshot(ProjectAsOf),
}

#[derive(Debug, Args)]
struct ProjectOnly {
    #[arg(long)]
    project: PathBuf,
}

#[derive(Debug, Args)]
struct ProjectAsOf {
    #[arg(long)]
    project: PathBuf,
    #[arg(long)]
    as_of: Option<String>,
}

#[derive(Debug, Args)]
struct ShowArgs {
    #[arg(long)]
    project: PathBuf,
    id: String,
    #[arg(long)]
    as_of: Option<String>,
}

#[derive(Debug, Args)]
struct HistoryArgs {
    #[arg(long)]
    project: PathBuf,
    logical_id: String,
}

#[derive(Debug, Args)]
struct DiffArgs {
    #[arg(long)]
    project: PathBuf,
    revision_id_a: String,
    revision_id_b: String,
}

#[derive(Debug, Args)]
struct QueryArgs {
    #[arg(long)]
    project: PathBuf,
    #[arg(long)]
    kind: Option<String>,
    #[arg(long)]
    thread: Option<String>,
    #[arg(long)]
    freshness: Option<String>,
    #[arg(long)]
    as_of: Option<String>,
    #[arg(long)]
    limit: Option<usize>,
}

#[derive(Debug, Args)]
struct ThreadArgs {
    #[command(subcommand)]
    command: ThreadCommand,
}

#[derive(Debug, Subcommand)]
enum ThreadCommand {
    List(ProjectOnly),
    Show(ThreadShowArgs),
}

#[derive(Debug, Args)]
struct ThreadShowArgs {
    #[arg(long)]
    project: PathBuf,
    thread_id: String,
    #[arg(long)]
    as_of: Option<String>,
}

#[derive(Debug, Args)]
struct ChainArgs {
    #[command(subcommand)]
    command: ChainCommand,
}

#[derive(Debug, Subcommand)]
enum ChainCommand {
    Validate(ChainValidateArgs),
}

#[derive(Debug, Args)]
struct ChainValidateArgs {
    #[arg(long)]
    project: PathBuf,
    chain_id: String,
    #[arg(long)]
    profile: String,
}

#[derive(Debug, Args)]
struct AccessArgs {
    #[command(subcommand)]
    command: AccessCommand,
}

#[derive(Debug, Subcommand)]
enum AccessCommand {
    Explain(AccessExplainArgs),
}

#[derive(Debug, Args)]
struct AccessExplainArgs {
    #[arg(long)]
    project: PathBuf,
    id: String,
}

#[derive(Debug, Args)]
struct ExportArgs {
    #[command(subcommand)]
    command: ExportCommand,
}

#[derive(Debug, Subcommand)]
enum ExportCommand {
    Check(ExportCheckArgs),
}

#[derive(Debug, Args)]
struct ExportCheckArgs {
    #[arg(long)]
    project: PathBuf,
    id: String,
    #[arg(long)]
    level_ceiling: String,
    #[arg(long)]
    compartment: Vec<String>,
    #[arg(long)]
    as_of: Option<String>,
}

fn main() -> ExitCode {
    let budget = ExecutionBudget::default();
    let arguments: Vec<OsString> = std::env::args_os().collect();
    let json_requested = arguments.iter().any(|argument| argument == "--json");
    let command_name = infer_command_name(&arguments);

    match Cli::try_parse_from(&arguments) {
        Ok(cli) => render_result(command_result_with_budget(&cli, &budget), cli.json, &budget),
        Err(error)
            if matches!(
                error.kind(),
                ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
            ) =>
        {
            let _ = error.print();
            ExitCode::SUCCESS
        }
        Err(error) if json_requested => {
            let result = CommandResult::new(
                command_name,
                Status::UsageError,
                None,
                vec![Finding::cli_usage(error.to_string())],
                None,
            );
            render_result(result, true, &budget)
        }
        Err(error) => {
            let _ = error.print();
            ExitCode::from(2)
        }
    }
}

fn command_result_with_budget(cli: &Cli, budget: &ExecutionBudget) -> CommandResult {
    let command = match &cli.command {
        RpCommand::Init(_) => "init",
        RpCommand::Validate(_) => "validate",
        RpCommand::Overview(_) => "overview",
        RpCommand::Show(_) => "show",
        RpCommand::History(_) => "history",
        RpCommand::Diff(_) => "diff",
        RpCommand::Query(_) => "query",
        RpCommand::Thread(ThreadArgs {
            command: ThreadCommand::List(_),
        }) => "thread list",
        RpCommand::Thread(_) => "thread show",
        RpCommand::Chain(_) => "chain validate",
        RpCommand::Access(_) => "access explain",
        RpCommand::Export(_) => "export check",
        RpCommand::Snapshot(_) => "snapshot",
    };
    if let Err(stop) = budget.checkpoint() {
        return terminal_result(command, None, stop);
    }
    let result = match &cli.command {
        RpCommand::Init(args) => init_result(&args.project, budget),
        RpCommand::Validate(args) => validate_result(&args.project, budget),
        RpCommand::Overview(args) => overview_result(args, budget),
        RpCommand::Show(args) => show_result(args, budget),
        RpCommand::History(args) => history_result(args, budget),
        RpCommand::Diff(args) => diff_result(args, budget),
        RpCommand::Query(args) => query_result(args, budget),
        RpCommand::Thread(ThreadArgs {
            command: ThreadCommand::List(args),
        }) => thread_list_result(args, budget),
        RpCommand::Thread(ThreadArgs {
            command: ThreadCommand::Show(args),
        }) => thread_show_result(args, budget),
        RpCommand::Chain(ChainArgs {
            command: ChainCommand::Validate(args),
        }) => chain_validate_result(args, budget),
        RpCommand::Access(AccessArgs {
            command: AccessCommand::Explain(args),
        }) => access_explain_result(args, budget),
        RpCommand::Export(ExportArgs {
            command: ExportCommand::Check(args),
        }) => export_check_result(args, budget),
        RpCommand::Snapshot(args) => snapshot_result(args, budget),
    };
    match budget.checkpoint() {
        Ok(()) => result,
        Err(stop) => terminal_result(command, result.as_of, stop),
    }
}

fn init_result(project: &std::path::Path, budget: &ExecutionBudget) -> CommandResult {
    match initialize_project_with_budget(project, budget) {
        Err(InitError::Stopped(stop)) => terminal_result("init", None, stop),
        Ok(report) => CommandResult::new(
            "init",
            Status::Ok,
            None,
            Vec::new(),
            Some(json!({
                "schema": "rp/cli-data/init/v1",
                "project_id": report.project_id,
                "created_paths": report.created_paths,
            })),
        ),
        Err(InitError::Conflict) => CommandResult::new(
            "init",
            Status::Conflict,
            None,
            vec![Finding::new(
                "RP_E_INIT_PROJECT_EXISTS",
                "init_conflict",
                Severity::Error,
                ".research already exists; refusing to overwrite",
                None,
                "",
            )],
            None,
        ),
        Err(InitError::Io(message)) => CommandResult::new(
            "init",
            Status::IoError,
            None,
            vec![Finding::new(
                "RP_E_IO_READ_FAILED",
                "io",
                Severity::Error,
                message,
                None,
                "",
            )],
            None,
        ),
    }
}

fn validate_result(project: &std::path::Path, budget: &ExecutionBudget) -> CommandResult {
    match validate_project_with_budget(project, ProjectLimits::default(), budget) {
        Ok(report) => {
            let valid = report.is_valid();
            let status = if valid { Status::Ok } else { Status::Invalid };
            CommandResult::new(
                "validate",
                status,
                None,
                report.findings,
                Some(json!({
                    "schema": "rp/cli-data/validate/v1",
                    "valid": valid,
                    "canonical_object_count": report.canonical_object_count,
                    "stage3_ran": report.stage3_ran,
                })),
            )
        }
        Err(error) => {
            let (status, code, family) = match &error {
                ProjectLoadError::Io(_) => (Status::IoError, "RP_E_IO_READ_FAILED", "io"),
                ProjectLoadError::Schema(_) | ProjectLoadError::UnsupportedPlatform => (
                    Status::InternalError,
                    "RP_E_INTERNAL_INVARIANT",
                    "internal_error",
                ),
            };
            CommandResult::new(
                "validate",
                status,
                None,
                vec![Finding::new(
                    code,
                    family,
                    Severity::Error,
                    error.to_string(),
                    None,
                    "",
                )],
                None,
            )
        }
    }
}

fn overview_result(args: &ProjectAsOf, budget: &ExecutionBudget) -> CommandResult {
    let as_of = match resolve_as_of(args.as_of.as_deref()) {
        Ok(as_of) => as_of,
        Err(result) => return result_for_usage("overview", args.as_of.clone(), result),
    };
    let index = match load_valid_index(&args.project, "overview", Some(as_of.clone()), budget) {
        Ok(index) => index,
        Err(result) => return result,
    };
    let overview = match index.overview(&as_of) {
        Ok(overview) => overview,
        Err(error) => return navigation_error_result("overview", Some(as_of), error),
    };
    CommandResult::new(
        "overview",
        Status::Ok,
        Some(as_of),
        Vec::new(),
        Some(with_schema("rp/cli-data/overview/v1", overview)),
    )
}

fn show_result(args: &ShowArgs, budget: &ExecutionBudget) -> CommandResult {
    let as_of = match resolve_as_of(args.as_of.as_deref()) {
        Ok(as_of) => as_of,
        Err(message) => return result_for_usage("show", args.as_of.clone(), message),
    };
    let index = match load_valid_index(&args.project, "show", Some(as_of.clone()), budget) {
        Ok(index) => index,
        Err(result) => return result,
    };
    match index.show(&args.id, &as_of) {
        Ok(show) => CommandResult::new(
            "show",
            Status::Ok,
            Some(as_of),
            Vec::new(),
            Some(with_schema("rp/cli-data/show/v1", show)),
        ),
        Err(error) => navigation_error_result("show", Some(as_of), error),
    }
}

fn history_result(args: &HistoryArgs, budget: &ExecutionBudget) -> CommandResult {
    let index = match load_valid_index(&args.project, "history", None, budget) {
        Ok(index) => index,
        Err(result) => return result,
    };
    match index.history(&args.logical_id) {
        Ok(history) => CommandResult::new(
            "history",
            Status::Ok,
            None,
            Vec::new(),
            Some(with_schema("rp/cli-data/history/v1", history)),
        ),
        Err(error) => navigation_error_result("history", None, error),
    }
}

fn diff_result(args: &DiffArgs, budget: &ExecutionBudget) -> CommandResult {
    let index = match load_valid_index(&args.project, "diff", None, budget) {
        Ok(index) => index,
        Err(result) => return result,
    };
    match index.diff(&args.revision_id_a, &args.revision_id_b) {
        Ok(diff) => CommandResult::new(
            "diff",
            Status::Ok,
            None,
            Vec::new(),
            Some(with_schema("rp/cli-data/diff/v1", diff)),
        ),
        Err(error) => navigation_error_result("diff", None, error),
    }
}

fn query_result(args: &QueryArgs, budget: &ExecutionBudget) -> CommandResult {
    let limit = args.limit.unwrap_or(10_000);
    if !(1..=100_000).contains(&limit) {
        return result_for_usage(
            "query",
            args.as_of.clone(),
            "--limit must be between 1 and 100000",
        );
    }
    let freshness = match args.freshness.as_deref() {
        Some(value) => match FreshnessStatus::parse(value) {
            Some(status) => Some(status),
            None => {
                return result_for_usage(
                    "query",
                    args.as_of.clone(),
                    "--freshness must be unknown, fresh, review-due, or stale",
                );
            }
        },
        None => None,
    };
    let as_of = match resolve_as_of(args.as_of.as_deref()) {
        Ok(as_of) => as_of,
        Err(message) => return result_for_usage("query", args.as_of.clone(), message),
    };
    let index = match load_valid_index(&args.project, "query", Some(as_of.clone()), budget) {
        Ok(index) => index,
        Err(result) => return result,
    };
    let query = match index.query(QueryOptions {
        kind: args.kind.clone(),
        thread_id: args.thread.clone(),
        freshness,
        as_of: as_of.clone(),
        limit,
    }) {
        Ok(query) => query,
        Err(error) => return navigation_error_result("query", Some(as_of), error),
    };
    CommandResult::new(
        "query",
        Status::Ok,
        Some(as_of),
        Vec::new(),
        Some(with_schema("rp/cli-data/query/v1", query)),
    )
}

fn thread_list_result(args: &ProjectOnly, budget: &ExecutionBudget) -> CommandResult {
    let index = match load_valid_index(&args.project, "thread list", None, budget) {
        Ok(index) => index,
        Err(result) => return result,
    };
    CommandResult::new(
        "thread list",
        Status::Ok,
        None,
        Vec::new(),
        Some(with_schema(
            "rp/cli-data/thread-list/v1",
            match index.thread_list_checked(10_000) {
                Ok(dto) => dto,
                Err(error) => return navigation_error_result("thread list", None, error),
            },
        )),
    )
}

fn thread_show_result(args: &ThreadShowArgs, budget: &ExecutionBudget) -> CommandResult {
    let as_of = match resolve_as_of(args.as_of.as_deref()) {
        Ok(as_of) => as_of,
        Err(message) => return result_for_usage("thread show", args.as_of.clone(), message),
    };
    let index = match load_valid_index(&args.project, "thread show", Some(as_of.clone()), budget) {
        Ok(index) => index,
        Err(result) => return result,
    };
    match index.thread_show(&args.thread_id, &as_of) {
        Ok(thread) => CommandResult::new(
            "thread show",
            Status::Ok,
            Some(as_of),
            Vec::new(),
            Some(with_schema("rp/cli-data/thread-show/v1", thread)),
        ),
        Err(error) => navigation_error_result("thread show", Some(as_of), error),
    }
}

fn snapshot_result(args: &ProjectAsOf, budget: &ExecutionBudget) -> CommandResult {
    let as_of = match resolve_as_of(args.as_of.as_deref()) {
        Ok(as_of) => as_of,
        Err(message) => return result_for_usage("snapshot", args.as_of.clone(), message),
    };
    let index = match load_valid_index(&args.project, "snapshot", Some(as_of.clone()), budget) {
        Ok(index) => index,
        Err(result) => return result,
    };
    match index.snapshot(&as_of) {
        Ok(snapshot) => CommandResult::new(
            "snapshot",
            Status::Ok,
            Some(as_of),
            Vec::new(),
            Some(with_schema("rp/cli-data/snapshot/v1", snapshot)),
        ),
        Err(error) => navigation_error_result("snapshot", Some(as_of), error),
    }
}

// The error is the final CLI envelope returned directly by every caller;
// boxing it would add allocation and unwrap churn without reducing live data.
#[allow(clippy::result_large_err)]
fn load_valid_index(
    project: &std::path::Path,
    command: &str,
    as_of: Option<String>,
    budget: &ExecutionBudget,
) -> Result<rp_core::ProjectIndex, CommandResult> {
    let report = validate_project_with_budget(project, ProjectLimits::default(), budget)
        .map_err(|error| project_error_result(command, as_of.clone(), error))?;
    if !report.is_valid() {
        return Err(CommandResult::new(
            command,
            Status::Invalid,
            as_of,
            report.findings,
            None,
        ));
    }
    report.index.ok_or_else(|| {
        CommandResult::new(
            command,
            Status::InternalError,
            as_of,
            vec![Finding::new(
                "RP_E_INTERNAL_INVARIANT",
                "internal_error",
                Severity::Error,
                "valid project did not produce an index",
                None,
                "",
            )],
            None,
        )
    })
}

fn resolve_as_of(value: Option<&str>) -> Result<String, &'static str> {
    match value {
        Some(value) => {
            value
                .parse::<jiff::Timestamp>()
                .map_err(|_| "--as-of must be an RFC 3339 timestamp")?;
            Ok(value.to_string())
        }
        None => Ok(jiff::Timestamp::now().to_string()),
    }
}

fn with_schema(schema: &str, value: impl serde::Serialize) -> serde_json::Value {
    let mut value = serde_json::to_value(value).expect("command DTO is serializable");
    value
        .as_object_mut()
        .expect("command DTO is an object")
        .insert(
            "schema".to_string(),
            serde_json::Value::String(schema.to_string()),
        );
    value
}

fn result_for_usage(
    command: &str,
    as_of: Option<String>,
    message: impl Into<String>,
) -> CommandResult {
    // Keep the caller's error precedence and only echo a valid supplied time.
    // Do not resolve the interactive default for an absent error-envelope time.
    let as_of = as_of.and_then(|value| resolve_as_of(Some(&value)).ok());
    CommandResult::new(
        command,
        Status::UsageError,
        as_of,
        vec![Finding::cli_usage(message)],
        None,
    )
}

fn navigation_error_result(
    command: &str,
    as_of: Option<String>,
    error: NavigationError,
) -> CommandResult {
    match error {
        NavigationError::Stopped(stop) => terminal_result(command, as_of, stop),
        NavigationError::NotFound => CommandResult::new(
            command,
            Status::NotFound,
            as_of,
            vec![not_found_finding("requested object")],
            None,
        ),
        NavigationError::InvalidAsOf | NavigationError::InvalidLimit => {
            result_for_usage(command, as_of, "invalid bounded navigation argument")
        }
        NavigationError::OversizedGraph(reason) => {
            let (code, msg) = match reason {
                rp_core::OversizedReason::SemanticNodes { count, limit } => (
                    "RP_E_RESOURCE_GRAPH_NODES_EXCEEDED",
                    format!("semantic node revisions ({count}) exceed limit of {limit}"),
                ),
                rp_core::OversizedReason::ScientificRelations { count, limit } => (
                    "RP_E_RESOURCE_GRAPH_EDGES_EXCEEDED",
                    format!(
                        "scientific relation revisions ({count}) exceed prototype limit of {limit}"
                    ),
                ),
                rp_core::OversizedReason::CanonicalRecords { count, limit } => (
                    "RP_E_RESOURCE_COLLECTION_SIZE_EXCEEDED",
                    format!("canonical records ({count}) exceed reader limit of {limit}"),
                ),
                rp_core::OversizedReason::CanonicalBytes { bytes, limit } => (
                    "RP_E_RESOURCE_TOTAL_BYTES_EXCEEDED",
                    format!("canonical bytes ({bytes}) exceed reader limit of {limit}"),
                ),
            };
            CommandResult::new(
                command,
                Status::Invalid,
                as_of,
                vec![Finding::new(
                    code,
                    "resource_limit",
                    Severity::Error,
                    msg,
                    None,
                    "",
                )],
                None,
            )
        }
        NavigationError::SerializationError(msg) => CommandResult::new(
            command,
            Status::InternalError,
            as_of,
            vec![Finding::new(
                "RP_E_INTERNAL_INVARIANT",
                "internal_error",
                Severity::Error,
                msg,
                None,
                "",
            )],
            None,
        ),
    }
}

fn chain_validate_result(args: &ChainValidateArgs, budget: &ExecutionBudget) -> CommandResult {
    let report = match validate_project_with_budget(&args.project, ProjectLimits::default(), budget)
    {
        Ok(report) => report,
        Err(error) => return project_error_result("chain validate", None, error),
    };
    let (mut findings, index) = report.into_command_parts();
    let Some(index) = index else {
        return CommandResult::new(
            "chain validate",
            Status::Invalid,
            None,
            findings.into_findings(),
            None,
        );
    };
    let Some(chain) = index.get(&args.chain_id) else {
        findings.push_error(|| not_found_finding(&args.chain_id));
        return CommandResult::new(
            "chain validate",
            Status::NotFound,
            None,
            findings.into_findings(),
            None,
        );
    };
    let Some(validation) = index.claim_chain_report(&args.chain_id) else {
        findings.push_error(|| {
            Finding::new(
                "RP_E_REFERENCE_TYPE_MISMATCH",
                "reference_type",
                Severity::Error,
                "requested ID is not a ClaimChainSnapshot",
                Some(chain.source_file.clone()),
                "",
            )
        });
        return CommandResult::new(
            "chain validate",
            Status::Invalid,
            None,
            findings.into_findings(),
            None,
        );
    };
    if validation.profile.as_ref() != args.profile {
        findings.push_error(|| {
            Finding::new(
                "RP_E_CLAIM_CHAIN_PROFILE_MISMATCH",
                "claim_chain_profile",
                Severity::Error,
                format!(
                    "requested profile {} does not match snapshot profile {}",
                    args.profile, validation.profile
                ),
                Some(chain.source_file.clone()),
                "/validation_policy/profile",
            )
        });
    }
    let valid =
        validation.valid && validation.profile.as_ref() == args.profile && findings.is_empty();
    let data = json!({
        "schema": "rp/cli-data/chain-validate/v1",
        "snapshot_id": args.chain_id,
        "profile": validation.profile,
        "valid": valid,
    });
    CommandResult::new(
        "chain validate",
        if valid { Status::Ok } else { Status::Invalid },
        None,
        findings.into_findings(),
        Some(data),
    )
}

fn access_explain_result(args: &AccessExplainArgs, budget: &ExecutionBudget) -> CommandResult {
    let report = match validate_project_with_budget(&args.project, ProjectLimits::default(), budget)
    {
        Ok(report) => report,
        Err(error) => return project_error_result("access explain", None, error),
    };
    let (mut findings, index) = report.into_command_parts();
    let Some(index) = index else {
        return CommandResult::new(
            "access explain",
            Status::Invalid,
            None,
            findings.into_findings(),
            None,
        );
    };
    let Some(access) = index.access_report(&args.id) else {
        findings.push_error(|| not_found_finding(&args.id));
        return CommandResult::new(
            "access explain",
            Status::NotFound,
            None,
            findings.into_findings(),
            None,
        );
    };
    let valid = access.valid && findings.is_empty();
    let ordered_dependency_explanation = match index.access_explanation_checked(&args.id) {
        Ok(Some(explanation)) => explanation,
        Err(stop) => return terminal_result("access explain", None, stop),
        Ok(None) => {
            return navigation_error_result("access explain", None, NavigationError::NotFound);
        }
    };
    let data = json!({
        "schema": "rp/cli-data/access-explain/v1",
        "target_id": args.id,
        "declared_access": access.declared,
        "required_dependency_floor": access.required_floor,
        "effective_access": access.effective,
        "dependency_count": access.dependency_count,
        "ordered_dependency_explanation": ordered_dependency_explanation,
    });
    CommandResult::new(
        "access explain",
        if valid { Status::Ok } else { Status::Invalid },
        None,
        findings.into_findings(),
        Some(data),
    )
}

fn export_check_result(args: &ExportCheckArgs, budget: &ExecutionBudget) -> CommandResult {
    let as_of = match resolve_as_of(args.as_of.as_deref()) {
        Ok(as_of) => as_of,
        // Invalid input is not a resolved RFC 3339 value in the result envelope.
        Err(message) => return result_for_usage("export check", None, message),
    };
    let Some(level_ceiling) = AccessLevel::parse(&args.level_ceiling) else {
        return CommandResult::new(
            "export check",
            Status::UsageError,
            Some(as_of),
            vec![Finding::cli_usage(
                "--level-ceiling must be public, internal, restricted, or exclusive",
            )],
            None,
        );
    };
    let report = match validate_project_with_budget(&args.project, ProjectLimits::default(), budget)
    {
        Ok(report) => report,
        Err(error) => return project_error_result("export check", Some(as_of), error),
    };
    let (mut findings, index) = report.into_command_parts();
    let Some(index) = index else {
        return CommandResult::new(
            "export check",
            Status::Invalid,
            Some(as_of),
            findings.into_findings(),
            None,
        );
    };
    let request = ExportRequest {
        level_ceiling,
        allowed_compartments: args.compartment.iter().cloned().collect::<BTreeSet<_>>(),
    };
    let Some(decision) = index.export_check(&args.id, request) else {
        findings.push_error(|| not_found_finding(&args.id));
        return CommandResult::new(
            "export check",
            Status::NotFound,
            Some(as_of),
            findings.into_findings(),
            None,
        );
    };
    let status = if !findings.is_empty() {
        Status::Invalid
    } else if decision.eligible {
        Status::Ok
    } else {
        Status::Denied
    };
    let data = json!({
        "schema": "rp/cli-data/export-check/v1",
        "target_id": args.id,
        "eligible": decision.eligible,
        "effective_access": decision.effective_access,
        "request": {
            "level_ceiling": decision.request.level_ceiling,
            "allowed_compartments": decision.request.allowed_compartments,
        },
    });
    CommandResult::new(
        "export check",
        status,
        Some(as_of),
        findings.into_findings(),
        Some(data),
    )
}

fn project_error_result(
    command: &str,
    as_of: Option<String>,
    error: ProjectLoadError,
) -> CommandResult {
    let (status, code, family) = match &error {
        ProjectLoadError::Io(_) => (Status::IoError, "RP_E_IO_READ_FAILED", "io"),
        ProjectLoadError::Schema(_) | ProjectLoadError::UnsupportedPlatform => (
            Status::InternalError,
            "RP_E_INTERNAL_INVARIANT",
            "internal_error",
        ),
    };
    CommandResult::new(
        command,
        status,
        as_of,
        vec![Finding::new(
            code,
            family,
            Severity::Error,
            error.to_string(),
            None,
            "",
        )],
        None,
    )
}

fn not_found_finding(id: &str) -> Finding {
    Finding::new(
        "RP_E_OBJECT_NOT_FOUND",
        "object_lookup",
        Severity::Error,
        format!("object {id} was not found"),
        None,
        "",
    )
}

fn terminal_result(
    command: &str,
    as_of: Option<String>,
    stop: rp_core::ExecutionStop,
) -> CommandResult {
    CommandResult::new(command, stop.status(), as_of, vec![stop.finding()], None)
}

struct RenderBuffer<'a> {
    bytes: Vec<u8>,
    budget: &'a ExecutionBudget,
    limit: usize,
}
impl Write for RenderBuffer<'_> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.budget.checkpoint().map_err(std::io::Error::other)?;
        if bytes.len() > self.limit.saturating_sub(self.bytes.len()) {
            return Err(std::io::Error::other("output byte cap"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
fn output_error(result: CommandResult, budget: &ExecutionBudget) -> CommandResult {
    if let Err(stop) = budget.checkpoint() {
        return terminal_result(&result.command, result.as_of, stop);
    }
    CommandResult::new(
        result.command,
        Status::Invalid,
        result.as_of,
        vec![Finding::new(
            "RP_E_RESOURCE_OUTPUT_SIZE_EXCEEDED",
            "resource_limit",
            Severity::Error,
            "rendered output exceeds the bounded output budget",
            None,
            "",
        )],
        None,
    )
}
fn encode_result(result: CommandResult, budget: &ExecutionBudget, limit: usize) -> (Vec<u8>, u8) {
    let mut writer = RenderBuffer {
        bytes: Vec::new(),
        budget,
        limit: limit.min(33_554_432),
    };
    let rendered =
        serde_json::to_writer(&mut writer, &result).is_ok() && writer.write_all(b"\n").is_ok();
    if rendered && budget.checkpoint().is_ok() {
        return (writer.bytes, result.exit_code);
    }
    // No stdout has been emitted. Discard ordinary output; one small fixed terminal
    // envelope bypasses the stopped execution/ordinary cap, never replenishes it.
    drop(writer);
    let terminal = output_error(result, budget);
    let code = terminal.exit_code;
    let mut bytes = Vec::new();
    serde_json::to_writer(&mut bytes, &terminal).expect("terminal DTO serializable");
    bytes.push(b'\n');
    (bytes, code)
}
fn render_result(result: CommandResult, json: bool, budget: &ExecutionBudget) -> ExitCode {
    if json {
        let (bytes, code) = encode_result(result, budget, 33_554_432);
        // Once selected, finish the envelope without cancellation mid-write.
        // Blocking stdout remains an external, non-cooperative limitation.
        return if std::io::stdout().lock().write_all(&bytes).is_ok() {
            ExitCode::from(code)
        } else {
            ExitCode::from(4)
        };
    }
    let mut writer = RenderBuffer {
        bytes: Vec::new(),
        budget,
        limit: 33_554_432,
    };
    let rendered = if result.status == Status::Ok {
        writeln!(writer, "{}: ok", result.command).is_ok()
    } else {
        result
            .findings
            .iter()
            .all(|f| writeln!(writer, "{}: {}", f.error_code, f.message).is_ok())
    };
    let (bytes, code, success) = if rendered && budget.checkpoint().is_ok() {
        (writer.bytes, result.exit_code, result.status == Status::Ok)
    } else {
        drop(writer);
        let terminal = output_error(result, budget);
        let f = &terminal.findings[0];
        (
            format!("{}: {}\n", f.error_code, f.message).into_bytes(),
            terminal.exit_code,
            false,
        )
    };
    let written = if success {
        std::io::stdout().lock().write_all(&bytes)
    } else {
        std::io::stderr().lock().write_all(&bytes)
    };
    if written.is_err() {
        ExitCode::from(4)
    } else {
        ExitCode::from(code)
    }
}

fn infer_command_name(arguments: &[OsString]) -> String {
    let words: Vec<_> = arguments
        .iter()
        .skip(1)
        .filter_map(|argument| argument.to_str())
        .filter(|argument| !argument.starts_with('-'))
        .collect();

    match words.as_slice() {
        ["thread", "list", ..] => "thread list",
        ["thread", "show", ..] => "thread show",
        ["chain", "validate", ..] => "chain validate",
        ["access", "explain", ..] => "access explain",
        ["export", "check", ..] => "export check",
        [command, ..] => command,
        [] => "rp",
    }
    .to_string()
}
