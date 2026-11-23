use std::io::{self, Write};

use opcov_core::report::Report;

use crate::view::{FileView, Taken, file_view};

fn record(out: &mut impl Write, file: &FileView<'_>) -> io::Result<()> {
    writeln!(out, "TN:")?;
    writeln!(out, "SF:{}", file.location)?;
    let named: Vec<_> = file
        .functions
        .iter()
        .filter_map(|function| return function.entry().map(|entry| return (entry, function)))
        .collect();
    for (start, function) in &named {
        writeln!(out, "FN:{start},{}", function.key)?;
    }
    for (_, function) in &named {
        writeln!(out, "FNDA:{},{}", function.tests, function.key)?;
    }
    let totals = file.totals(None);
    writeln!(out, "FNF:{}", totals.functions.total)?;
    writeln!(out, "FNH:{}", totals.functions.covered)?;
    let mut branches: Vec<_> = file
        .functions
        .iter()
        .flat_map(|function| return function.branches.iter())
        .collect();
    branches.sort_by_key(|branch| return (branch.line, branch.block, branch.number));
    for branch in branches {
        let line = branch.line;
        let (block, number) = (branch.block, branch.number);
        match branch.taken {
            Taken::Unreached => writeln!(out, "BRDA:{line},{block},{number},-")?,
            Taken::Tests(count) => writeln!(out, "BRDA:{line},{block},{number},{count}")?,
        }
    }
    writeln!(out, "BRF:{}", totals.branches.total)?;
    writeln!(out, "BRH:{}", totals.branches.covered)?;
    for line in file.lines.iter().filter(|line| return line.executable()) {
        writeln!(out, "DA:{},{}", line.line, line.tests())?;
    }
    writeln!(out, "LF:{}", totals.lines.total)?;
    writeln!(out, "LH:{}", totals.lines.covered)?;
    return writeln!(out, "end_of_record");
}

pub(crate) fn write<W: Write>(report: &Report, mut out: W) -> io::Result<W> {
    let paths = &report.header().paths;
    for (path, file) in report.files() {
        record(&mut out, &file_view(paths, path, file))?;
    }
    return Ok(out);
}
