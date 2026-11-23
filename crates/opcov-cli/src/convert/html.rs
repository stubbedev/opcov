use std::fmt;
use std::io::{self, Write};

use opcov_core::report::SourcePath;

use super::{Context, ConvertError};
use crate::source::read_verified;
use crate::view::{FileView, LineView, Tally, Totals};

const STYLE: &str = "body{font-family:system-ui,sans-serif;margin:1rem;color:#1f2328;background:#fff}\
table{border-collapse:collapse}\
.summary td,.summary th{padding:.25rem .75rem;border-bottom:1px solid #d0d7de;text-align:left}\
.source{width:100%;font-family:ui-monospace,monospace;font-size:13px}\
.source td{padding:0 .5rem;white-space:pre;vertical-align:top}\
.source td.n,.source td.t,.source td.b{text-align:right;color:#57606a}\
tr.covered td.s{background:#dafbe1}\
tr.uncovered td.s{background:#ffebe9}\
tr.partial td.s{background:#fff8c5}\
@media (prefers-color-scheme:dark){body{color:#e6edf3;background:#0d1117}\
tr.covered td.s{background:#12361f}tr.uncovered td.s{background:#4b1d1d}tr.partial td.s{background:#3d3410}}";

struct Escaped<'t>(&'t str);

impl fmt::Display for Escaped<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for character in self.0.chars() {
            match character {
                '&' => formatter.write_str("&amp;")?,
                '<' => formatter.write_str("&lt;")?,
                '>' => formatter.write_str("&gt;")?,
                '"' => formatter.write_str("&quot;")?,
                other => fmt::Write::write_char(formatter, other)?,
            }
        }
        return Ok(());
    }
}

struct Share(Tally);

impl fmt::Display for Share {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        return write!(
            formatter,
            "{}/{} ({})",
            self.0.covered,
            self.0.total,
            self.0.percent()
        );
    }
}

fn row_class(line: Option<&LineView<'_>>) -> &'static str {
    let Some(line) = line else {
        return "plain";
    };
    if !line.executable() && line.branches.total == 0 {
        return "plain";
    }
    if !line.covered() {
        return "uncovered";
    }
    if line.branches.covered < line.branches.total {
        return "partial";
    }
    return "covered";
}

fn summary_row(
    out: &mut impl Write,
    index: usize,
    path: &SourcePath,
    totals: Totals,
) -> io::Result<()> {
    return writeln!(
        out,
        "<tr><td><a href=\"#f{index}\">{}</a></td><td>{}</td><td>{}</td><td>{}</td></tr>",
        Escaped(path.as_str()),
        Share(totals.lines),
        Share(totals.branches),
        Share(totals.functions)
    );
}

fn source_section(
    out: &mut impl Write,
    index: usize,
    file: &FileView<'_>,
    text: &str,
) -> io::Result<()> {
    writeln!(
        out,
        "<section id=\"f{index}\"><h2>{}</h2><table class=\"source\">",
        Escaped(file.path.as_str())
    )?;
    let mut views = file.lines.iter().peekable();
    for (number, source) in (1_u32..).zip(text.lines()) {
        let line = views.next_if(|view| return view.line.get() == number);
        let tests = line
            .filter(|view| return view.executable())
            .map_or_else(String::new, |view| return view.tests().to_string());
        let branches = line
            .filter(|view| return view.branches.total > 0)
            .map_or_else(String::new, |view| {
                return format!("{}/{}", view.branches.covered, view.branches.total);
            });
        writeln!(
            out,
            "<tr class=\"{}\"><td class=\"n\">{number}</td><td class=\"t\">{tests}</td><td class=\"b\">{branches}</td><td class=\"s\">{}</td></tr>",
            row_class(line),
            Escaped(source)
        )?;
    }
    return writeln!(out, "</table></section>");
}

pub(super) fn write<W: Write>(context: &Context<'_>, mut out: W) -> Result<W, ConvertError> {
    let mut files: Vec<(SourcePath, Totals)> = Vec::new();
    let mut all = Totals::default();
    context.each(
        &|_, view, _| return Ok((view.path.clone(), view.totals(None))),
        &mut |(path, totals)| {
            all.absorb(totals);
            files.push((path, totals));
            return Ok(());
        },
    )?;
    writeln!(
        out,
        "<!DOCTYPE html>\n<html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>opcov coverage</title><style>{STYLE}</style></head><body>"
    )?;
    writeln!(
        out,
        "<h1>opcov coverage</h1><p>Lines {}, branches {}, functions {}. Counts are numbers of tests.</p>",
        Share(all.lines),
        Share(all.branches),
        Share(all.functions)
    )?;
    writeln!(
        out,
        "<table class=\"summary\"><thead><tr><th>File</th><th>Lines</th><th>Branches</th><th>Functions</th></tr></thead><tbody>"
    )?;
    for (index, (path, totals)) in files.iter().enumerate() {
        summary_row(&mut out, index, path, *totals)?;
    }
    writeln!(out, "</tbody></table>")?;
    let paths = &context.header().paths;
    context.each(
        &|index, view, file| {
            if files
                .get(index)
                .is_none_or(|(path, _)| return path != view.path)
            {
                return Err(context.changed());
            }
            let source = read_verified(paths, view.path, file).map_err(ConvertError::Source)?;
            let text = String::from_utf8_lossy(&source);
            let mut bytes = Vec::new();
            source_section(&mut bytes, index, view, &text)?;
            return Ok(bytes);
        },
        &mut |bytes| return Ok(out.write_all(&bytes)?),
    )?;
    writeln!(out, "</body></html>")?;
    return Ok(out);
}
