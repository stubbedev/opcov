#![expect(
    clippy::redundant_pub_crate,
    reason = "pub(crate) is the intended visibility; the pub this lint suggests is denied by rustc unreachable_pub"
)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use opcov_core::report::Sha256;

pub(crate) const BINARY: &str = env!("CARGO_BIN_EXE_opcov");
pub(crate) const SOURCE: &str = "app/Foo.php";

const TEMPLATE: &str = r#"{"schema":"opcov-report/1","generator":{"opcov":"1.0.0","php":"8.5.12","zts":false,"vm":"HYBRID"},"settings":{"collect_paths":["app"],"exclude_paths":[],"exclude_pattern":null},"paths":{"mode":"relative","root":"@ROOT@"},"tests":["T\\A::one","T\\B::two"],"files":{"app/Foo.php":{"sha256":"@SHA@","lines":{"9":[0,1],"10":[0,1],"11":[0],"13":[1],"14":[1],"16":[1],"21":[],"27":[1],"28":[1]},"functions":{"{main}@app/Foo.php":{"span":null,"outcomes":[],"loops":[]},"App\\helper":{"span":[25,1,29,2],"outcomes":[],"loops":[]},"App\\Foo::bar":{"span":[7,5,17,6],"outcomes":[{"id":"o1","site":2,"kind":"CoalesceValue","line":9,"span":[9,18,9,33],"hits":[1]},{"id":"o2","site":2,"kind":"CoalesceFallback","line":9,"span":[9,18,9,33],"hits":[0]},{"id":"o3","site":4,"kind":"IfTrue","line":10,"span":null,"hits":[0]},{"id":"o4","site":4,"kind":"IfFalse","line":10,"span":null,"hits":[1]},{"id":"o5","site":7,"kind":"LoopBack","line":13,"span":null,"hits":[1]},{"id":"o6","site":7,"kind":"LoopExit","line":13,"span":null,"hits":[]},{"id":"o7","site":11,"kind":"Engine","line":16,"span":null,"hits":[]}@OUTCOMES@],"loops":[{"id":"l1","kind":"Foreach","line":13,"zero":[],"second":[]}]},"App\\Foo::baz":{"span":[19,5,22,6],"outcomes":[],"loops":[]}@FUNCTIONS@,"{closure}@app/Foo.php:27-27":{"span":[27,10,27,36],"outcomes":[],"loops":[]}}}}}
"#;

pub(crate) const UNMATCHED_OUTCOME: &str =
    r#",{"id":"o8","site":13,"kind":"IfTrue","line":14,"span":null,"hits":[1]}"#;
pub(crate) const UNKNOWN_FUNCTION: &str =
    r#","App\\Foo::gone":{"span":null,"outcomes":[],"loops":[]}"#;

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Extras {
    pub outcomes: &'static str,
    pub functions: &'static str,
}

pub(crate) fn fixture_source() -> PathBuf {
    return Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(SOURCE);
}

pub(crate) fn sha256(bytes: &[u8]) -> String {
    return Sha256::digest(bytes).to_string();
}

pub(crate) fn report(root: &str, sha: &str, extras: Extras) -> String {
    return TEMPLATE
        .replace("@ROOT@", root)
        .replace("@SHA@", sha)
        .replace("@OUTCOMES@", extras.outcomes)
        .replace("@FUNCTIONS@", extras.functions);
}

pub(crate) fn workspace(name: &str) -> PathBuf {
    let directory = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    if let Err(error) = fs::remove_dir_all(&directory)
        && error.kind() != std::io::ErrorKind::NotFound
    {
        panic!("cannot clear {}: {error}", directory.display());
    }
    fs::create_dir_all(directory.join("app")).unwrap();
    return directory;
}

pub(crate) fn project(name: &str, extras: Extras) -> (PathBuf, PathBuf) {
    let directory = workspace(name);
    let source = fs::read(fixture_source()).unwrap();
    fs::write(directory.join(SOURCE), &source).unwrap();
    let report_path = directory.join("report.json");
    let root = directory.to_str().unwrap();
    fs::write(&report_path, report(root, &sha256(&source), extras)).unwrap();
    return (directory, report_path);
}

pub(crate) fn run(arguments: &[&str]) -> Output {
    return Command::new(BINARY).args(arguments).output().unwrap();
}

pub(crate) fn stdout(output: &Output) -> String {
    return String::from_utf8(output.stdout.clone()).unwrap();
}

pub(crate) fn stderr(output: &Output) -> String {
    return String::from_utf8(output.stderr.clone()).unwrap();
}
