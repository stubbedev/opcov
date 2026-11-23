use core::ptr::{self, NonNull};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use opcov_core::{
    filter::{
        CaseSensitivity, CompiledFile, Decision, SelectionConfig, SkipReason, eval_host,
        stream_scheme,
    },
    report::Settings,
};
use opcov_sys::{
    opcov_read_long, opcov_string_init, opcov_string_release, pcre_cache_entry,
    pcre_get_compiled_regex_cache, php_pcre_match_impl, php_pcre_pce_decref, php_pcre_pce_incref,
    zval,
};

use crate::{
    engine::{OffReason, Request, Verdict, with_engine},
    ini, zend,
};

const CASE: CaseSensitivity = if cfg!(target_os = "macos") {
    CaseSensitivity::Insensitive
} else {
    CaseSensitivity::Sensitive
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Pattern(NonNull<pcre_cache_entry>);

impl Pattern {
    fn compile(source: &str) -> Option<Self> {
        let executor = zend::executor_globals();
        // SAFETY: the executor globals are live during a request; error reporting is muted so that an
        // invalid pattern does not print at request startup, and restored right after.
        let error_reporting = unsafe { (*executor).error_reporting };
        // SAFETY: as above.
        unsafe { (*executor).error_reporting = 0 };
        // SAFETY: creates a request-scoped zend_string copy of `source`.
        let regex = unsafe { opcov_string_init(source.as_ptr().cast(), source.len(), false) };
        // SAFETY: `regex` is a live zend_string; PCRE copies what it caches.
        let entry = NonNull::new(unsafe { pcre_get_compiled_regex_cache(regex) });
        // SAFETY: releases the string created above.
        unsafe { opcov_string_release(regex) };
        // SAFETY: restores the value read above.
        unsafe { (*executor).error_reporting = error_reporting };
        return entry.map(|entry| {
            // SAFETY: keeps the cache entry alive until `release`, even if PCRE's cache evicts it.
            unsafe { php_pcre_pce_incref(entry.as_ptr()) };
            return Self(entry);
        });
    }

    fn matches(self, subject: &str) -> Option<bool> {
        // SAFETY: creates a request-scoped zend_string copy of `subject`.
        let string = unsafe { opcov_string_init(subject.as_ptr().cast(), subject.len(), false) };
        let mut result = zval::default();
        // SAFETY: the pattern entry is kept alive by its reference; without sub-patterns the result
        // is a long (or false on a matching error) and owns no memory.
        unsafe {
            php_pcre_match_impl(
                self.0.as_ptr(),
                string,
                &raw mut result,
                ptr::null_mut(),
                false,
                0,
                0,
            );
        };
        // SAFETY: releases the string created above.
        unsafe { opcov_string_release(string) };
        let mut count = 0;
        // SAFETY: `result` was initialised by php_pcre_match_impl.
        let is_long = unsafe { opcov_read_long(&raw const result, &raw mut count) };
        return is_long.then_some(count > 0);
    }

    pub(crate) fn release(self) {
        // SAFETY: drops the reference taken in `compile`.
        unsafe { php_pcre_pce_decref(self.0.as_ptr()) };
    }
}

pub(crate) fn configure() -> Request {
    let mut request = Request {
        selection: None,
        settings: None,
        pattern: None,
        decisions: BTreeMap::new(),
        problem: None,
    };
    let settings = [ini::COLLECT_PATHS, ini::EXCLUDE_PATHS, ini::EXCLUDE_PATTERN].map(ini::string);
    let [collect, exclude, pattern] = match settings {
        [Ok(collect), Ok(exclude), Ok(pattern)] => [collect, exclude, pattern],
        [Err(error), _, _] | [_, Err(error), _] | [_, _, Err(error)] => {
            request.problem = Some(OffReason::Selection(format!(
                "{} is not valid UTF-8",
                error.0.to_string_lossy()
            )));
            return request;
        }
    };
    let configured = SelectionConfig::parse(&collect, &exclude, |listfile| {
        return fs::read_to_string(listfile);
    })
    .and_then(|unresolved| {
        let settings = Settings {
            collect_paths: unresolved.collect_entries().iter().cloned().collect(),
            exclude_paths: unresolved.exclude_entries().iter().cloned().collect(),
            exclude_pattern: (!pattern.is_empty()).then(|| return pattern.clone()),
        };
        let selection = unresolved.resolve(|entry| return fs::canonicalize(entry), CASE)?;
        return Ok((selection, settings));
    });
    let (selection, settings) = match configured {
        Ok(configured) => configured,
        Err(error) => {
            request.problem = Some(OffReason::Selection(error.to_string()));
            return request;
        }
    };
    if selection.is_empty_collect() {
        request.problem = Some(OffReason::CollectPathsEmpty);
        return request;
    }
    if !pattern.is_empty() {
        let Some(compiled) = Pattern::compile(&pattern) else {
            request.problem = Some(OffReason::InvalidPattern(pattern));
            return request;
        };
        request.pattern = Some(compiled);
    }
    request.selection = Some(selection);
    request.settings = Some(settings);
    return request;
}

#[derive(Debug)]
enum Classified {
    Resolved { path: PathBuf, report: String },
    Eval { host: PathBuf, report: String },
    StreamWrapper,
    Unresolvable,
}

fn resolve(path: &str) -> Option<(PathBuf, String)> {
    let resolved = fs::canonicalize(path).ok()?;
    let text = resolved.to_str()?.to_owned();
    return Some((resolved, text));
}

fn classify(reported: &str) -> Classified {
    if let Some(host) = eval_host(reported) {
        return resolve(host).map_or(Classified::Unresolvable, |(host, _)| {
            return Classified::Eval {
                host,
                report: reported.to_owned(),
            };
        });
    }
    if stream_scheme(reported).is_some() {
        return Classified::StreamWrapper;
    }
    return resolve(reported).map_or(Classified::Unresolvable, |(path, report)| {
        return Classified::Resolved { path, report };
    });
}

pub(crate) fn decide(reported: &[u8]) -> Verdict {
    let Ok(name) = str::from_utf8(reported) else {
        return Verdict::Skip(SkipReason::Unresolvable);
    };
    let cached = with_engine(|engine| {
        let request = engine.request.as_ref()?;
        request.selection.as_ref()?;
        if let Some(verdict) = request.decisions.get(name) {
            return Some(Err(verdict.clone()));
        }
        return Some(Ok(request.pattern));
    })
    .flatten();
    let pattern = match cached {
        None => return Verdict::Off,
        Some(Err(verdict)) => return verdict,
        Some(Ok(pattern)) => pattern,
    };
    let classified = classify(name);
    let subject = match &classified {
        Classified::Resolved { path, .. } | Classified::Eval { host: path, .. } => {
            Some(path.as_path())
        }
        Classified::StreamWrapper | Classified::Unresolvable => None,
    };
    let matched = match (pattern, subject.and_then(Path::to_str)) {
        (Some(pattern), Some(subject)) => pattern.matches(subject).ok_or(subject),
        _ => Ok(false),
    };
    return with_engine(|engine| {
        let excluded = match matched {
            Ok(excluded) => excluded,
            Err(subject) => {
                engine.set_problem(OffReason::PatternFailed(subject.to_owned()));
                return Verdict::Off;
            }
        };
        let Some(selection) = engine
            .request
            .as_ref()
            .and_then(|request| return request.selection.as_ref())
        else {
            return Verdict::Off;
        };
        let compiled = match &classified {
            Classified::Resolved { path, .. } => CompiledFile::Resolved(path),
            Classified::Eval { host, .. } => CompiledFile::Eval { host },
            Classified::StreamWrapper => CompiledFile::StreamWrapper(name),
            Classified::Unresolvable => CompiledFile::Unresolvable(name),
        };
        let verdict = match selection.decide(compiled, excluded) {
            Decision::Skipped(reason) => Verdict::Skip(reason),
            Decision::Selected => match &classified {
                Classified::Resolved { report, .. } | Classified::Eval { report, .. } => {
                    Verdict::Measure(engine.files.intern(report))
                }
                Classified::StreamWrapper | Classified::Unresolvable => Verdict::Off,
            },
        };
        if let Some(request) = engine.request.as_mut() {
            request.decisions.insert(name.to_owned(), verdict.clone());
        }
        return verdict;
    })
    .unwrap_or(Verdict::Off);
}
