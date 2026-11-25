use std::fs::{self, DirBuilder, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{self, Child, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use opcov_core::cfg::PhpVersion;
use serde::{Deserialize, Serialize};

use crate::failure::{Failure, Verdict};

const TROUBLESHOOTING: &str = concat!(
    env!("CARGO_PKG_REPOSITORY"),
    "/blob/main/docs/troubleshooting.md"
);
const CLI_VERSION: &str = env!("CARGO_PKG_VERSION");
const DEADLINE: Duration = Duration::from_secs(60);
const POLL: Duration = Duration::from_millis(20);

const PROBE_SOURCE: &str = "<?php
function opcov_doctor_probe(bool $taken): string
{
    if ($taken) {
        return 'taken';
    }
    return 'not taken';
}
";
const PROBE_COVERED: [u32; 2] = [4, 5];
const PROBE_EXECUTABLE: [u32; 3] = [4, 5, 7];

const SELFTEST_SCRIPT: &str = r"<?php
$result = [
    'loaded' => extension_loaded('opcov'),
    'php' => PHP_VERSION,
    'zts' => ZEND_THREAD_SAFE,
    'sapi' => PHP_SAPI,
    'ini_file' => php_ini_loaded_file() ?: null,
    'scanned_files' => php_ini_scanned_files() ?: null,
];
if ($result['loaded']) {
    $result['version'] = \opcov\version();
    try {
        \opcov\ensure_can_collect();
        \opcov\start();
        require __DIR__ . '/probe.php';
        opcov_doctor_probe(true);
        $hits = \opcov\stop();
        \opcov\reset();
        $file = __DIR__ . '/probe.php';
        $function = $hits[$file]['functions']['opcov_doctor_probe'] ?? [];
        $result['probe'] = [
            'covered' => array_keys($hits[$file]['lines'] ?? []),
            'executable' => array_keys(\opcov\executable([$file])[$file]['lines'] ?? []),
            'outcomes' => count($function['outcomes'] ?? []),
        ];
    } catch (\Throwable $e) {
        $result['refusal'] = get_class($e) . ': ' . $e->getMessage();
    }
    $result['status'] = \opcov\get_status();
}
file_put_contents($argv[1], json_encode($result, JSON_THROW_ON_ERROR));
";

#[derive(Debug, Deserialize)]
struct Conflict {
    code: String,
    problem: String,
    fix: String,
}

#[derive(Debug, Deserialize)]
struct Status {
    off_reason: Option<String>,
    off_code: Option<String>,
    vm_kind: String,
    conflicts: Vec<Conflict>,
}

#[derive(Debug, Deserialize)]
struct ProbeRun {
    covered: Vec<u32>,
    executable: Vec<u32>,
    outcomes: usize,
}

#[derive(Debug, Deserialize)]
struct ChildReport {
    loaded: bool,
    php: String,
    zts: bool,
    sapi: String,
    ini_file: Option<String>,
    scanned_files: Option<String>,
    version: Option<String>,
    refusal: Option<String>,
    probe: Option<ProbeRun>,
    status: Option<Status>,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub(crate) struct Finding {
    code: String,
    problem: String,
    fix: String,
    link: String,
}

impl Finding {
    fn new(code: &str, problem: String, fix: String) -> Self {
        return Self {
            code: code.to_owned(),
            problem,
            fix,
            link: format!("{TROUBLESHOOTING}#{code}"),
        };
    }
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum SelfTest {
    Passed,
    NotRun,
    Failed,
}

#[derive(Debug, Serialize)]
struct Runtime {
    php: String,
    zts: bool,
    sapi: String,
    opcov: Option<String>,
    vm_kind: Option<String>,
    ini_file: Option<String>,
    scanned_files: Vec<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct Checkup {
    php_binary: String,
    runtime: Option<Runtime>,
    self_test: SelfTest,
    findings: Vec<Finding>,
    startup_output: String,
}

impl Checkup {
    pub(crate) const fn verdict(&self) -> Verdict {
        return if self.findings.is_empty() {
            Verdict::Clean
        } else {
            Verdict::Findings
        };
    }
}

fn php_minor(api: u32) -> String {
    return PhpVersion::from_module_api(api).map_or_else(
        || return format!("the PHP with module API {api}"),
        |version| return format!("PHP {version}"),
    );
}

fn compiled_value<'a>(output: &'a str, prefix: &str, key: &str) -> Option<&'a str> {
    return output.lines().find_map(|line| {
        let rest = line.trim_start().strip_prefix(prefix)?;
        return rest.trim_start().strip_prefix(key).map(str::trim);
    });
}

fn build_flavour(build: &str) -> String {
    let threads = if build.split(',').any(|part| return part == "TS") {
        "ZTS (thread-safe)"
    } else {
        "NTS (non-thread-safe)"
    };
    let debug = if build.split(',').any(|part| return part == "debug") {
        " debug"
    } else {
        ""
    };
    return format!("{threads}{debug}");
}

const NTS_ONLY_SYMBOLS: [&str; 2] = ["executor_globals", "compiler_globals"];
const ZTS_ONLY_SYMBOLS: [&str; 3] = [
    "tsrm_get_ls_cache",
    "executor_globals_offset",
    "compiler_globals_offset",
];

fn missing_symbol(output: &str) -> Option<&str> {
    let line = output.lines().find(|line| {
        return line.contains("Unable to load dynamic library") && line.contains("opcov");
    })?;
    let (_, rest) = line.split_once("undefined symbol: ")?;
    return rest
        .split(|character: char| return character == ')' || character.is_whitespace())
        .next();
}

fn thread_safety_mismatch(output: &str) -> Option<Finding> {
    let symbol = missing_symbol(output)?;
    let (built, running) = if NTS_ONLY_SYMBOLS.contains(&symbol) {
        ("NTS (non-thread-safe)", "ZTS (thread-safe)")
    } else if ZTS_ONLY_SYMBOLS.contains(&symbol) {
        ("ZTS (thread-safe)", "NTS (non-thread-safe)")
    } else {
        return None;
    };
    return Some(Finding::new(
        "build-mismatch",
        format!(
            "the opcov extension is a {built} build, but this PHP is {running}, so loading it fails on the missing symbol {symbol}"
        ),
        format!("install the {running} build of opcov for this PHP"),
    ));
}

fn build_mismatch(output: &str) -> Option<Finding> {
    if !output.contains("opcov: Unable to initialize module") {
        return thread_safety_mismatch(output);
    }
    let module_api = compiled_value(output, "Module compiled with", "module API=");
    let php_api = compiled_value(output, "PHP    compiled with", "module API=")
        .or_else(|| return compiled_value(output, "PHP compiled with", "module API="));
    if let (Some(module), Some(php)) = (
        module_api.and_then(|value| return value.parse::<u32>().ok()),
        php_api.and_then(|value| return value.parse::<u32>().ok()),
    ) {
        return Some(Finding::new(
            "build-mismatch",
            format!(
                "the opcov extension was built for {} (module API {module}), but this PHP is {} (module API {php}), so PHP refuses to load it",
                php_minor(module),
                php_minor(php)
            ),
            format!("install the opcov build for {}", php_minor(php)),
        ));
    }
    let module_build = compiled_value(output, "Module compiled with", "build ID=")?;
    let php_build = compiled_value(output, "PHP    compiled with", "build ID=")
        .or_else(|| return compiled_value(output, "PHP compiled with", "build ID="))?;
    return Some(Finding::new(
        "build-mismatch",
        format!(
            "the opcov extension is a {} build (build ID {module_build}), but this PHP is {} (build ID {php_build}), so PHP refuses to load it",
            build_flavour(module_build),
            build_flavour(php_build)
        ),
        format!(
            "install the {} build of opcov for this PHP",
            build_flavour(php_build)
        ),
    ));
}

fn compatibility_line(version: &str) -> Option<(u64, Option<u64>)> {
    let mut parts = version.split('.').map(str::parse::<u64>);
    let major = parts.next()?.ok()?;
    let minor = parts.next()?.ok()?;
    return Some((major, (major == 0).then_some(minor)));
}

fn version_mismatch(extension: &str) -> Option<Finding> {
    if compatibility_line(extension).is_some()
        && compatibility_line(extension) == compatibility_line(CLI_VERSION)
    {
        return None;
    }
    return Some(Finding::new(
        "version-mismatch",
        format!(
            "the opcov extension is version {extension}, but this opcov CLI is version {CLI_VERSION}; reports and settings are only guaranteed to agree within one compatibility line (the same major version, or the same minor version before 1.0)"
        ),
        format!("install the extension and the CLI from the same opcov release ({CLI_VERSION})"),
    ));
}

fn without_link(reason: &str) -> &str {
    return reason
        .rsplit_once(" (see https://")
        .map_or(reason, |(text, _)| return text);
}

fn status_findings(status: &Status, findings: &mut Vec<Finding>) {
    for conflict in &status.conflicts {
        findings.push(Finding::new(
            &conflict.code,
            conflict.problem.clone(),
            conflict.fix.clone(),
        ));
    }
    let (Some(reason), Some(code)) = (&status.off_reason, &status.off_code) else {
        return;
    };
    if status
        .conflicts
        .iter()
        .any(|conflict| return &conflict.code == code)
    {
        return;
    }
    findings.push(Finding::new(
        code,
        without_link(reason).to_owned(),
        String::from("follow the troubleshooting entry"),
    ));
}

fn probe_verdict(report: &ChildReport, findings: &mut Vec<Finding>) -> SelfTest {
    if let Some(probe) = &report.probe {
        if probe.covered == PROBE_COVERED
            && probe.executable == PROBE_EXECUTABLE
            && probe.outcomes == 1
        {
            return SelfTest::Passed;
        }
        findings.push(Finding::new(
            "self-test-failed",
            format!(
                "the probe self-test covered lines {:?} of {:?} with {} outcome(s), expected lines {PROBE_COVERED:?} of {PROBE_EXECUTABLE:?} with 1 outcome",
                probe.covered, probe.executable, probe.outcomes
            ),
            String::from("file a bug with this output and `opcov\\get_debug_info()` of the probe file"),
        ));
        return SelfTest::Failed;
    }
    let explained = !findings.is_empty();
    if let Some(refusal) = report.refusal.as_ref().filter(|_| return !explained) {
        findings.push(Finding::new(
            "self-test-failed",
            format!("the probe self-test failed: {}", without_link(refusal)),
            String::from("file a bug with this output"),
        ));
        return SelfTest::Failed;
    }
    return SelfTest::NotRun;
}

fn assess(php_binary: &Path, report: Option<ChildReport>, startup_output: String) -> Checkup {
    let mut findings = Vec::new();
    let Some(report) = report.filter(|report| return report.loaded) else {
        findings.push(build_mismatch(&startup_output).unwrap_or_else(|| {
            return Finding::new(
                "extension-not-loaded",
                format!("{} does not load the opcov extension", php_binary.display()),
                String::from(
                    "add extension=opcov (or the full path of opcov.so) to an INI file this PHP reads; `php --ini` lists them",
                ),
            );
        }));
        return Checkup {
            php_binary: php_binary.display().to_string(),
            runtime: None,
            self_test: SelfTest::NotRun,
            findings,
            startup_output,
        };
    };
    if let Some(mismatch) = report.version.as_deref().and_then(version_mismatch) {
        findings.push(mismatch);
    }
    if let Some(status) = &report.status {
        status_findings(status, &mut findings);
    }
    let self_test = probe_verdict(&report, &mut findings);
    return Checkup {
        php_binary: php_binary.display().to_string(),
        runtime: Some(Runtime {
            php: report.php,
            zts: report.zts,
            sapi: report.sapi,
            opcov: report.version,
            vm_kind: report.status.map(|status| return status.vm_kind),
            ini_file: report.ini_file,
            scanned_files: report.scanned_files.map_or_else(Vec::new, |files| {
                return files
                    .split(',')
                    .map(|file| return file.trim().to_owned())
                    .filter(|file| return !file.is_empty())
                    .collect();
            }),
        }),
        self_test,
        findings,
        startup_output,
    };
}

struct Workspace(PathBuf);

impl Workspace {
    fn create() -> Result<Self, Failure> {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| return elapsed.subsec_nanos());
        let requested =
            std::env::temp_dir().join(format!("opcov-doctor-{}-{nanos}", process::id()));
        DirBuilder::new()
            .create(&requested)
            .map_err(|error| return Failure::io("create", &requested, &error))?;
        let path = fs::canonicalize(&requested)
            .map_err(|error| return Failure::io("resolve", &requested, &error))?;
        return Ok(Self(path));
    }

    fn write(&self, name: &str, contents: &str) -> Result<PathBuf, Failure> {
        let path = self.0.join(name);
        fs::write(&path, contents).map_err(|error| return Failure::io("write", &path, &error))?;
        return Ok(path);
    }

    fn file(&self, name: &str) -> Result<(PathBuf, File), Failure> {
        let path = self.0.join(name);
        let file =
            File::create(&path).map_err(|error| return Failure::io("create", &path, &error))?;
        return Ok((path, file));
    }

    fn remove(self) -> Result<(), Failure> {
        return fs::remove_dir_all(&self.0)
            .map_err(|error| return Failure::io("remove", &self.0, &error));
    }
}

fn wait(child: &mut Child, php: &Path) -> Result<ExitStatus, Failure> {
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait().map_err(|error| {
            return Failure::io("wait for", php, &error);
        })? {
            return Ok(status);
        }
        if started.elapsed() > DEADLINE {
            child
                .kill()
                .map_err(|error| return Failure::io("stop", php, &error))?;
            child
                .wait()
                .map_err(|error| return Failure::io("wait for", php, &error))?;
            return Err(Failure::failed(format!(
                "{} did not finish the self-test within {} seconds",
                php.display(),
                DEADLINE.as_secs()
            )));
        }
        thread::sleep(POLL);
    }
}

fn read_text(path: &Path) -> Result<String, Failure> {
    let bytes = fs::read(path).map_err(|error| return Failure::io("read", path, &error))?;
    return Ok(String::from_utf8_lossy(&bytes).into_owned());
}

fn examine(workspace: &Workspace, php: &Path) -> Result<Checkup, Failure> {
    workspace.write("probe.php", PROBE_SOURCE)?;
    let script = workspace.write("doctor.php", SELFTEST_SCRIPT)?;
    let result = workspace.0.join("result.json");
    let (output_path, output) = workspace.file("output.txt")?;
    let errors = output
        .try_clone()
        .map_err(|error| return Failure::io("share", &output_path, &error))?;
    let mut child = Command::new(php)
        .arg("-d")
        .arg("opcov.enabled=1")
        .arg("-d")
        .arg(format!("opcov.collect_paths={}", workspace.0.display()))
        .arg(&script)
        .arg(&result)
        .stdin(Stdio::null())
        .stdout(output)
        .stderr(errors)
        .spawn()
        .map_err(|error| return Failure::io("run", php, &error))?;
    let status = wait(&mut child, php)?;
    let startup_output = read_text(&output_path)?;
    let report = match fs::read(&result) {
        Ok(bytes) => Some(
            serde_json::from_slice::<ChildReport>(&bytes).map_err(|error| {
                return Failure::failed(format!(
                    "cannot read the self-test result {}: {error}",
                    result.display()
                ));
            })?,
        ),
        Err(error) if error.kind() == io::ErrorKind::NotFound && status.success() => {
            return Err(Failure::failed(format!(
                "{} exited without writing the self-test result {}; its output was:\n{startup_output}",
                php.display(),
                result.display()
            )));
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err(Failure::failed(format!(
                "{} failed ({status}) before writing the self-test result; its output was:\n{startup_output}",
                php.display()
            )));
        }
        Err(error) => return Err(Failure::io("read", &result, &error)),
    };
    return Ok(assess(php, report, startup_output));
}

pub(crate) fn run(php: &Path) -> Result<Checkup, Failure> {
    let workspace = Workspace::create()?;
    let examined = examine(&workspace, php);
    let removed = workspace.remove();
    let checkup = examined?;
    removed?;
    return Ok(checkup);
}

pub(crate) fn render_text(out: &mut dyn Write, checkup: &Checkup) -> io::Result<()> {
    writeln!(out, "opcov doctor: {}", checkup.php_binary)?;
    if let Some(runtime) = &checkup.runtime {
        writeln!(
            out,
            "  PHP {} {} ({}), opcov {}, VM {}",
            runtime.php,
            if runtime.zts { "ZTS" } else { "NTS" },
            runtime.sapi,
            runtime.opcov.as_deref().unwrap_or("unknown"),
            runtime.vm_kind.as_deref().unwrap_or("unknown")
        )?;
        writeln!(
            out,
            "  php.ini: {}",
            runtime.ini_file.as_deref().unwrap_or("(none)")
        )?;
        for scanned in &runtime.scanned_files {
            writeln!(out, "  scanned: {scanned}")?;
        }
    }
    let self_test = match checkup.self_test {
        SelfTest::Passed => "passed",
        SelfTest::NotRun => "not run",
        SelfTest::Failed => "failed",
    };
    writeln!(out, "  probe self-test: {self_test}")?;
    for finding in &checkup.findings {
        writeln!(out, "problem [{}]: {}", finding.code, finding.problem)?;
        writeln!(out, "  fix: {}", finding.fix)?;
        writeln!(out, "  see: {}", finding.link)?;
    }
    if checkup.runtime.is_none() && !checkup.startup_output.trim().is_empty() {
        writeln!(out, "PHP printed while starting:")?;
        for line in checkup.startup_output.lines() {
            writeln!(out, "  {line}")?;
        }
    }
    return writeln!(out, "doctor: {} problem(s)", checkup.findings.len());
}

pub(crate) fn render_json(out: &mut dyn Write, checkup: &Checkup) -> io::Result<()> {
    serde_json::to_writer(&mut *out, checkup)?;
    return writeln!(out);
}

#[cfg(test)]
mod tests {
    use super::*;

    const API_MISMATCH: &str = "PHP Warning:  PHP Startup: opcov: Unable to initialize module
Module compiled with module API=20250925
PHP    compiled with module API=20240924
These options need to match
 in Unknown on line 0
";

    const BUILD_MISMATCH: &str = "Warning: PHP Startup: opcov: Unable to initialize module
Module compiled with build ID=API20250925,NTS
PHP    compiled with build ID=API20250925,TS
These options need to match
";

    fn child(json: &str) -> ChildReport {
        return serde_json::from_str(json).unwrap();
    }

    fn codes(checkup: &Checkup) -> Vec<&str> {
        return checkup
            .findings
            .iter()
            .map(|finding| return finding.code.as_str())
            .collect();
    }

    fn loaded(extra: &str) -> String {
        return format!(
            r#"{{"loaded":true,"php":"8.5.12","zts":false,"sapi":"cli","ini_file":null,"scanned_files":"/etc/php/a.ini,\n/etc/php/b.ini","version":"{CLI_VERSION}"{extra}}}"#
        );
    }

    #[test]
    fn a_minor_version_mismatch_names_both_php_versions() {
        let finding = build_mismatch(API_MISMATCH).unwrap();
        assert_eq!(finding.code, "build-mismatch");
        assert_eq!(
            finding.problem,
            "the opcov extension was built for PHP 8.5 (module API 20250925), but this PHP is PHP 8.4 (module API 20240924), so PHP refuses to load it"
        );
        assert_eq!(finding.fix, "install the opcov build for PHP 8.4");
    }

    #[test]
    fn a_thread_safety_mismatch_names_both_builds() {
        let finding = build_mismatch(BUILD_MISMATCH).unwrap();
        assert_eq!(
            finding.problem,
            "the opcov extension is a NTS (non-thread-safe) build (build ID API20250925,NTS), but this PHP is ZTS (thread-safe) (build ID API20250925,TS), so PHP refuses to load it"
        );
        assert_eq!(
            finding.fix,
            "install the ZTS (thread-safe) build of opcov for this PHP"
        );
    }

    #[test]
    fn a_thread_safety_mismatch_found_by_the_dynamic_loader_names_both_builds() {
        let nts_on_zts = "Warning: PHP Startup: Unable to load dynamic library '/x/opcov.so' (tried: /x/opcov.so (/x/opcov.so: undefined symbol: executor_globals), /lib/opcov.so.so (cannot open)) in Unknown on line 0";
        let finding = build_mismatch(nts_on_zts).unwrap();
        assert_eq!(
            finding.problem,
            "the opcov extension is a NTS (non-thread-safe) build, but this PHP is ZTS (thread-safe), so loading it fails on the missing symbol executor_globals"
        );
        let zts_on_nts = nts_on_zts.replace("executor_globals)", "tsrm_get_ls_cache)");
        assert_eq!(
            build_mismatch(&zts_on_nts).unwrap().fix,
            "install the NTS (non-thread-safe) build of opcov for this PHP"
        );
        let other = nts_on_zts.replace("executor_globals)", "something_else)");
        assert_eq!(build_mismatch(&other), None);
    }

    #[test]
    fn an_unloaded_extension_without_a_mismatch_says_how_to_load_it() {
        let checkup = assess(
            Path::new("php"),
            Some(child(
                r#"{"loaded":false,"php":"8.5.12","zts":false,"sapi":"cli","ini_file":null,"scanned_files":null}"#,
            )),
            String::new(),
        );
        assert_eq!(codes(&checkup), ["extension-not-loaded"]);
        assert_eq!(checkup.self_test, SelfTest::NotRun);
        let mismatched = assess(Path::new("php"), None, String::from(API_MISMATCH));
        assert_eq!(codes(&mismatched), ["build-mismatch"]);
    }

    #[test]
    fn a_passing_self_test_has_no_findings() {
        let checkup = assess(
            Path::new("php"),
            Some(child(&loaded(
                r#","probe":{"covered":[4,5],"executable":[4,5,7],"outcomes":1},"status":{"off_reason":null,"off_code":null,"vm_kind":"HYBRID","conflicts":[]}"#,
            ))),
            String::new(),
        );
        assert_eq!(checkup.self_test, SelfTest::Passed);
        assert_eq!(codes(&checkup), Vec::<&str>::new());
        assert_eq!(checkup.verdict(), Verdict::Clean);
        let runtime = checkup.runtime.unwrap();
        assert_eq!(runtime.scanned_files, ["/etc/php/a.ini", "/etc/php/b.ini"]);
    }

    #[test]
    fn every_conflict_is_a_finding_with_its_fix_and_the_refusal_is_not_repeated() {
        let checkup = assess(
            Path::new("php"),
            Some(child(&loaded(
                r#","refusal":"opcov\\CannotCollectException: opcov cannot collect coverage: ...","status":{"off_reason":"a (see https://x#opcache-jit); b (see https://x#opcache-optimizer)","off_code":"opcache-jit","vm_kind":"HYBRID","conflicts":[{"code":"opcache-jit","problem":"jit on","fix":"set opcache.jit=disable"},{"code":"opcache-optimizer","problem":"optimizer on","fix":"set opcache.optimization_level=0"}]}"#,
            ))),
            String::new(),
        );
        assert_eq!(codes(&checkup), ["opcache-jit", "opcache-optimizer"]);
        assert_eq!(checkup.findings[0].fix, "set opcache.jit=disable");
        assert_eq!(
            checkup.findings[1].link,
            "https://github.com/stubbedev/opcov/blob/main/docs/troubleshooting.md#opcache-optimizer"
        );
        assert_eq!(checkup.self_test, SelfTest::NotRun);
        assert_eq!(checkup.verdict(), Verdict::Findings);
    }

    #[test]
    fn an_off_reason_that_is_not_a_conflict_is_a_finding_without_its_link_text() {
        let checkup = assess(
            Path::new("php"),
            Some(child(&loaded(
                r#","refusal":"x","status":{"off_reason":"a second PHP thread started (see https://x#second-thread)","off_code":"second-thread","vm_kind":"HYBRID","conflicts":[]}"#,
            ))),
            String::new(),
        );
        assert_eq!(codes(&checkup), ["second-thread"]);
        assert_eq!(checkup.findings[0].problem, "a second PHP thread started");
    }

    #[test]
    fn a_wrong_probe_result_fails_the_self_test() {
        let checkup = assess(
            Path::new("php"),
            Some(child(&loaded(
                r#","probe":{"covered":[4],"executable":[4,5,7],"outcomes":0},"status":{"off_reason":null,"off_code":null,"vm_kind":"HYBRID","conflicts":[]}"#,
            ))),
            String::new(),
        );
        assert_eq!(codes(&checkup), ["self-test-failed"]);
        assert_eq!(checkup.self_test, SelfTest::Failed);
    }

    #[test]
    fn extensions_from_another_compatibility_line_are_reported() {
        assert_eq!(version_mismatch(CLI_VERSION), None);
        assert_eq!(compatibility_line("0.1.9"), Some((0, Some(1))));
        assert_eq!(compatibility_line("1.4.0"), compatibility_line("1.0.2"));
        assert_ne!(compatibility_line("0.2.0"), compatibility_line("0.1.0"));
        assert_eq!(
            version_mismatch("nonsense").map(|finding| return finding.code),
            Some(String::from("version-mismatch"))
        );
    }
}
