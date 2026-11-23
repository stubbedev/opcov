#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::error::Error;
use std::fs::{self, File};
use std::io::{self, BufWriter, Write as _};
use std::path::{Path, PathBuf};

use opcov_core::cfg::{FilePath, Line, OpIndex};
use opcov_core::report::{
    FileCoverage, Function, Generator, Header, HitSet, IdSorted, Outcome, OutcomeId, OutcomeKind,
    PathBase, ReportWriter, Root, Settings, Sha256, SourcePath, Span, TestIndex, TestTable, VmKind,
};
use opcov_syntax::{FunctionSource, SourceFile};

type Failure = Box<dyn Error>;

struct Shape {
    tests: u32,
    window: u64,
    density: u64,
}

const fn mix(mut value: u64) -> u64 {
    value = value.wrapping_add(0x9e37_79b9_7f4a_7c15);
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    return value ^ (value >> 31);
}

fn sources(root: &Path) -> Result<Vec<String>, Failure> {
    let mut pending = vec![root.to_path_buf()];
    let mut found = Vec::new();
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory)? {
            let entry = entry?;
            let kind = entry.file_type()?;
            let path = entry.path();
            if kind.is_dir() {
                pending.push(path);
            } else if kind.is_file()
                && path
                    .extension()
                    .is_some_and(|extension| return extension == "php")
            {
                let relative = path.strip_prefix(root)?;
                found.push(relative.to_str().ok_or("non-UTF-8 path")?.to_owned());
            }
        }
    }
    found.sort_unstable();
    return Ok(found);
}

fn statement(text: &str) -> bool {
    let trimmed = text.trim();
    return !(trimmed.is_empty()
        || matches!(trimmed, "{" | "}" | "};" | ");" | "]," | "];" | ")")
        || trimmed.starts_with("//")
        || trimmed.starts_with('*')
        || trimmed.starts_with("/*")
        || trimmed.starts_with('#'));
}

struct FileTests {
    first: u64,
    last: u64,
    salt: u64,
}

impl FileTests {
    fn of(position: u64, files: u64, shape: &Shape) -> Self {
        let tests = u64::from(shape.tests);
        let first = position
            .saturating_sub(shape.window)
            .saturating_mul(tests)
            .div_ceil(files.max(1));
        let last = position
            .saturating_add(shape.window)
            .saturating_add(1)
            .saturating_mul(tests)
            .checked_div(files.max(1))
            .unwrap_or_default()
            .min(tests);
        return Self {
            first,
            last,
            salt: mix(position),
        };
    }

    fn hits(&self, site: u64, density: u64) -> Result<HitSet, Failure> {
        let mut tests = Vec::new();
        for test in self.first..self.last {
            if mix(self.salt ^ mix(site) ^ test) % 100 < density {
                tests.push(TestIndex::new(u32::try_from(test)?));
            }
        }
        return Ok(HitSet::new(tests)?);
    }
}

fn function(
    source: &FunctionSource,
    tests: &FileTests,
    shape: &Shape,
    site: &mut u64,
) -> Result<Function, Failure> {
    let mut outcomes = Vec::new();
    let mut id = 0_u32;
    for (op, construct) in (0_u32..).zip(source.constructs()) {
        for outcome in construct.outcomes() {
            *site = site.wrapping_add(1);
            let span = outcome.span;
            let start = span.start();
            let end = span.end();
            let report_span = Span::new([
                start.line().get(),
                start.column().get(),
                end.line().get(),
                end.column().get(),
            ])
            .ok();
            outcomes.push(Outcome::new(
                OutcomeId::new(id),
                OpIndex::new(op),
                OutcomeKind::new(outcome.kind)?,
                start.line(),
                report_span,
                tests.hits(*site, shape.density / 2)?,
            )?);
            id = id.wrapping_add(1);
        }
    }
    let span = source.span();
    return Ok(Function {
        span: Span::new([
            span.start().line().get(),
            span.start().column().get(),
            span.end().line().get(),
            span.end().column().get(),
        ])
        .ok(),
        outcomes: IdSorted::new(outcomes)?,
        loops: IdSorted::default(),
    });
}

fn coverage(
    bytes: &[u8],
    parsed: &SourceFile,
    tests: &FileTests,
    shape: &Shape,
) -> Result<FileCoverage, Failure> {
    let text = String::from_utf8_lossy(bytes);
    let rows: Vec<&str> = text.lines().collect();
    let mut executable = BTreeSet::new();
    for source in parsed.functions() {
        let span = source.span();
        executable.extend(
            (span.start().line().get()..=span.end().line().get())
                .filter(|&line| {
                    return usize::try_from(line.saturating_sub(1))
                        .ok()
                        .and_then(|index| return rows.get(index))
                        .is_some_and(|row| return statement(row));
                })
                .filter_map(Line::new),
        );
    }
    let mut lines = BTreeMap::new();
    for line in executable {
        lines.insert(line, tests.hits(u64::from(line.get()), shape.density)?);
    }
    let mut functions = BTreeMap::new();
    let mut site = 1_u64 << 32;
    for source in parsed.functions() {
        functions.insert(
            source.key().clone(),
            function(source, tests, shape, &mut site)?,
        );
    }
    return Ok(FileCoverage {
        sha256: Sha256::digest(bytes),
        lines,
        functions,
    });
}

fn generate(root: &Path, output: &Path, shape: &Shape) -> Result<(), Failure> {
    let root = fs::canonicalize(root)?;
    let root_text = root.to_str().ok_or("non-UTF-8 root")?.to_owned();
    let paths = sources(&root)?;
    let names: Vec<String> = (0..shape.tests)
        .map(|test| return format!("Tests\\Scale\\T{test:06}::test"))
        .collect();
    let header = Header {
        generator: Generator {
            opcov: "0.1.0".to_owned(),
            php: "8.5.0".to_owned(),
            zts: false,
            vm: VmKind::Hybrid,
        },
        settings: Settings {
            collect_paths: BTreeSet::from([root_text.clone()]),
            exclude_paths: BTreeSet::new(),
            exclude_pattern: None,
        },
        paths: PathBase::Relative(Root::new(root_text)?),
        tests: TestTable::new(names)?,
    };
    let files = u64::try_from(paths.len())?;
    let mut writer = ReportWriter::begin(
        BufWriter::with_capacity(1 << 20, File::create(output)?),
        &header,
    )?;
    let mut skipped = 0_u64;
    for (position, relative) in (0_u64..).zip(&paths) {
        let bytes = fs::read(root.join(relative))?;
        let key_file: FilePath = relative.parse()?;
        let Ok(parsed) = SourceFile::parse(key_file, &bytes) else {
            skipped = skipped.saturating_add(1);
            continue;
        };
        let tests = FileTests::of(position, files, shape);
        let file = coverage(&bytes, &parsed, &tests, shape)?;
        writer.entry(&SourcePath::new(relative.clone())?, &file)?;
    }
    writer.finish()?.flush()?;
    writeln!(
        io::stderr(),
        "{files} sources, {skipped} skipped (parse errors)"
    )?;
    return Ok(());
}

fn argument(arguments: &[String], position: usize, default: u64) -> Result<u64, Failure> {
    return arguments
        .get(position)
        .map_or(Ok(default), |raw| return raw.parse().map_err(Into::into));
}

fn run() -> Result<(), Failure> {
    let arguments: Vec<String> = env::args().collect();
    let (Some(root), Some(output)) = (arguments.get(1), arguments.get(2)) else {
        return Err(
            "usage: report_scale <php-root> <report.json> [tests] [window] [density%]".into(),
        );
    };
    let shape = Shape {
        tests: u32::try_from(argument(&arguments, 3, 50_000)?)?,
        window: argument(&arguments, 4, 8)?,
        density: argument(&arguments, 5, 60)?,
    };
    return generate(&PathBuf::from(root), &PathBuf::from(output), &shape);
}

fn main() -> Result<(), Failure> {
    return run();
}
