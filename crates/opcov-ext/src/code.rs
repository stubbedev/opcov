use core::fmt;

pub(crate) const TROUBLESHOOTING: &str = concat!(
    env!("CARGO_PKG_REPOSITORY"),
    "/blob/main/docs/troubleshooting.md"
);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum Code {
    Panic,
    SecondThread,
    IniRegistrationFailed,
    NotEnabledAtStartup,
    NoReservedSlot,
    NotEnabled,
    CompileHooksMissing,
    InvalidSelection,
    CollectPathsEmpty,
    InvalidExcludePattern,
    ExcludePatternFailed,
    SnapshotFailed,
    NothingSelected,
    OpcacheSharedSapi,
    OpcacheProtectMemory,
    OpcacheFileCache,
    OpcacheJit,
    OpcachePreload,
    OpcacheOptimizer,
    XdebugCoverage,
    PcovEnabled,
}

impl Code {
    #[cfg(test)]
    pub(crate) const ALL: [Self; 21] = [
        Self::Panic,
        Self::SecondThread,
        Self::IniRegistrationFailed,
        Self::NotEnabledAtStartup,
        Self::NoReservedSlot,
        Self::NotEnabled,
        Self::CompileHooksMissing,
        Self::InvalidSelection,
        Self::CollectPathsEmpty,
        Self::InvalidExcludePattern,
        Self::ExcludePatternFailed,
        Self::SnapshotFailed,
        Self::NothingSelected,
        Self::OpcacheSharedSapi,
        Self::OpcacheProtectMemory,
        Self::OpcacheFileCache,
        Self::OpcacheJit,
        Self::OpcachePreload,
        Self::OpcacheOptimizer,
        Self::XdebugCoverage,
        Self::PcovEnabled,
    ];

    pub(crate) const fn name(self) -> &'static str {
        return match self {
            Self::Panic => "panic",
            Self::SecondThread => "second-thread",
            Self::IniRegistrationFailed => "ini-registration-failed",
            Self::NotEnabledAtStartup => "not-enabled-at-startup",
            Self::NoReservedSlot => "no-reserved-slot",
            Self::NotEnabled => "not-enabled",
            Self::CompileHooksMissing => "compile-hooks-missing",
            Self::InvalidSelection => "invalid-selection",
            Self::CollectPathsEmpty => "collect-paths-empty",
            Self::InvalidExcludePattern => "invalid-exclude-pattern",
            Self::ExcludePatternFailed => "exclude-pattern-failed",
            Self::SnapshotFailed => "snapshot-failed",
            Self::NothingSelected => "nothing-selected",
            Self::OpcacheSharedSapi => "opcache-shared-sapi",
            Self::OpcacheProtectMemory => "opcache-protect-memory",
            Self::OpcacheFileCache => "opcache-file-cache",
            Self::OpcacheJit => "opcache-jit",
            Self::OpcachePreload => "opcache-preload",
            Self::OpcacheOptimizer => "opcache-optimizer",
            Self::XdebugCoverage => "xdebug-coverage",
            Self::PcovEnabled => "pcov-enabled",
        };
    }

    pub(crate) fn link(self) -> String {
        return format!("{TROUBLESHOOTING}#{}", self.name());
    }
}

impl fmt::Display for Code {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        return f.write_str(self.name());
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Explained<'a, T> {
    pub(crate) code: Code,
    pub(crate) text: &'a T,
}

impl<T: fmt::Display> fmt::Display for Explained<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        return write!(f, "{} (see {})", self.text, self.code.link());
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeSet, fs, path::Path};

    use super::*;

    fn troubleshooting() -> String {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/troubleshooting.md");
        return fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
    }

    fn documented_codes(page: &str) -> BTreeSet<String> {
        return page
            .lines()
            .filter_map(|line| return line.strip_prefix("## "))
            .map(|heading| return heading.trim().to_owned())
            .collect();
    }

    #[test]
    fn every_code_has_a_troubleshooting_entry_and_every_entry_a_code() {
        let documented = documented_codes(&troubleshooting());
        let codes: BTreeSet<String> = Code::ALL
            .iter()
            .map(|code| return code.name().to_owned())
            .collect();
        let undocumented: Vec<&String> = codes.difference(&documented).collect();
        let stale: Vec<&String> = documented.difference(&codes).collect();
        assert!(
            undocumented.is_empty(),
            "codes without an entry in docs/troubleshooting.md: {undocumented:?}"
        );
        assert!(
            stale.is_empty(),
            "docs/troubleshooting.md entries no code uses: {stale:?}"
        );
    }

    #[test]
    fn every_code_name_is_its_own_github_anchor() {
        for code in Code::ALL {
            let name = code.name();
            assert!(
                name.bytes()
                    .all(|byte| return byte.is_ascii_lowercase() || byte == b'-'),
                "{name} must be lowercase words joined by hyphens, so that the heading's anchor is the name itself"
            );
        }
    }

    #[test]
    fn every_refusal_goes_through_a_code() {
        let sources = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut throws = Vec::new();
        for entry in fs::read_dir(&sources).unwrap() {
            let path = entry.unwrap().path();
            if path
                .file_name()
                .is_some_and(|name| return name == "code.rs")
            {
                continue;
            }
            let text = fs::read_to_string(&path).unwrap();
            let count = text.matches("Exception::CannotCollect").count();
            if count > 0 {
                throws.push((
                    path.file_name().unwrap().to_string_lossy().into_owned(),
                    count,
                ));
            }
        }
        throws.sort();
        assert_eq!(
            throws,
            vec![(String::from("api.rs"), 1), (String::from("php.rs"), 1)],
            "CannotCollectException is thrown in exactly one place, api::refuse, which links the reason's troubleshooting entry"
        );
    }

    #[test]
    fn a_link_names_the_entry() {
        assert_eq!(
            Code::OpcacheJit.link(),
            "https://github.com/stubbedev/opcov/blob/main/docs/troubleshooting.md#opcache-jit"
        );
    }
}
