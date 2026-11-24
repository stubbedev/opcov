use core::{ops::Range, ptr};

use opcov_core::report::{LoopId, OutcomeId};
use opcov_sys::{
    opcov_emit_hits, opcov_file_hits, opcov_function_hits, opcov_hit_keys, opcov_loop_hit,
    zend_string, zval,
};

use crate::{
    engine::{Covered, FileId, Files, FunctionId},
    guard::{Caught, caught},
    php::Key,
};

#[derive(Debug)]
struct Names {
    lines: Key,
    functions: Key,
    outcomes: Key,
    loops: Key,
    zero: Key,
    second: Key,
}

impl Names {
    fn new() -> Option<Self> {
        return Some(Self {
            lines: Key::new(b"lines")?,
            functions: Key::new(b"functions")?,
            outcomes: Key::new(b"outcomes")?,
            loops: Key::new(b"loops")?,
            zero: Key::new(b"zero")?,
            second: Key::new(b"second")?,
        });
    }

    const fn raw(&self) -> opcov_hit_keys {
        return opcov_hit_keys {
            lines: self.lines.get(),
            functions: self.functions.get(),
            outcomes: self.outcomes.get(),
            loops: self.loops.get(),
            zero: self.zero.get(),
            second: self.second.get(),
        };
    }
}

#[derive(Debug, Default)]
struct Numbered {
    keys: Vec<Key>,
}

impl Numbered {
    fn get(&mut self, number: u32, name: impl Fn(u32) -> String) -> Option<*mut zend_string> {
        let index = usize::try_from(number).ok()?;
        while self.keys.len() <= index {
            let next = u32::try_from(self.keys.len()).ok()?;
            self.keys.push(Key::new(name(next).as_bytes())?);
        }
        return self.keys.get(index).map(Key::get);
    }
}

#[derive(Debug, Clone)]
struct FileSlot {
    file: FileId,
    lines: Range<usize>,
    functions: Range<usize>,
}

#[derive(Debug, Clone)]
struct FunctionSlot {
    function: FunctionId,
    file: FileId,
    outcomes: Range<usize>,
    zero: Range<usize>,
    second: Range<usize>,
}

impl FunctionSlot {
    fn new(files: &Files, function: FunctionId) -> Option<Self> {
        return Some(Self {
            function,
            file: files.function(function)?.file,
            outcomes: 0..0,
            zero: 0..0,
            second: 0..0,
        });
    }
}

#[derive(Debug, Default)]
pub(crate) struct Hits {
    names: Option<Names>,
    outcome_keys: Numbered,
    loop_keys: Numbered,
    files: Vec<FileSlot>,
    lines: Vec<u32>,
    functions: Vec<FunctionSlot>,
    outcomes: Vec<*mut zend_string>,
    zero: Vec<u32>,
    second: Vec<u32>,
    loops: Vec<opcov_loop_hit>,
    loop_ranges: Vec<Range<usize>>,
    raw_functions: Vec<opcov_function_hits>,
    raw_files: Vec<opcov_file_hits>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Section {
    Zero,
    Second,
}

impl Hits {
    fn clear(&mut self) {
        self.files.clear();
        self.lines.clear();
        self.functions.clear();
        self.outcomes.clear();
        self.zero.clear();
        self.second.clear();
        self.loops.clear();
        self.loop_ranges.clear();
        self.raw_functions.clear();
        self.raw_files.clear();
    }

    fn function_slot(&mut self, files: &Files, function: FunctionId) -> Option<&mut FunctionSlot> {
        let position = match self
            .functions
            .binary_search_by_key(&function, |slot| return slot.function)
        {
            Ok(position) => position,
            Err(position) => {
                self.functions
                    .insert(position, FunctionSlot::new(files, function)?);
                position
            }
        };
        return self.functions.get_mut(position);
    }

    fn numbered(
        &mut self,
        files: &Files,
        section: Section,
        function: FunctionId,
        number: u32,
    ) -> Option<()> {
        let numbers = match section {
            Section::Zero => &mut self.zero,
            Section::Second => &mut self.second,
        };
        let at = numbers.len();
        numbers.push(number);
        let slot = self.function_slot(files, function)?;
        let range = match section {
            Section::Zero => &mut slot.zero,
            Section::Second => &mut slot.second,
        };
        if range.start == range.end {
            *range = at..at;
        }
        range.end = at.saturating_add(1);
        return Some(());
    }

    fn group(&mut self, files: &Files, covered: &[Covered]) -> Option<()> {
        for &item in covered {
            match item {
                Covered::Line(file, line) => {
                    let at = self.lines.len();
                    if self
                        .files
                        .last()
                        .is_none_or(|slot| return slot.file != file)
                    {
                        self.files.push(FileSlot {
                            file,
                            lines: at..at,
                            functions: 0..0,
                        });
                    }
                    self.lines.push(line.get());
                    if let Some(slot) = self.files.last_mut() {
                        slot.lines.end = at.saturating_add(1);
                    }
                }
                Covered::Outcome(function, number) => {
                    let key = self.outcome_keys.get(number, |next| {
                        return OutcomeId::new(next).to_string();
                    })?;
                    let at = self.outcomes.len();
                    self.outcomes.push(key);
                    if self
                        .functions
                        .last()
                        .is_none_or(|slot| return slot.function != function)
                    {
                        let mut slot = FunctionSlot::new(files, function)?;
                        slot.outcomes = at..at;
                        self.functions.push(slot);
                    }
                    if let Some(slot) = self.functions.last_mut() {
                        slot.outcomes.end = at.saturating_add(1);
                    }
                }
                Covered::Zero(function, number) => {
                    self.numbered(files, Section::Zero, function, number)?;
                }
                Covered::Second(function, number) => {
                    self.numbered(files, Section::Second, function, number)?;
                }
            }
        }
        return Some(());
    }

    fn attach(&mut self, files: &Files) -> Option<()> {
        for index in 0..self.functions.len() {
            let file = self.functions.get(index)?.file;
            if let Err(position) = self
                .files
                .binary_search_by_key(&file, |slot| return slot.file)
            {
                self.files.insert(
                    position,
                    FileSlot {
                        file,
                        lines: 0..0,
                        functions: 0..0,
                    },
                );
            }
        }
        self.functions.sort_unstable_by(|left, right| {
            return left.file.cmp(&right.file).then_with(|| {
                return files.rank(left.function).cmp(&files.rank(right.function));
            });
        });
        for slot in &mut self.files {
            let start = self
                .functions
                .partition_point(|function| return function.file < slot.file);
            let end = self
                .functions
                .partition_point(|function| return function.file <= slot.file);
            slot.functions = start..end;
        }
        self.files.sort_unstable_by(|left, right| {
            let path =
                |slot: &FileSlot| return files.get(slot.file).map(|record| return &record.path);
            return path(left).cmp(&path(right));
        });
        return Some(());
    }

    fn loops_of(&mut self, zero: Range<usize>, second: Range<usize>) -> Option<Range<usize>> {
        let start = self.loops.len();
        let zero = self.zero.get(zero)?;
        let second = self.second.get(second)?;
        let (mut z, mut s) = (zero.iter().peekable(), second.iter().peekable());
        loop {
            let (number, seen_zero, seen_second) = match (z.peek(), s.peek()) {
                (None, None) => break,
                (Some(&&left), Some(&&right)) if left == right => {
                    z.next();
                    s.next();
                    (left, true, true)
                }
                (Some(&&left), Some(&&right)) if left < right => {
                    z.next();
                    (left, true, false)
                }
                (Some(&&left), None) => {
                    z.next();
                    (left, true, false)
                }
                (_, Some(&&right)) => {
                    s.next();
                    (right, false, true)
                }
            };
            let key = self
                .loop_keys
                .get(number, |next| return LoopId::new(next).to_string())?;
            self.loops.push(opcov_loop_hit {
                key,
                zero: seen_zero,
                second: seen_second,
            });
        }
        return Some(start..self.loops.len());
    }

    fn lay_out(&mut self, files: &Files) -> Option<()> {
        for index in 0..self.functions.len() {
            let slot = self.functions.get(index)?;
            let (zero, second) = (slot.zero.clone(), slot.second.clone());
            let range = self.loops_of(zero, second)?;
            self.loop_ranges.push(range);
        }
        for (slot, loops) in self.functions.iter().zip(&self.loop_ranges) {
            let outcomes = self.outcomes.get(slot.outcomes.clone())?;
            let looped = self.loops.get(loops.clone())?;
            self.raw_functions.push(opcov_function_hits {
                key: files.function_key(slot.function)?,
                outcomes: outcomes.as_ptr(),
                outcome_count: outcomes.len(),
                loops: looped.as_ptr(),
                loop_count: looped.len(),
            });
        }
        for slot in &self.files {
            let lines = self.lines.get(slot.lines.clone())?;
            let functions = self.raw_functions.get(slot.functions.clone())?;
            self.raw_files.push(opcov_file_hits {
                path: files.path_key(slot.file)?,
                lines: lines.as_ptr(),
                line_count: lines.len(),
                functions: if functions.is_empty() {
                    ptr::null()
                } else {
                    functions.as_ptr()
                },
                function_count: functions.len(),
            });
        }
        return Some(());
    }

    fn prepare(&mut self, files: &Files, covered: &[Covered]) -> Option<opcov_hit_keys> {
        self.clear();
        if self.names.is_none() {
            self.names = Some(Names::new()?);
        }
        self.group(files, covered)?;
        self.attach(files)?;
        self.lay_out(files)?;
        return self.names.as_ref().map(Names::raw);
    }

    pub(crate) unsafe fn emit(
        &mut self,
        files: &Files,
        covered: &[Covered],
        target: *mut zval,
    ) -> Option<Caught<()>> {
        let keys = self.prepare(files, covered)?;
        // SAFETY: the caller passes a writable zval; every pointer in the prepared hits points into
        // this value's buffers or at keys it keeps alive, and the engine copies what it keeps.
        return Some(caught(unsafe {
            opcov_emit_hits(
                target,
                &raw const keys,
                self.raw_files.as_ptr(),
                self.raw_files.len(),
            )
        }));
    }
}
