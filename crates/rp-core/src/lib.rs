mod access;
mod claim;
mod content;
mod content_observations;
mod digest;
mod execution;
pub use execution::{ExecutionBudget, ExecutionStop};
mod finding;
mod findings;
mod init;
mod model;
mod navigation;
mod project;
mod report_acquisition;
mod report_binding;
mod report_fingerprint;
#[cfg(test)]
mod report_fingerprint_tests;
mod report_json;
mod report_subject;
#[cfg(test)]
mod report_subject_tests;
mod schema;
mod yaml;

pub use access::{
    AccessLabel, AccessLevel, AccessReport, DependencyStep, ExportDecision, ExportRequest,
};
pub use claim::{ClaimChainReport, SourceClosureEntry};
pub use digest::{DigestError, canonicalize_jcs, jcs_sha256};
pub use finding::{CommandResult, Finding, ProjectPath, ProjectPathError, Severity, Status};
pub use findings::{DEFAULT_FINDINGS_LIMIT, FindingOutput};
pub use init::{InitError, InitReport, initialize_project, initialize_project_with_budget};
pub use model::{ObjectRecord, ObjectType, ProjectIndex, ReferenceEdge, ReferenceExpectation};
pub use navigation::{
    DiffChange, DiffDto, FreshnessCounts, FreshnessEvaluation, FreshnessStatus, HistoryDto,
    NavigationError, OversizedReason, OverviewDto, QueryDto, QueryOptions, ShowDto, SnapshotDto,
    ThreadListDto, ThreadShowDto,
};
pub use project::{
    ProjectLimits, ProjectLoadError, ValidationReport, validate_project,
    validate_project_with_budget, validate_project_with_limits,
};
pub use report_acquisition::UnboundReportCandidate;
pub use report_binding::{
    ReportBindingObservation, ReportComparison, ReportLabels, ReportSubjectUnavailable,
};
pub use report_json::{ReportJsonError, ReportJsonLimits, parse_report_json};
pub use schema::{EmbeddedResource, SchemaBundle, SchemaBundleError};
pub use yaml::{ParsedYaml, SourceSpan, YamlLimits, parse_restricted_yaml};
