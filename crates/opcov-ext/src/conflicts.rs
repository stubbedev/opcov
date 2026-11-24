use core::fmt;
use std::{env, ffi::CStr};

use opcov_sys::{opcov_ini_bool, opcov_ini_long, opcov_module_loaded, opcov_sapi_name};
use serde_json::{Value, json};

use crate::{code::Code, ini};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Opcache {
    pub(crate) enable: bool,
    pub(crate) enable_cli: bool,
    pub(crate) protect_memory: bool,
    pub(crate) file_cache: String,
    pub(crate) jit: String,
    pub(crate) jit_buffer_size: String,
    pub(crate) jit_buffer_bytes: i64,
    pub(crate) preload: String,
    pub(crate) optimization_level: String,
    pub(crate) optimization_bits: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ModeSource {
    Environment,
    Ini,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct XdebugMode {
    pub(crate) mode: String,
    pub(crate) source: ModeSource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Environment {
    pub(crate) sapi: String,
    pub(crate) opcache: Option<Opcache>,
    pub(crate) xdebug: Option<XdebugMode>,
    pub(crate) pcov_enabled: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Conflict {
    OpcacheSharedSapi { sapi: String },
    OpcacheProtectMemory,
    OpcacheFileCache { directory: String },
    OpcacheJit { mode: String, buffer: String },
    OpcachePreload { script: String },
    OpcacheOptimizer { level: String },
    XdebugCoverage(XdebugMode),
    PcovEnabled,
}

impl Conflict {
    pub(crate) const fn code(&self) -> Code {
        return match self {
            Self::OpcacheSharedSapi { .. } => Code::OpcacheSharedSapi,
            Self::OpcacheProtectMemory => Code::OpcacheProtectMemory,
            Self::OpcacheFileCache { .. } => Code::OpcacheFileCache,
            Self::OpcacheJit { .. } => Code::OpcacheJit,
            Self::OpcachePreload { .. } => Code::OpcachePreload,
            Self::OpcacheOptimizer { .. } => Code::OpcacheOptimizer,
            Self::XdebugCoverage(_) => Code::XdebugCoverage,
            Self::PcovEnabled => Code::PcovEnabled,
        };
    }

    pub(crate) fn problem(&self) -> String {
        return match self {
            Self::OpcacheSharedSapi { sapi } => format!(
                "OPcache is enabled under the {sapi} SAPI, which shares the compiled code between processes, so a probe in one process would rewrite the code of all of them"
            ),
            Self::OpcacheProtectMemory => String::from(
                "opcache.protect_memory=1 makes the compiled code read-only, so probes cannot be installed",
            ),
            Self::OpcacheFileCache { directory } => format!(
                "opcache.file_cache={directory} stores the compiled code, probes included, on disk for other processes"
            ),
            Self::OpcacheJit { mode, buffer } => format!(
                "opcache.jit={mode} with opcache.jit_buffer_size={buffer} turns the JIT on, and JIT-compiled code never reads the opcode handlers that probes replace"
            ),
            Self::OpcachePreload { script } => format!(
                "opcache.preload={script} compiles code before opcov installs its hooks, so preloaded code gets no probes"
            ),
            Self::OpcacheOptimizer { level } => format!(
                "opcache.optimization_level={level} lets the OPcache optimizer remove and merge code, so lines and outcomes disappear from the report"
            ),
            Self::XdebugCoverage(XdebugMode { mode, source }) => format!(
                "Xdebug is loaded with mode \"{mode}\" (from {}), which includes coverage: two coverage tools in one process measure each other",
                match source {
                    ModeSource::Environment => "the XDEBUG_MODE environment variable",
                    ModeSource::Ini => "xdebug.mode",
                }
            ),
            Self::PcovEnabled => String::from(
                "PCOV is loaded with pcov.enabled=1: two coverage tools in one process measure each other, and PCOV's instruction loop calls every probe again",
            ),
        };
    }

    pub(crate) fn fix(&self) -> String {
        return match self {
            Self::OpcacheSharedSapi { .. } => {
                String::from("set opcache.enable=0, or collect coverage under the CLI SAPI")
            }
            Self::OpcacheProtectMemory => String::from("set opcache.protect_memory=0"),
            Self::OpcacheFileCache { .. } => {
                String::from("set opcache.file_cache= (empty) and opcache.file_cache_only=0")
            }
            Self::OpcacheJit { .. } => String::from("set opcache.jit=disable"),
            Self::OpcachePreload { .. } => String::from("set opcache.preload= (empty)"),
            Self::OpcacheOptimizer { .. } => String::from("set opcache.optimization_level=0"),
            Self::XdebugCoverage(XdebugMode { source, .. }) => match source {
                ModeSource::Environment => String::from(
                    "set XDEBUG_MODE to modes without coverage (for example XDEBUG_MODE=off), or do not load Xdebug",
                ),
                ModeSource::Ini => String::from(
                    "set xdebug.mode to modes without coverage (for example xdebug.mode=off), or do not load Xdebug",
                ),
            },
            Self::PcovEnabled => String::from("set pcov.enabled=0, or do not load PCOV"),
        };
    }

    pub(crate) fn to_json(&self) -> Value {
        return json!({
            "code": self.code().name(),
            "problem": self.problem(),
            "fix": self.fix(),
        });
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Conflicts {
    first: Conflict,
    rest: Vec<Conflict>,
}

impl Conflicts {
    pub(crate) fn of(mut found: Vec<Conflict>) -> Option<Self> {
        if found.is_empty() {
            return None;
        }
        let first = found.remove(0);
        return Some(Self { first, rest: found });
    }

    pub(crate) const fn first(&self) -> &Conflict {
        return &self.first;
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = &Conflict> {
        return core::iter::once(&self.first).chain(&self.rest);
    }
}

impl fmt::Display for Conflict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        return write!(f, "{}; {}", self.problem(), self.fix());
    }
}

fn shares_code_between_processes(sapi: &str) -> bool {
    return !matches!(sapi, "cli" | "phpdbg");
}

fn jit_number_is_valid(mut number: i64) -> bool {
    let digit = |value: i64| return value.rem_euclid(10);
    let optimization = digit(number);
    if optimization == 0 || optimization > 5 {
        return false;
    }
    number = number.div_euclid(10);
    let trigger = digit(number);
    if trigger > 5 || trigger == 4 {
        return false;
    }
    number = number.div_euclid(10);
    if digit(number) > 2 {
        return false;
    }
    number = number.div_euclid(10);
    if digit(number) > 1 {
        return false;
    }
    return number.div_euclid(10) == 0;
}

pub(crate) fn jit_mode_is_on(mode: &str) -> bool {
    let lower = mode.to_ascii_lowercase();
    return match lower.as_str() {
        "disable" | "" | "0" | "off" | "no" | "false" => false,
        "1" | "on" | "yes" | "true" | "tracing" | "function" => true,
        number => number
            .parse::<i64>()
            .is_ok_and(|value| return value > 0 && jit_number_is_valid(value)),
    };
}

fn has_coverage(mode: &str) -> bool {
    return mode.split(',').any(|item| return item.trim() == "coverage");
}

impl Opcache {
    pub(crate) fn active_under(&self, sapi: &str) -> bool {
        return self.enable && (shares_code_between_processes(sapi) || self.enable_cli);
    }

    fn conflicts(&self, sapi: &str, found: &mut Vec<Conflict>) {
        if !self.active_under(sapi) {
            return;
        }
        if shares_code_between_processes(sapi) {
            found.push(Conflict::OpcacheSharedSapi {
                sapi: sapi.to_owned(),
            });
            return;
        }
        if self.protect_memory {
            found.push(Conflict::OpcacheProtectMemory);
        }
        if !self.file_cache.is_empty() {
            found.push(Conflict::OpcacheFileCache {
                directory: self.file_cache.clone(),
            });
        }
        if self.jit_buffer_bytes > 0 && jit_mode_is_on(&self.jit) {
            found.push(Conflict::OpcacheJit {
                mode: self.jit.clone(),
                buffer: self.jit_buffer_size.clone(),
            });
        }
        if !self.preload.is_empty() {
            found.push(Conflict::OpcachePreload {
                script: self.preload.clone(),
            });
        }
        if self.optimization_bits != 0 {
            found.push(Conflict::OpcacheOptimizer {
                level: self.optimization_level.clone(),
            });
        }
    }
}

impl Environment {
    pub(crate) fn conflicts(&self) -> Vec<Conflict> {
        let mut found = Vec::new();
        if let Some(opcache) = &self.opcache {
            opcache.conflicts(&self.sapi, &mut found);
        }
        if let Some(xdebug) = self
            .xdebug
            .as_ref()
            .filter(|xdebug| return has_coverage(&xdebug.mode))
        {
            found.push(Conflict::XdebugCoverage(xdebug.clone()));
        }
        if self.pcov_enabled == Some(true) {
            found.push(Conflict::PcovEnabled);
        }
        return found;
    }

    pub(crate) fn refresh(&mut self) {
        if let Some(opcache) = &mut self.opcache {
            opcache.refresh(&self.sapi);
        }
    }

    pub(crate) fn current() -> Self {
        return Self {
            sapi: sapi_name(),
            opcache: loaded(c"zend opcache").then(Opcache::current),
            xdebug: loaded(c"xdebug").then(xdebug_mode),
            pcov_enabled: loaded(c"pcov").then(|| return flag(c"pcov.enabled")),
        };
    }
}

fn sapi_name() -> String {
    // SAFETY: the SAPI module is the process-wide, static description of the running SAPI; its name
    // is a NUL-terminated string that lives as long as the process.
    let name = unsafe { opcov_sapi_name() };
    if name.is_null() {
        return String::new();
    }
    // SAFETY: as above.
    return unsafe { CStr::from_ptr(name) }
        .to_string_lossy()
        .into_owned();
}

fn loaded(module: &CStr) -> bool {
    // SAFETY: looks a lower-case module name up in the engine's module registry.
    return unsafe { opcov_module_loaded(module.as_ptr(), module.count_bytes()) };
}

fn flag(name: &CStr) -> bool {
    // SAFETY: reads an INI entry by its NUL-terminated name and length; a missing entry is false.
    return unsafe { opcov_ini_bool(name.as_ptr(), name.count_bytes()) };
}

fn number(name: &CStr) -> i64 {
    // SAFETY: reads an INI entry by its NUL-terminated name and length; a missing entry is 0.
    return unsafe { opcov_ini_long(name.as_ptr(), name.count_bytes()) };
}

fn not_utf8(name: &CStr) -> String {
    return format!("<{} is not valid UTF-8>", name.to_string_lossy());
}

fn text(name: &'static CStr) -> String {
    return ini::string(name).unwrap_or_else(|_not_utf8| return not_utf8(name));
}

fn refresh_text(name: &'static CStr, current: &mut String) {
    if ini::refresh_string(name, current).is_err() {
        *current = not_utf8(name);
    }
}

impl Opcache {
    fn refresh(&mut self, sapi: &str) {
        if !self.active_under(sapi) {
            return;
        }
        self.enable = flag(c"opcache.enable");
        if self.jit_buffer_bytes > 0 {
            refresh_text(c"opcache.jit", &mut self.jit);
        }
    }

    fn current() -> Self {
        return Self {
            enable: flag(c"opcache.enable"),
            enable_cli: flag(c"opcache.enable_cli"),
            protect_memory: flag(c"opcache.protect_memory"),
            file_cache: text(c"opcache.file_cache"),
            jit: text(c"opcache.jit"),
            jit_buffer_size: text(c"opcache.jit_buffer_size"),
            jit_buffer_bytes: number(c"opcache.jit_buffer_size"),
            preload: text(c"opcache.preload"),
            optimization_level: text(c"opcache.optimization_level"),
            optimization_bits: number(c"opcache.optimization_level"),
        };
    }

    pub(crate) fn to_json(&self, sapi: &str) -> Value {
        return json!({
            "active": self.active_under(sapi),
            "enable": self.enable,
            "enable_cli": self.enable_cli,
            "protect_memory": self.protect_memory,
            "file_cache": self.file_cache,
            "jit": self.jit,
            "jit_buffer_size": self.jit_buffer_size,
            "preload": self.preload,
            "optimization_level": self.optimization_level,
        });
    }
}

fn xdebug_mode() -> XdebugMode {
    if let Some(mode) = env::var("XDEBUG_MODE")
        .ok()
        .filter(|mode| return !mode.is_empty())
    {
        return XdebugMode {
            mode,
            source: ModeSource::Environment,
        };
    }
    return XdebugMode {
        mode: text(c"xdebug.mode"),
        source: ModeSource::Ini,
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cli_opcache() -> Opcache {
        return Opcache {
            enable: true,
            enable_cli: true,
            protect_memory: false,
            file_cache: String::new(),
            jit: String::from("disable"),
            jit_buffer_size: String::from("64M"),
            jit_buffer_bytes: 64,
            preload: String::new(),
            optimization_level: String::from("0"),
            optimization_bits: 0,
        };
    }

    fn environment(sapi: &str, opcache: Option<Opcache>) -> Environment {
        return Environment {
            sapi: sapi.to_owned(),
            opcache,
            xdebug: None,
            pcov_enabled: None,
        };
    }

    fn codes(environment: &Environment) -> Vec<Code> {
        return environment.conflicts().iter().map(Conflict::code).collect();
    }

    #[test]
    fn cli_opcache_without_optimizer_jit_file_cache_or_preload_is_supported() {
        assert_eq!(codes(&environment("cli", Some(cli_opcache()))), []);
        assert_eq!(codes(&environment("phpdbg", Some(cli_opcache()))), []);
        assert_eq!(codes(&environment("cli", None)), []);
    }

    #[test]
    fn opcache_disabled_for_the_cli_conflicts_with_nothing() {
        let opcache = Opcache {
            enable_cli: false,
            protect_memory: true,
            file_cache: String::from("/tmp/cache"),
            jit: String::from("tracing"),
            optimization_bits: 1,
            ..cli_opcache()
        };
        assert_eq!(codes(&environment("cli", Some(opcache))), []);
    }

    #[test]
    fn opcache_under_a_sharing_sapi_is_refused_whatever_enable_cli_says() {
        let opcache = Opcache {
            enable_cli: false,
            ..cli_opcache()
        };
        assert_eq!(
            codes(&environment("fpm-fcgi", Some(opcache.clone()))),
            [Code::OpcacheSharedSapi]
        );
        let disabled = Opcache {
            enable: false,
            ..opcache
        };
        assert_eq!(codes(&environment("cgi-fcgi", Some(disabled))), []);
    }

    #[test]
    fn every_unsupported_cli_setting_is_reported_with_its_fix() {
        let opcache = Opcache {
            protect_memory: true,
            file_cache: String::from("/tmp/cache"),
            jit: String::from("tracing"),
            preload: String::from("/app/preload.php"),
            optimization_level: String::from("0x7FFEBFFF"),
            optimization_bits: 0x7FFE_BFFF,
            ..cli_opcache()
        };
        let found = environment("cli", Some(opcache)).conflicts();
        let text: Vec<String> = found.iter().map(ToString::to_string).collect();
        assert_eq!(
            text,
            [
                "opcache.protect_memory=1 makes the compiled code read-only, so probes cannot be installed; set opcache.protect_memory=0",
                "opcache.file_cache=/tmp/cache stores the compiled code, probes included, on disk for other processes; set opcache.file_cache= (empty) and opcache.file_cache_only=0",
                "opcache.jit=tracing with opcache.jit_buffer_size=64M turns the JIT on, and JIT-compiled code never reads the opcode handlers that probes replace; set opcache.jit=disable",
                "opcache.preload=/app/preload.php compiles code before opcov installs its hooks, so preloaded code gets no probes; set opcache.preload= (empty)",
                "opcache.optimization_level=0x7FFEBFFF lets the OPcache optimizer remove and merge code, so lines and outcomes disappear from the report; set opcache.optimization_level=0",
            ]
        );
    }

    #[test]
    fn jit_modes_follow_opcache_parsing() {
        for on in [
            "tracing", "on", "ON", "1", "yes", "true", "function", "1254", "1205", "0001",
        ] {
            assert!(jit_mode_is_on(on), "{on} turns the JIT on");
        }
        for off in [
            "disable", "off", "0", "", "no", "false", "1250", "1264", "1245", "3255", "21255",
            "-1255", "fast",
        ] {
            assert!(!jit_mode_is_on(off), "{off} leaves the JIT off");
        }
    }

    #[test]
    fn the_jit_is_off_without_a_buffer() {
        let opcache = Opcache {
            jit: String::from("tracing"),
            jit_buffer_size: String::from("0"),
            jit_buffer_bytes: 0,
            ..cli_opcache()
        };
        assert_eq!(codes(&environment("cli", Some(opcache))), []);
    }

    #[test]
    fn coverage_tools_conflict_only_in_their_coverage_modes() {
        let mut checked = environment("cli", None);
        checked.xdebug = Some(XdebugMode {
            mode: String::from("develop,debug"),
            source: ModeSource::Ini,
        });
        checked.pcov_enabled = Some(false);
        assert_eq!(codes(&checked), []);
        checked.xdebug = Some(XdebugMode {
            mode: String::from("develop, coverage"),
            source: ModeSource::Environment,
        });
        checked.pcov_enabled = Some(true);
        assert_eq!(codes(&checked), [Code::XdebugCoverage, Code::PcovEnabled]);
        assert!(
            checked.conflicts()[0].fix().contains("XDEBUG_MODE"),
            "the fix names the environment variable the mode came from"
        );
    }
}
