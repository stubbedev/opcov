use std::fmt;
use std::io;
use std::path::Path;
use std::process::ExitCode;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Verdict {
    Clean,
    Findings,
}

impl Verdict {
    pub(crate) fn exit_code(self) -> ExitCode {
        return match self {
            Self::Clean => ExitCode::SUCCESS,
            Self::Findings => ExitCode::from(1),
        };
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Severity {
    Refused,
    Failed,
}

impl Severity {
    pub(crate) fn exit_code(self) -> ExitCode {
        return ExitCode::from(match self {
            Self::Refused => 1,
            Self::Failed => 2,
        });
    }
}

#[derive(Debug)]
pub(crate) struct Failure {
    pub(crate) severity: Severity,
    pub(crate) message: String,
}

impl Failure {
    pub(crate) const fn failed(message: String) -> Self {
        return Self {
            severity: Severity::Failed,
            message,
        };
    }

    pub(crate) fn io(action: &str, path: &Path, error: &io::Error) -> Self {
        return Self::failed(format!("cannot {action} {}: {error}", path.display()));
    }
}

impl fmt::Display for Failure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        return formatter.write_str(&self.message);
    }
}
