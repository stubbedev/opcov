use std::collections::BTreeSet;
use std::path::PathBuf;

use opcov_core::cfg::Line;

use super::{Change, ChangeError, ChangeSet, FileChange, LineRange};

fn line(text: &str) -> Option<Line> {
    let canonical = !text.is_empty()
        && !text.starts_with('0')
        && text.bytes().all(|byte| return byte.is_ascii_digit());
    return canonical
        .then(|| return text.parse().ok().and_then(Line::new))
        .flatten();
}

fn range(text: &str) -> Option<LineRange> {
    return match text.split_once('-') {
        Some((first, last)) => LineRange::new(line(first)?, line(last)?),
        None => line(text).map(LineRange::single),
    };
}

fn looks_like_ranges(text: &str) -> bool {
    return !text.is_empty()
        && text
            .bytes()
            .all(|byte| return byte.is_ascii_digit() || byte == b'-' || byte == b',');
}

fn entry(text: &str) -> Result<(PathBuf, FileChange), String> {
    let Some((path, ranges)) = text.rsplit_once(':').filter(|(_, ranges)| {
        return looks_like_ranges(ranges);
    }) else {
        return Ok((PathBuf::from(text), FileChange::Whole));
    };
    if path.is_empty() {
        return Err(format!("{text:?} names no path before the line ranges"));
    }
    let parts = ranges
        .split(',')
        .map(|part| {
            return range(part).map(Change::Lines).ok_or_else(|| {
                return format!(
                    "{part:?} is not a line range: expected `start-end` or `line`, with 1 <= start <= end"
                );
            });
        })
        .collect::<Result<BTreeSet<Change>, String>>()?;
    return Ok((PathBuf::from(path), FileChange::Parts(parts)));
}

pub(crate) fn parse_list(source: &str, input: &str) -> Result<ChangeSet, ChangeError> {
    let mut set = ChangeSet::default();
    for (number, raw) in (1_usize..).zip(input.lines()) {
        let text = raw.trim_end();
        if text.is_empty() {
            continue;
        }
        let (path, change) = entry(text).map_err(|message| {
            return ChangeError {
                source: source.to_owned(),
                line: number,
                message,
            };
        })?;
        set.add(path, change);
    }
    return Ok(set);
}
