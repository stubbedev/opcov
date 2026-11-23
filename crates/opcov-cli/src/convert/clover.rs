use std::collections::BTreeMap;
use std::io::Write;

use opcov_core::cfg::{ClassRef, Line};

use super::{Context, ConvertError};
use crate::spool::Spool;
use crate::view::{FileView, FunctionView, LineView, Totals, Unit};
use crate::xml::{Attribute, Value, XmlError, XmlWriter};

const TIMESTAMP: &str = "0";
const COMPLEXITY: &str = "0";

#[derive(Debug, Clone, Copy, Default)]
struct Counts {
    totals: Totals,
    classes: u64,
    files: u64,
    packages: u64,
}

impl Counts {
    const fn absorb(&mut self, other: Self) {
        self.totals.absorb(other.totals);
        self.classes = self.classes.saturating_add(other.classes);
        self.files = self.files.saturating_add(other.files);
        self.packages = self.packages.saturating_add(other.packages);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Level {
    Class,
    File,
    Package,
    Project,
}

fn metrics<W: Write>(xml: &mut XmlWriter<W>, counts: Counts, level: Level) -> Result<(), XmlError> {
    let totals = counts.totals;
    let elements = totals
        .lines
        .total
        .saturating_add(totals.branches.total)
        .saturating_add(totals.functions.total);
    let covered_elements = totals
        .lines
        .covered
        .saturating_add(totals.branches.covered)
        .saturating_add(totals.functions.covered);
    let mut attributes: Vec<Attribute<'_>> = Vec::with_capacity(13);
    if level == Level::Project {
        attributes.push(("packages", Value::Display(&counts.packages)));
    }
    if matches!(level, Level::Project | Level::Package) {
        attributes.push(("files", Value::Display(&counts.files)));
    }
    if level != Level::Class {
        attributes.push(("classes", Value::Display(&counts.classes)));
    }
    attributes.extend([
        ("complexity", Value::Text(COMPLEXITY)),
        ("methods", Value::Display(&totals.functions.total)),
        ("coveredmethods", Value::Display(&totals.functions.covered)),
        ("conditionals", Value::Display(&totals.branches.total)),
        (
            "coveredconditionals",
            Value::Display(&totals.branches.covered),
        ),
        ("statements", Value::Display(&totals.lines.total)),
        ("coveredstatements", Value::Display(&totals.lines.covered)),
        ("elements", Value::Display(&elements)),
        ("coveredelements", Value::Display(&covered_elements)),
    ]);
    return xml.empty("metrics", &attributes);
}

fn namespace(class: &ClassRef) -> Option<&str> {
    let ClassRef::Named(name) = class else {
        return None;
    };
    return name
        .as_str()
        .rsplit_once('\\')
        .map(|(namespace, _)| return namespace);
}

fn package_of(file: &FileView<'_>) -> Option<String> {
    return file.units().into_iter().find_map(|unit| {
        return match unit {
            Unit::Class(class) => namespace(class).map(str::to_owned),
            Unit::File => None,
        };
    });
}

enum Entry<'v> {
    Method(&'v FunctionView<'v>),
    Statement(&'v LineView<'v>),
}

fn line_entries<'v>(file: &'v FileView<'v>) -> BTreeMap<Line, Entry<'v>> {
    let mut entries: BTreeMap<Line, Entry<'v>> = BTreeMap::new();
    for line in &file.lines {
        entries.insert(line.line, Entry::Statement(line));
    }
    for function in &file.functions {
        if let Some(start) = function.start() {
            entries.insert(start, Entry::Method(function));
        }
    }
    return entries;
}

fn line_element<W: Write>(
    xml: &mut XmlWriter<W>,
    line: Line,
    entry: &Entry<'_>,
) -> Result<(), XmlError> {
    return match entry {
        Entry::Method(function) => xml.empty(
            "line",
            &[
                ("num", Value::Display(&line)),
                ("type", Value::Text("method")),
                ("name", Value::Text(&function.member_name())),
                ("complexity", Value::Text(COMPLEXITY)),
                ("count", Value::Display(&function.tests)),
            ],
        ),
        Entry::Statement(view) if view.branches.total > 0 => xml.empty(
            "line",
            &[
                ("num", Value::Display(&line)),
                ("type", Value::Text("cond")),
                ("count", Value::Display(&view.tests())),
                ("truecount", Value::Display(&view.branches.covered)),
                ("falsecount", Value::Display(&view.branches.uncovered())),
            ],
        ),
        Entry::Statement(view) => xml.empty(
            "line",
            &[
                ("num", Value::Display(&line)),
                ("type", Value::Text("stmt")),
                ("count", Value::Display(&view.tests())),
            ],
        ),
    };
}

fn classes<'f>(file: &'f FileView<'_>) -> Vec<&'f ClassRef> {
    return file
        .units()
        .into_iter()
        .filter_map(|unit| {
            return match unit {
                Unit::Class(class) => Some(class),
                Unit::File => None,
            };
        })
        .collect();
}

fn file_counts(file: &FileView<'_>) -> Counts {
    return Counts {
        totals: file.totals(None),
        classes: classes(file)
            .iter()
            .fold(0, |count, _| return u64::saturating_add(count, 1)),
        files: 1,
        packages: 0,
    };
}

fn file_element<W: Write>(xml: &mut XmlWriter<W>, file: &FileView<'_>) -> Result<(), XmlError> {
    let location = file.location.as_str();
    return xml.element(
        "file",
        &[
            ("name", Value::Text(location)),
            ("path", Value::Text(location)),
        ],
        |xml| {
            metrics(xml, file_counts(file), Level::File)?;
            for class in classes(file) {
                let name = class.to_string();
                let mut attributes = vec![("name", Value::Text(&name))];
                if let Some(namespace) = namespace(class) {
                    attributes.push(("namespace", Value::Text(namespace)));
                }
                xml.element("class", &attributes, |xml| {
                    let class_counts = Counts {
                        totals: file.totals(Some(Unit::Class(class))),
                        ..Counts::default()
                    };
                    return metrics(xml, class_counts, Level::Class);
                })?;
            }
            for (line, entry) in &line_entries(file) {
                line_element(xml, *line, entry)?;
            }
            return Ok(());
        },
    );
}

#[derive(Debug, Default)]
struct Package {
    counts: Counts,
    files: Vec<usize>,
}

const FILE_DEPTH: usize = 2;
const PACKAGED_FILE_DEPTH: usize = 3;

pub(super) fn write<W: Write>(context: &Context<'_>, out: W) -> Result<W, ConvertError> {
    let mut packages: BTreeMap<Option<String>, Package> = BTreeMap::new();
    let mut members: Vec<bool> = Vec::new();
    context.each(
        &|sequence, view, _| return Ok((sequence, package_of(view), file_counts(view))),
        &mut |(sequence, name, counts)| {
            members.push(name.is_some());
            let package = packages.entry(name).or_default();
            package.counts.absorb(counts);
            package.files.push(sequence);
            return Ok(());
        },
    )?;
    let mut project = Counts::default();
    for (name, package) in &mut packages {
        package.counts.packages = u64::from(name.is_some());
        project.absorb(package.counts);
    }
    let mut spool = Spool::new()?;
    context.each(
        &|sequence, view, _| {
            let named = members
                .get(sequence)
                .copied()
                .ok_or_else(|| return context.changed())?;
            let depth = if named {
                PACKAGED_FILE_DEPTH
            } else {
                FILE_DEPTH
            };
            let mut fragment = XmlWriter::fragment(Vec::new(), depth);
            file_element(&mut fragment, view)?;
            return Ok(fragment.finish());
        },
        &mut |bytes| {
            spool.push(&bytes)?;
            return Ok(());
        },
    )?;
    let mut spooled = spool.into_reader()?;
    let mut xml = XmlWriter::begin(out, None)?;
    xml.open(
        "coverage",
        &[
            ("clover", Value::Text(env!("CARGO_PKG_VERSION"))),
            ("generated", Value::Text(TIMESTAMP)),
        ],
    )?;
    xml.open("project", &[("timestamp", Value::Text(TIMESTAMP))])?;
    metrics(&mut xml, project, Level::Project)?;
    for (name, package) in &packages {
        if let Some(name) = name {
            xml.open("package", &[("name", Value::Text(name))])?;
            metrics(&mut xml, package.counts, Level::Package)?;
        }
        for &file in &package.files {
            xml.raw(spooled.chunk(file)?)?;
        }
        if name.is_some() {
            xml.close("package")?;
        }
    }
    xml.close("project")?;
    xml.close("coverage")?;
    return Ok(xml.finish());
}
