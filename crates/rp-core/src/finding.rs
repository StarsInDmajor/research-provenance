use std::{fmt, path::Component, sync::Arc};

use serde::Serialize;
use serde_json::Value;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Status {
    Ok,
    Invalid,
    Denied,
    NotFound,
    Conflict,
    UsageError,
    IoError,
    InternalError,
    Interrupted,
}

impl Status {
    #[must_use]
    pub const fn exit_code(&self) -> u8 {
        match self {
            Self::Ok => 0,
            Self::Invalid | Self::Denied | Self::NotFound | Self::Conflict => 1,
            Self::UsageError => 2,
            Self::IoError => 3,
            Self::InternalError => 4,
            Self::Interrupted => 130,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
    Info,
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct ProjectPath(Arc<str>);

impl ProjectPath {
    pub fn new(path: impl Into<String>) -> Result<Self, ProjectPathError> {
        let path = path.into();
        let is_windows_drive = path.as_bytes().get(1) == Some(&b':')
            && path.as_bytes().first().is_some_and(u8::is_ascii_alphabetic);
        if path.is_empty() || path.contains('\0') || path.contains('\\') || is_windows_drive {
            return Err(ProjectPathError);
        }

        let candidate = std::path::Path::new(&path);
        if candidate.is_absolute()
            || candidate
                .components()
                .any(|component| !matches!(component, Component::Normal(_)))
            || percent_decodes_to_unsafe_path(&path)
        {
            return Err(ProjectPathError);
        }

        Ok(Self(path.into()))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn percent_decodes_to_unsafe_path(path: &str) -> bool {
    let bytes = path.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut introduced_separator = false;
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let Some(high) = hex_value(bytes[index + 1]) else {
                decoded.push(bytes[index]);
                index += 1;
                continue;
            };
            let Some(low) = hex_value(bytes[index + 2]) else {
                decoded.push(bytes[index]);
                index += 1;
                continue;
            };
            let value = (high << 4) | low;
            introduced_separator |= matches!(value, b'/' | b'\\');
            decoded.push(value);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }

    introduced_separator
        || decoded.contains(&b'\\')
        || decoded.first() == Some(&b'/')
        || decoded
            .split(|byte| *byte == b'/')
            .any(|component| component == b"." || component == b".." || component.contains(&b'/'))
        || decoded.windows(3).any(|window| {
            window[0] == b'%'
                && matches!(
                    (hex_value(window[1]), hex_value(window[2])),
                    (Some(_), Some(_))
                )
        })
}

const fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProjectPathError;

impl fmt::Display for ProjectPathError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("path must be normalized and project-relative")
    }
}

impl std::error::Error for ProjectPathError {}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Finding {
    pub schema: &'static str,
    pub error_code: &'static str,
    pub finding_family: &'static str,
    pub severity: Severity,
    pub message: String,
    pub source_file: Option<ProjectPath>,
    pub json_pointer: String,
}

impl Finding {
    #[must_use]
    pub fn new(
        error_code: &'static str,
        finding_family: &'static str,
        severity: Severity,
        message: impl Into<String>,
        source_file: Option<ProjectPath>,
        json_pointer: impl Into<String>,
    ) -> Self {
        Self {
            schema: "rp/finding/v1",
            error_code,
            finding_family,
            severity,
            message: sanitize_terminal_text(&message.into()),
            source_file,
            json_pointer: json_pointer.into(),
        }
    }

    #[must_use]
    pub fn cli_usage(message: impl Into<String>) -> Self {
        Self::new(
            "RP_E_CLI_USAGE",
            "cli_usage",
            Severity::Error,
            message,
            None,
            "",
        )
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CommandResult {
    pub schema: &'static str,
    pub command: String,
    pub status: Status,
    pub exit_code: u8,
    pub as_of: Option<String>,
    pub findings: Vec<Finding>,
    pub data: Option<Value>,
}

impl CommandResult {
    #[must_use]
    pub fn new(
        command: impl Into<String>,
        status: Status,
        as_of: Option<String>,
        mut findings: Vec<Finding>,
        data: Option<Value>,
    ) -> Self {
        findings.sort_by(|left, right| {
            (
                left.severity,
                left.source_file.as_ref(),
                left.json_pointer.as_str(),
                left.error_code,
            )
                .cmp(&(
                    right.severity,
                    right.source_file.as_ref(),
                    right.json_pointer.as_str(),
                    right.error_code,
                ))
        });
        let exit_code = status.exit_code();
        Self {
            schema: "rp/cli-result/v1",
            command: command.into(),
            status,
            exit_code,
            as_of,
            findings,
            data,
        }
    }
}

fn sanitize_terminal_text(text: &str) -> String {
    text.chars()
        .map(|character| {
            if character.is_control()
                || matches!(
                    character,
                    '\u{061c}'
                        | '\u{200e}'
                        | '\u{200f}'
                        | '\u{202a}'..='\u{202e}'
                        | '\u{2066}'..='\u{2069}'
                )
            {
                '\u{fffd}'
            } else {
                character
            }
        })
        .collect()
}
