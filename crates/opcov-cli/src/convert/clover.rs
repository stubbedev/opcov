use std::collections::BTreeMap;
use std::io::Write;

use opcov_core::cfg::{ClassRef, Line};
use opcov_core::report::Report;

use crate::view::{FileView, FunctionView, LineView, Totals, Unit, file_view};
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

fn package_of<'f>(file: &'f FileView<'_>) -> Option<&'f str> {
    return file.units().into_iter().find_map(|unit| {
        return match unit {
            Unit::Class(class) => namespace(class),
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

fn package_counts(files: &[&FileView<'_>], named: bool) -> Counts {
    let mut counts = Counts {
        packages: u64::from(named),
        ..Counts::default()
    };
    for file in files {
        counts.absorb(file_counts(file));
    }
    return counts;
}

pub(crate) fn write<W: Write>(report: &Report, out: W) -> Result<W, XmlError> {
    let paths = &report.header().paths;
    let views: Vec<FileView<'_>> = report
        .files()
        .iter()
        .map(|(path, file)| return file_view(paths, path, file))
        .collect();
    let mut packages: BTreeMap<Option<&str>, Vec<&FileView<'_>>> = BTreeMap::new();
    for view in &views {
        packages.entry(package_of(view)).or_default().push(view);
    }
    let mut project = Counts::default();
    for (package, files) in &packages {
        project.absorb(package_counts(files, package.is_some()));
    }
    return XmlWriter::document(out, None, |xml| {
        let root = [
            ("clover", Value::Text(env!("CARGO_PKG_VERSION"))),
            ("generated", Value::Text(TIMESTAMP)),
        ];
        return xml.element("coverage", &root, |xml| {
            let attributes = [("timestamp", Value::Text(TIMESTAMP))];
            return xml.element("project", &attributes, |xml| {
                metrics(xml, project, Level::Project)?;
                for (package, files) in &packages {
                    let Some(name) = package else {
                        for file in files {
                            file_element(xml, file)?;
                        }
                        continue;
                    };
                    xml.element("package", &[("name", Value::Text(name))], |xml| {
                        metrics(xml, package_counts(files, true), Level::Package)?;
                        for file in files {
                            file_element(xml, file)?;
                        }
                        return Ok(());
                    })?;
                }
                return Ok(());
            });
        });
    });
}
