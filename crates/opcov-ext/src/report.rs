use std::{collections::BTreeMap, ffi::CStr, fmt};

use opcov_core::{
    cfg::{FilePath, FunctionKey, KeyError, Line},
    report::{
        FileCoverage, Function, Generator, Header, HitSet, ModelError, PathBase, Report, Sha256,
        SourcePath, TestIndex, TestTable, VmKind, line_hits,
    },
};
use opcov_sys::{ZTS, opcov_php_version, zend_vm_kind};

use crate::{
    VERSION,
    engine::{Engine, FileId, FileRecord, Source},
};

#[derive(Debug)]
pub(crate) enum ReportError {
    Model(ModelError),
    Key {
        path: SourcePath,
        error: KeyError,
    },
    SourceChanged {
        path: String,
        first: Sha256,
        then: Sha256,
    },
    SourceUnreadable {
        path: String,
        reason: String,
    },
    SourceUnknown {
        path: String,
    },
    VmKind(i32),
    PhpVersion,
    NotConfigured,
}

impl fmt::Display for ReportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        return match self {
            Self::Model(error) => write!(f, "{error}"),
            Self::Key { path, error } => {
                write!(f, "{path} cannot name the functions of its entry: {error}")
            }
            Self::SourceChanged { path, first, then } => write!(
                f,
                "{path} was compiled from different sources in this process (sha256 {first}, then {then}), so no single sha256 describes its coverage"
            ),
            Self::SourceUnreadable { path, reason } => {
                write!(f, "the source of {path} could not be hashed: {reason}")
            }
            Self::SourceUnknown { path } => {
                write!(f, "opcov recorded no source for the measured file {path}")
            }
            Self::VmKind(raw) => write!(f, "the Zend VM kind {raw} is unknown to opcov"),
            Self::PhpVersion => f.write_str("the PHP version is not valid UTF-8"),
            Self::NotConfigured => f.write_str("no file selection is configured for this request"),
        };
    }
}

impl From<ModelError> for ReportError {
    fn from(error: ModelError) -> Self {
        return Self::Model(error);
    }
}

pub(crate) struct Entry {
    pub(crate) lines: BTreeMap<Line, HitSet>,
    pub(crate) functions: BTreeMap<FunctionKey, Function>,
}

pub(crate) fn entry(
    record: &FileRecord,
    key_file: &FilePath,
    hits: impl IntoIterator<Item = (Line, TestIndex)>,
) -> Result<Entry, ModelError> {
    return Ok(Entry {
        lines: line_hits(record.lines.iter().copied(), hits)?,
        functions: record
            .functions
            .keys()
            .map(|key| return (key.in_file(key_file), Function::default()))
            .collect(),
    });
}

fn vm_kind() -> Result<VmKind, ReportError> {
    // SAFETY: zend_vm_kind only returns a compile-time constant of the running engine.
    let raw = unsafe { zend_vm_kind() };
    let kind = opcov_sys::VmKind::try_from(raw)
        .map_err(|unknown| return ReportError::VmKind(unknown.0))?;
    return match kind.name().to_bytes() {
        b"CALL" => Ok(VmKind::Call),
        b"SWITCH" => Ok(VmKind::Switch),
        b"GOTO" => Ok(VmKind::Goto),
        b"HYBRID" => Ok(VmKind::Hybrid),
        b"TAILCALL" => Ok(VmKind::Tailcall),
        _ => Err(ReportError::VmKind(raw)),
    };
}

fn generator() -> Result<Generator, ReportError> {
    // SAFETY: php_version returns a static NUL-terminated string.
    let php = unsafe { CStr::from_ptr(opcov_php_version()) }
        .to_str()
        .map_err(|_utf8| return ReportError::PhpVersion)?;
    return Ok(Generator {
        opcov: VERSION.to_string_lossy().into_owned(),
        php: php.to_owned(),
        zts: ZTS,
        vm: vm_kind()?,
    });
}

fn hashed(record: &FileRecord) -> Result<Option<Sha256>, ReportError> {
    let path = || return record.path.clone();
    return match &record.source {
        Some(Source::Hashed(hash)) => Ok(Some(*hash)),
        Some(Source::Changed { first, then }) => Err(ReportError::SourceChanged {
            path: path(),
            first: *first,
            then: *then,
        }),
        Some(Source::Unreadable(reason)) => Err(ReportError::SourceUnreadable {
            path: path(),
            reason: reason.clone(),
        }),
        None => Ok(None),
    };
}

impl Engine {
    pub(crate) fn report(&self, paths: PathBase) -> Result<Report, ReportError> {
        let settings = self
            .request
            .as_ref()
            .and_then(|request| return request.settings.clone())
            .ok_or(ReportError::NotConfigured)?;
        let attributed = self.attribution.tests();
        let names: Vec<String> = attributed
            .iter()
            .map(|test| return test.name.clone())
            .collect();
        let (tests, indexes) = TestTable::from_unordered(&names)?;
        let mut hits: BTreeMap<FileId, Vec<(Line, TestIndex)>> = BTreeMap::new();
        for (test, index) in attributed.iter().zip(indexes) {
            for &(file, line) in &test.lines {
                hits.entry(file).or_default().push((line, index));
            }
        }
        let mut files = BTreeMap::new();
        for (file, record) in self.files.iter() {
            let file_hits = hits.remove(&file).unwrap_or_default();
            let Some(sha256) = hashed(record)? else {
                if record.lines.is_empty() && file_hits.is_empty() {
                    continue;
                }
                return Err(ReportError::SourceUnknown {
                    path: record.path.clone(),
                });
            };
            let path = paths.locate(&record.path)?;
            let key_file: FilePath = path.as_str().parse().map_err(|error| {
                return ReportError::Key {
                    path: path.clone(),
                    error,
                };
            })?;
            let Entry { lines, functions } = entry(record, &key_file, file_hits)?;
            files.insert(
                path,
                FileCoverage {
                    sha256,
                    lines,
                    functions,
                },
            );
        }
        let header = Header {
            generator: generator()?,
            settings,
            paths,
            tests,
        };
        return Ok(Report::new(header, files)?);
    }
}
