use std::io::Write;

use opcov_core::report::PathBase;

use super::{Context, ConvertError};
use crate::view::{FileView, LineView, Tally, Totals, Unit, line_totals};
use crate::xml::{Value, XmlError, XmlWriter};

const DOCTYPE: &str =
    r#"<!DOCTYPE coverage SYSTEM "http://cobertura.sourceforge.net/xml/coverage-04.dtd">"#;
const TIMESTAMP: &str = "0";
const COMPLEXITY: &str = "0";

fn line_element<W: Write>(xml: &mut XmlWriter<W>, line: &LineView<'_>) -> Result<(), XmlError> {
    let number = line.line;
    let hits = line.tests();
    if line.branches.total == 0 {
        return xml.empty(
            "line",
            &[
                ("number", Value::Display(&number)),
                ("hits", Value::Display(&hits)),
                ("branch", Value::Text("false")),
            ],
        );
    }
    let branches = line.branches;
    let coverage = format!(
        "{} ({}/{})",
        branches.percent(),
        branches.covered,
        branches.total
    );
    return xml.empty(
        "line",
        &[
            ("number", Value::Display(&number)),
            ("hits", Value::Display(&hits)),
            ("branch", Value::Text("true")),
            ("condition-coverage", Value::Text(&coverage)),
        ],
    );
}

fn lines_element<'l, W: Write>(
    xml: &mut XmlWriter<W>,
    lines: impl Iterator<Item = &'l LineView<'l>>,
) -> Result<(), XmlError> {
    return xml.element("lines", &[], |xml| {
        for line in lines {
            line_element(xml, line)?;
        }
        return Ok(());
    });
}

fn rates(totals: Totals) -> (String, String) {
    return (
        totals.lines.ratio().to_string(),
        totals.branches.ratio().to_string(),
    );
}

fn class_element<W: Write>(
    xml: &mut XmlWriter<W>,
    file: &FileView<'_>,
    unit: Unit<'_>,
    filename: &str,
) -> Result<(), XmlError> {
    let name = match unit {
        Unit::Class(class) => class.to_string(),
        Unit::File => file.path.to_string(),
    };
    let owned = || {
        return file
            .lines
            .iter()
            .filter(move |line| return file.unit_of_line(line) == unit);
    };
    let (line_rate, branch_rate) = rates(file.totals(Some(unit)));
    return xml.element(
        "class",
        &[
            ("name", Value::Text(&name)),
            ("filename", Value::Text(filename)),
            ("line-rate", Value::Text(&line_rate)),
            ("branch-rate", Value::Text(&branch_rate)),
            ("complexity", Value::Text(COMPLEXITY)),
        ],
        |xml| {
            xml.element("methods", &[], |xml| {
                for (index, function) in file.functions.iter().enumerate() {
                    if function.unit != unit || function.range.is_none() {
                        continue;
                    }
                    let mine = || {
                        return file
                            .lines
                            .iter()
                            .filter(move |line| return line.owner == Some(index));
                    };
                    let (line_rate, branch_rate) = rates(line_totals(mine()));
                    xml.element(
                        "method",
                        &[
                            ("name", Value::Text(&function.member_name())),
                            ("signature", Value::Text("")),
                            ("line-rate", Value::Text(&line_rate)),
                            ("branch-rate", Value::Text(&branch_rate)),
                            ("complexity", Value::Text(COMPLEXITY)),
                        ],
                        |xml| return lines_element(xml, mine()),
                    )?;
                }
                return Ok(());
            })?;
            return lines_element(xml, owned());
        },
    );
}

fn package_element<W: Write>(xml: &mut XmlWriter<W>, view: &FileView<'_>) -> Result<(), XmlError> {
    let filename = view.path.as_str();
    let (line_rate, branch_rate) = rates(view.totals(None));
    return xml.element(
        "package",
        &[
            ("name", Value::Text(filename)),
            ("line-rate", Value::Text(&line_rate)),
            ("branch-rate", Value::Text(&branch_rate)),
            ("complexity", Value::Text(COMPLEXITY)),
        ],
        |xml| {
            return xml.element("classes", &[], |xml| {
                for unit in view.units() {
                    class_element(xml, view, unit, filename)?;
                }
                return Ok(());
            });
        },
    );
}

pub(super) fn write<W: Write>(context: &Context<'_>, out: W) -> Result<W, ConvertError> {
    let mut totals = Totals::default();
    context.each(&|_, view, _| return Ok(view.totals(None)), &mut |file| {
        totals.absorb(file);
        return Ok(());
    })?;
    let (line_rate, branch_rate) = rates(totals);
    let lines: Tally = totals.lines;
    let branches: Tally = totals.branches;
    let version = env!("CARGO_PKG_VERSION");
    let mut xml = XmlWriter::begin(out, Some(DOCTYPE))?;
    xml.open(
        "coverage",
        &[
            ("line-rate", Value::Text(&line_rate)),
            ("branch-rate", Value::Text(&branch_rate)),
            ("lines-covered", Value::Display(&lines.covered)),
            ("lines-valid", Value::Display(&lines.total)),
            ("branches-covered", Value::Display(&branches.covered)),
            ("branches-valid", Value::Display(&branches.total)),
            ("complexity", Value::Text(COMPLEXITY)),
            ("version", Value::Text(version)),
            ("timestamp", Value::Text(TIMESTAMP)),
        ],
    )?;
    if let PathBase::Relative(root) = &context.header().paths {
        xml.element("sources", &[], |xml| {
            return xml.text_element("source", root.as_str());
        })?;
    }
    xml.open("packages", &[])?;
    let depth = xml.depth();
    context.each(
        &|_, view, _| {
            let mut fragment = XmlWriter::fragment(Vec::new(), depth);
            package_element(&mut fragment, view)?;
            return Ok(fragment.finish());
        },
        &mut |bytes| return Ok(xml.raw(&bytes)?),
    )?;
    xml.close("packages")?;
    xml.close("coverage")?;
    return Ok(xml.finish());
}
