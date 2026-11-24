use core::ptr::{self, NonNull};
use std::{
    collections::BTreeMap,
    ffi::OsStr,
    fs, io,
    os::unix::ffi::OsStrExt as _,
    path::{Path, PathBuf},
};

use opcov_core::{
    cfg::FilePath,
    encoding,
    filter::{
        CaseSensitivity, CompiledFile, Decision, PatternMatch, SelectionConfig, eval_host,
        stream_scheme,
    },
    report::Settings,
};
use opcov_sys::{
    opcov_pcre_compile, opcov_pcre_match, pcre_cache_entry, php_pcre_pce_decref,
    php_pcre_pce_incref,
};

use crate::{
    engine::{Map, OffReason, Request, Verdict, with_engine},
    guard::caught,
    ini, zend,
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
        let mut entry: *mut pcre_cache_entry = ptr::null_mut();
        // SAFETY: PCRE compiles a request-scoped copy of `source` and caches what it keeps; a fatal
        // error on the way is caught inside the call.
        let completed =
            unsafe { opcov_pcre_compile(source.as_ptr().cast(), source.len(), &raw mut entry) };
        // SAFETY: restores the value read above.
        unsafe { (*executor).error_reporting = error_reporting };
        if caught(completed).is_err() {
            return None;
        }
        return NonNull::new(entry).map(|entry| {
            // SAFETY: keeps the cache entry alive until `release`, even if PCRE's cache evicts it.
            unsafe { php_pcre_pce_incref(entry.as_ptr()) };
            return Self(entry);
        });
    }

    fn matches(self, subject: &[u8]) -> PatternMatch {
        let mut matched = -1;
        // SAFETY: the pattern entry is kept alive by its reference; the engine matches a
        // request-scoped copy of `subject` and writes -1 for a matching error.
        let completed = unsafe {
            opcov_pcre_match(
                self.0.as_ptr(),
                subject.as_ptr().cast(),
                subject.len(),
                &raw mut matched,
            )
        };
        if caught(completed).is_err() {
            return PatternMatch::Failed;
        }
        return match matched {
            0 => PatternMatch::Unmatched,
            1 => PatternMatch::Matched,
            _ => PatternMatch::Failed,
        };
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
        directories: Directories::default(),
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
        let selection = unresolved.resolve(
            |entry| return fs::canonicalize(entry),
            CaseSensitivity::HOST,
        )?;
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
    Resolved { path: PathBuf, report: FilePath },
    Eval { host: PathBuf, report: FilePath },
    StreamWrapper,
    Unresolvable,
}

#[derive(Debug, Default)]
pub(crate) struct Directories(Map<PathBuf, PathBuf>);

impl Directories {
    fn split(path: &Path) -> Option<(&Path, &OsStr)> {
        let bytes = path.as_os_str().as_bytes();
        let slash = bytes.iter().rposition(|&byte| return byte == b'/')?;
        let (parent, name) = bytes.split_at(slash);
        let name = name.get(1..)?;
        let usable = CaseSensitivity::HOST == CaseSensitivity::Sensitive
            && bytes.starts_with(b"/")
            && !matches!(name, b"" | b"." | b"..");
        let parent = if parent.is_empty() { b"/" } else { parent };
        return usable.then(|| {
            return (
                Path::new(OsStr::from_bytes(parent)),
                OsStr::from_bytes(name),
            );
        });
    }

    fn canonicalize(&mut self, path: &Path) -> io::Result<PathBuf> {
        let Some((parent, name)) = Self::split(path) else {
            return fs::canonicalize(path);
        };
        let candidate = if let Some(known) = self.0.get(parent) {
            known.join(name)
        } else {
            let known = fs::canonicalize(parent)?;
            let joined = known.join(name);
            self.0.insert(parent.to_path_buf(), known);
            joined
        };
        if fs::symlink_metadata(&candidate)?.file_type().is_symlink() {
            return fs::canonicalize(&candidate);
        }
        return Ok(candidate);
    }

    fn resolve(&mut self, path: &str) -> Option<(PathBuf, FilePath)> {
        let bytes = encoding::decode(path).ok()?;
        let resolved = self
            .canonicalize(Path::new(OsStr::from_bytes(&bytes)))
            .ok()?;
        let report = FilePath::from_bytes(resolved.as_os_str().as_bytes()).ok()?;
        return Some((resolved, report));
    }
}

fn classify(reported: &str, directories: &mut Directories) -> Classified {
    if let Some(host) = eval_host(reported) {
        let resolved = directories.resolve(host).zip(reported.parse().ok());
        return resolved.map_or(Classified::Unresolvable, |((host, _), report)| {
            return Classified::Eval { host, report };
        });
    }
    if stream_scheme(reported).is_some() {
        return Classified::StreamWrapper;
    }
    return directories
        .resolve(reported)
        .map_or(Classified::Unresolvable, |(path, report)| {
            return Classified::Resolved { path, report };
        });
}

pub(crate) fn decide(reported: &[u8]) -> Verdict {
    let encoded = encoding::encode(reported);
    let name: &str = &encoded;
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
    let Some(classified) = with_engine(|engine| {
        return engine
            .request
            .as_mut()
            .map(|request| return classify(name, &mut request.directories));
    })
    .flatten() else {
        return Verdict::Off;
    };
    let subject = match &classified {
        Classified::Resolved { path, .. } | Classified::Eval { host: path, .. } => {
            Some(path.as_path())
        }
        Classified::StreamWrapper | Classified::Unresolvable => None,
    };
    let matched = match (pattern, subject) {
        (Some(pattern), Some(subject)) => pattern.matches(subject.as_os_str().as_bytes()),
        _ => PatternMatch::Unmatched,
    };
    return with_engine(|engine| {
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
        let verdict = match selection.decide(compiled, matched) {
            Decision::Skipped(reason) => Verdict::Skip(reason),
            Decision::Selected => match &classified {
                Classified::Resolved { report, .. } | Classified::Eval { report, .. } => engine
                    .files
                    .intern(report.clone())
                    .map_or(Verdict::Off, Verdict::Measure),
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

#[cfg(test)]
mod tests {
    use std::os::unix::fs::symlink;

    use super::*;

    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let root =
                std::env::temp_dir().join(format!("opcov-select-{name}-{}", std::process::id()));
            fs::create_dir_all(root.join("real/src")).unwrap();
            fs::write(root.join("real/src/a.php"), "<?php\n").unwrap();
            symlink(root.join("real"), root.join("link")).unwrap();
            symlink(root.join("real/src/a.php"), root.join("real/src/alias.php")).unwrap();
            return Self(fs::canonicalize(root).unwrap());
        }

        fn path(&self, relative: &str) -> String {
            return self.0.join(relative).to_str().unwrap().to_owned();
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }

    #[test]
    fn resolution_matches_canonicalize_through_directory_links_and_dot_dot() {
        let scratch = Scratch::new("links");
        let mut directories = Directories::default();
        for relative in [
            "link/src/a.php",
            "real/src/../src/a.php",
            "link/src/alias.php",
            "real/src/a.php",
        ] {
            let path = scratch.path(relative);
            assert_eq!(
                directories.canonicalize(Path::new(&path)).unwrap(),
                fs::canonicalize(&path).unwrap(),
                "{relative}"
            );
        }
        assert_eq!(
            directories
                .canonicalize(Path::new(&scratch.path("link/src/alias.php")))
                .unwrap(),
            scratch.0.join("real/src/a.php"),
            "a symlinked file resolves to its target"
        );
    }

    #[test]
    fn a_missing_file_does_not_resolve_even_in_a_known_directory() {
        let scratch = Scratch::new("missing");
        let mut directories = Directories::default();
        directories
            .canonicalize(Path::new(&scratch.path("real/src/a.php")))
            .unwrap();
        for missing in ["real/src/gone.php", "absent/a.php"] {
            let error = directories
                .canonicalize(Path::new(&scratch.path(missing)))
                .unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::NotFound, "{missing}");
        }
    }
}
