use opcov_core::cfg::Line;

use super::diff::{Hunk, HunkLine, LineKind};

const EDIT_BUDGET: usize = 1024;

fn split(source: &[u8]) -> Vec<&[u8]> {
    return source
        .split_inclusive(|byte| return *byte == b'\n')
        .collect();
}

fn hunk_line(kind: LineKind, line: &[u8]) -> HunkLine {
    let (text, terminated) = line
        .strip_suffix(b"\n")
        .map_or((line, false), |body| return (body, true));
    return HunkLine {
        kind,
        text: text.to_vec(),
        terminated,
    };
}

fn number(index: usize) -> Option<Line> {
    return Line::new(u32::try_from(index.checked_add(1)?).ok()?);
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Step {
    Keep,
    Remove,
    Add,
}

struct Snapshot {
    low: usize,
    values: Vec<usize>,
}

impl Snapshot {
    fn get(&self, diagonal: usize) -> Option<usize> {
        return self.values.get(diagonal.checked_sub(self.low)?).copied();
    }
}

fn goes_down(
    frontier: &dyn Fn(usize) -> Option<usize>,
    diagonal: usize,
    low: usize,
    high: usize,
) -> bool {
    return diagonal == low
        || (diagonal != high
            && frontier(diagonal.saturating_sub(1)) < frontier(diagonal.saturating_add(1)));
}

fn same(old: &[&[u8]], position: usize, new: &[&[u8]], other: usize) -> bool {
    return old.get(position) == new.get(other);
}

fn shortest(old: &[&[u8]], new: &[&[u8]]) -> Option<Vec<Step>> {
    let (n, m) = (old.len(), new.len());
    let max = n.saturating_add(m).min(EDIT_BUDGET);
    let offset = max.saturating_add(1);
    let mut frontier = vec![0_usize; offset.saturating_mul(2).saturating_add(1)];
    let mut trace: Vec<Snapshot> = Vec::new();
    for depth in 0..=max {
        let (low, high) = (offset.checked_sub(depth)?, offset.saturating_add(depth));
        let kept = low.saturating_sub(1);
        trace.push(Snapshot {
            low: kept,
            values: frontier.get(kept..=high.saturating_add(1))?.to_vec(),
        });
        for diagonal in (low..=high).step_by(2) {
            let read = |index: usize| return frontier.get(index).copied();
            let mut x = if goes_down(&read, diagonal, low, high) {
                read(diagonal.saturating_add(1))?
            } else {
                read(diagonal.saturating_sub(1))?.saturating_add(1)
            };
            let mut y = x.saturating_add(offset).checked_sub(diagonal)?;
            while x < n && y < m && same(old, x, new, y) {
                x = x.saturating_add(1);
                y = y.saturating_add(1);
            }
            *frontier.get_mut(diagonal)? = x;
            if x >= n && y >= m {
                return backtrack(&trace, n, m, offset);
            }
        }
    }
    return None;
}

fn backtrack(trace: &[Snapshot], n: usize, m: usize, offset: usize) -> Option<Vec<Step>> {
    let mut steps = Vec::new();
    let (mut x, mut y) = (n, m);
    for (depth, snapshot) in trace.iter().enumerate().rev() {
        if depth == 0 {
            steps.extend(std::iter::repeat_n(Step::Keep, x));
            break;
        }
        let diagonal = x.saturating_add(offset).checked_sub(y)?;
        let (low, high) = (offset.checked_sub(depth)?, offset.saturating_add(depth));
        let read = |index: usize| return snapshot.get(index);
        let down = goes_down(&read, diagonal, low, high);
        let previous = if down {
            diagonal.saturating_add(1)
        } else {
            diagonal.saturating_sub(1)
        };
        let start_x = snapshot.get(previous)?;
        let start_y = start_x.saturating_add(offset).checked_sub(previous)?;
        let (after_x, after_y) = if down {
            (start_x, start_y.saturating_add(1))
        } else {
            (start_x.saturating_add(1), start_y)
        };
        while x > after_x && y > after_y {
            steps.push(Step::Keep);
            x = x.saturating_sub(1);
            y = y.saturating_sub(1);
        }
        steps.push(if down { Step::Add } else { Step::Remove });
        (x, y) = (start_x, start_y);
    }
    steps.reverse();
    return Some(steps);
}

fn replace_all(old: &[&[u8]], new: &[&[u8]]) -> Vec<Step> {
    let mut steps = vec![Step::Remove; old.len()];
    steps.extend(std::iter::repeat_n(Step::Add, new.len()));
    return steps;
}

pub(crate) fn hunks(old_source: &[u8], new_source: &[u8]) -> Option<Vec<Hunk>> {
    let (old, new) = (split(old_source), split(new_source));
    let prefix = old
        .iter()
        .zip(&new)
        .take_while(|(left, right)| return left == right)
        .count();
    let suffix = old
        .iter()
        .rev()
        .zip(new.iter().rev())
        .take(old.len().min(new.len()).saturating_sub(prefix))
        .take_while(|(left, right)| return left == right)
        .count();
    let old_middle = old
        .get(prefix..old.len().saturating_sub(suffix))
        .unwrap_or_default();
    let new_middle = new
        .get(prefix..new.len().saturating_sub(suffix))
        .unwrap_or_default();
    let steps = shortest(old_middle, new_middle)
        .unwrap_or_else(|| return replace_all(old_middle, new_middle));
    let mut found = Vec::new();
    let (mut x, mut y) = (prefix, prefix);
    let mut open: Option<Hunk> = None;
    for step in steps {
        if step == Step::Keep {
            found.extend(open.take());
            x = x.saturating_add(1);
            y = y.saturating_add(1);
            continue;
        }
        if open.is_none() {
            open = Some(Hunk {
                old_first: number(x)?,
                new_first: number(y)?,
                lines: Vec::new(),
            });
        }
        let hunk = open.as_mut()?;
        if step == Step::Remove {
            hunk.lines.push(hunk_line(LineKind::Removed, old.get(x)?));
            x = x.saturating_add(1);
        } else {
            hunk.lines.push(hunk_line(LineKind::Added, new.get(y)?));
            y = y.saturating_add(1);
        }
    }
    found.extend(open);
    return Some(found);
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::hunks;
    use crate::changes::original;

    fn file(lines: &[u8], terminated: bool) -> Vec<u8> {
        let mut text: Vec<u8> = lines
            .iter()
            .flat_map(|line| return [b'a'.saturating_add(*line % 4), b'\n'])
            .collect();
        if !terminated {
            text.pop();
        }
        return text;
    }

    proptest! {
        #[test]
        fn reverting_the_hunks_gives_back_the_old_file(
            old in proptest::collection::vec(any::<u8>(), 0..60),
            new in proptest::collection::vec(any::<u8>(), 0..60),
            old_terminated in any::<bool>(),
            new_terminated in any::<bool>(),
        ) {
            let (old, new) = (file(&old, old_terminated), file(&new, new_terminated));
            prop_assert_eq!(original(&new, &hunks(&old, &new).unwrap()).unwrap(), old);
        }
    }

    #[test]
    fn an_edit_gives_one_hunk_with_only_the_changed_lines() {
        let found = hunks(b"a\nb\nc\nd\n", b"a\nB\nc\nd\ne\n").unwrap();
        let shape: Vec<(u32, u32, usize)> = found
            .iter()
            .map(|hunk| return (hunk.old_first.get(), hunk.new_first.get(), hunk.lines.len()))
            .collect();
        assert_eq!(shape, [(2, 2, 2), (5, 5, 1)]);
    }

    #[test]
    fn identical_files_give_no_hunks() {
        assert_eq!(hunks(b"a\nb\n", b"a\nb\n"), Some(Vec::new()));
    }
}
