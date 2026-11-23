use core::ffi::c_char;
use std::{
    fs,
    path::{Path, PathBuf},
    sync::OnceLock,
};

use opcov_core::{
    cfg::FilePath,
    output::write_atomically,
    report::{PathBase, Root},
};
use opcov_sys::{
    ArgInfo, HashTable, MAXPATHLEN, MAY_BE_ARRAY, MAY_BE_NULL, MAY_BE_STRING, MAY_BE_VOID,
    argument, function_entry, opcov_array_entry_at, opcov_array_value_at, opcov_getcwd,
    opcov_parse_array, opcov_parse_optional_string, opcov_parse_path, opcov_parse_path_and_array,
    opcov_read_bool, opcov_read_string, opcov_type_name, optional_argument, zend_execute_data,
    zend_function_entry, zend_string, zend_ulong, zval,
};
use serde_json::{Map, Value, json};

use crate::{
    accepts_no_arguments,
    engine::{Engine, FileId, FiredList, OffReason, Role, Verdict, with_engine},
    guard::{COLLECTION, EntryPoint, guard},
    ini,
    php::{self, Exception},
    report::{self, Entry},
    threads, zend,
};

pub(crate) static STARTUP: OnceLock<OffReason> = OnceLock::new();

const CANNOT_COLLECT: &str = "opcov cannot collect coverage";

static VOID: ArgInfo<0> = ArgInfo::returning(MAY_BE_VOID);
static ARRAY: ArgInfo<0> = ArgInfo::returning(MAY_BE_ARRAY);
static FILES: ArgInfo<1> = ArgInfo::new(1, MAY_BE_ARRAY, [argument(c"files", MAY_BE_ARRAY)]);
static FILE: ArgInfo<1> = ArgInfo::new(1, MAY_BE_ARRAY, [argument(c"file", MAY_BE_STRING)]);
static TEST: ArgInfo<1> = ArgInfo::new(
    0,
    MAY_BE_ARRAY,
    [optional_argument(
        c"test",
        MAY_BE_STRING | MAY_BE_NULL,
        c"null",
    )],
);
static REPORT: ArgInfo<2> = ArgInfo::new(
    1,
    MAY_BE_VOID,
    [
        argument(c"path", MAY_BE_STRING),
        optional_argument(c"options", MAY_BE_ARRAY, c"[]"),
    ],
);

pub(crate) const ENSURE_CAN_COLLECT: zend_function_entry =
    function_entry(c"opcov\\ensure_can_collect", ensure_can_collect, &VOID);
pub(crate) const START: zend_function_entry = function_entry(c"opcov\\start", start, &VOID);
pub(crate) const STOP: zend_function_entry = function_entry(c"opcov\\stop", stop, &TEST);
pub(crate) const RESET: zend_function_entry = function_entry(c"opcov\\reset", reset, &VOID);
pub(crate) const EXECUTABLE: zend_function_entry =
    function_entry(c"opcov\\executable", executable, &FILES);
pub(crate) const GET_STATUS: zend_function_entry =
    function_entry(c"opcov\\get_status", get_status, &ARRAY);
pub(crate) const GET_DEBUG_INFO: zend_function_entry =
    function_entry(c"opcov\\get_debug_info", get_debug_info, &FILE);
pub(crate) const WRITE_REPORT: zend_function_entry =
    function_entry(c"opcov\\write_report", write_report, &REPORT);

fn off_reason() -> Option<OffReason> {
    if let Some(disabled) = COLLECTION.disabled() {
        return Some(OffReason::Panic(disabled.clone()));
    }
    if threads::second_thread_started() {
        return Some(OffReason::SecondThread);
    }
    if let Some(startup) = STARTUP.get() {
        return Some(startup.clone());
    }
    return with_engine(|engine| {
        if engine.request.is_none() {
            return Some(OffReason::NotEnabled);
        }
        return engine.problem().cloned();
    })
    .unwrap_or(Some(OffReason::NotEnabledAtStartup));
}

fn refuse(reason: &OffReason) {
    php::throw(
        Exception::CannotCollect,
        &format!("{CANNOT_COLLECT}: {reason}"),
    );
}

impl Engine {
    fn selected_files(&self) -> impl Iterator<Item = FileId> {
        let mut selected: Vec<FileId> = self
            .request
            .iter()
            .flat_map(|request| return request.decisions.values())
            .filter_map(|verdict| {
                return match verdict {
                    Verdict::Measure(file) => Some(*file),
                    Verdict::Skip(_) | Verdict::Off => None,
                };
            })
            .collect();
        selected.sort_unstable();
        selected.dedup();
        return selected.into_iter();
    }

    fn path(&self, file: FileId) -> &str {
        return self
            .files
            .get(file)
            .map_or("", |record| return record.path.as_str());
    }
}

unsafe extern "C" fn ensure_can_collect(
    execute_data: *mut zend_execute_data,
    _return_value: *mut zval,
) {
    guard(EntryPoint::EnsureCanCollect, (), || {
        // SAFETY: the engine calls this handler with the frame of this call.
        if !unsafe { accepts_no_arguments(execute_data) } {
            return;
        }
        if let Some(reason) = off_reason() {
            refuse(&reason);
            return;
        }
        let empty_run = with_engine(|engine| {
            if engine.completed_cycles == 0 || engine.selected_files().next().is_some() {
                return None;
            }
            let skipped = engine
                .request
                .as_ref()
                .map_or(0, |request| return request.decisions.len());
            return Some(format!(
                "{CANNOT_COLLECT}: opcov.collect_paths selected none of the {skipped} files compiled during this run"
            ));
        })
        .flatten();
        if let Some(message) = empty_run {
            php::throw(Exception::CannotCollect, &message);
        }
    });
}

unsafe extern "C" fn start(execute_data: *mut zend_execute_data, _return_value: *mut zval) {
    guard(EntryPoint::Start, (), || {
        // SAFETY: the engine calls this handler with the frame of this call.
        if !unsafe { accepts_no_arguments(execute_data) } {
            return;
        }
        if let Some(reason) = off_reason() {
            refuse(&reason);
            return;
        }
        let started = with_engine(|engine| {
            if engine.collecting {
                return false;
            }
            engine.rearm(FiredList::Strays);
            engine.collecting = true;
            return true;
        });
        if started == Some(false) {
            php::throw(
                Exception::Logic,
                "opcov\\start() was called while collecting; call opcov\\stop() first",
            );
        }
    });
}

unsafe fn emit_hits(engine: &Engine, return_value: *mut zval) {
    let lines = &engine.scratch.lines;
    let mut files = 0_usize;
    let mut previous: Option<FileId> = None;
    for &(file, _) in lines {
        if previous != Some(file) {
            files = files.saturating_add(1);
            previous = Some(file);
        }
    }
    // SAFETY: the caller passes the engine's return slot; every child zval is moved into its parent.
    unsafe {
        php::array(return_value, files);
        let mut position = 0_usize;
        while let Some(&(file, _)) = lines.get(position) {
            let mut entry = zval::default();
            php::array(&raw mut entry, 1);
            let mut covered = zval::default();
            php::array(&raw mut covered, 0);
            while let Some(&(current, line)) = lines.get(position) {
                if current != file {
                    break;
                }
                let mut hit = php::bool_value(true);
                php::set_index(&raw mut covered, line.get(), &mut hit);
                position = position.saturating_add(1);
            }
            php::set(&raw mut entry, "lines", &mut covered);
            php::set(return_value, engine.path(file), &mut entry);
        }
    }
}

impl Engine {
    fn collect_hits(&mut self) {
        let mut lines = core::mem::take(&mut self.scratch.lines);
        lines.clear();
        for fired in &self.fired.hits {
            let Some(unit) = self.units.get(fired.unit) else {
                continue;
            };
            if let Some(probe) = unit.probes.get(fired.probe) {
                lines.push((unit.file, probe.line));
            }
        }
        lines.sort_unstable_by(|left, right| {
            return self
                .path(left.0)
                .cmp(self.path(right.0))
                .then(left.1.cmp(&right.1));
        });
        lines.dedup();
        self.scratch.lines = lines;
        let hits = core::mem::take(&mut self.fired.hits);
        self.fired.stopped.extend_from_slice(&hits);
        let mut emptied = hits;
        emptied.clear();
        self.fired.hits = emptied;
    }
}

enum TestArgument<'a> {
    Refused,
    Absent,
    Given(&'a str),
}

unsafe fn test_argument<'a>(execute_data: *mut zend_execute_data) -> TestArgument<'a> {
    let mut raw: *const c_char = core::ptr::null();
    let mut length = 0_usize;
    // SAFETY: the caller passes the frame of this call.
    if !unsafe { opcov_parse_optional_string(execute_data, &raw mut raw, &raw mut length) } {
        return TestArgument::Refused;
    }
    if raw.is_null() {
        return TestArgument::Absent;
    }
    // SAFETY: a PHP string argument's bytes stay alive during this call.
    let bytes = unsafe { core::slice::from_raw_parts(raw.cast::<u8>(), length) };
    let Ok(test) = str::from_utf8(bytes) else {
        php::throw(
            Exception::Value,
            "opcov\\stop(): Argument #1 ($test) must be valid UTF-8",
        );
        return TestArgument::Refused;
    };
    if test.is_empty() {
        php::throw(
            Exception::Value,
            "opcov\\stop(): Argument #1 ($test) must not be empty",
        );
        return TestArgument::Refused;
    }
    return TestArgument::Given(test);
}

unsafe extern "C" fn stop(execute_data: *mut zend_execute_data, return_value: *mut zval) {
    guard(EntryPoint::Stop, (), || {
        // SAFETY: the engine calls this handler with the frame of this call.
        let test = match unsafe { test_argument(execute_data) } {
            TestArgument::Refused => return,
            TestArgument::Absent => None,
            TestArgument::Given(test) => Some(test),
        };
        if let Some(reason) = off_reason() {
            with_engine(|engine| engine.collecting = false);
            refuse(&reason);
            return;
        }
        let stopped = with_engine(|engine| {
            if !engine.collecting {
                return false;
            }
            engine.collecting = false;
            engine.completed_cycles = engine.completed_cycles.saturating_add(1);
            engine.collect_hits();
            if let Some(test) = test {
                engine.attribution.attribute(test, &engine.scratch.lines);
            }
            // SAFETY: the engine passes this call's return slot.
            unsafe { emit_hits(engine, return_value) };
            return true;
        });
        if stopped != Some(true) {
            php::throw(
                Exception::Logic,
                "opcov\\stop() was called without opcov\\start()",
            );
        }
    });
}

unsafe extern "C" fn reset(execute_data: *mut zend_execute_data, _return_value: *mut zval) {
    guard(EntryPoint::Reset, (), || {
        // SAFETY: the engine calls this handler with the frame of this call.
        if !unsafe { accepts_no_arguments(execute_data) } {
            return;
        }
        let collecting = with_engine(|engine| {
            if engine.collecting {
                return true;
            }
            engine.rearm_all();
            return false;
        });
        if collecting == Some(true) {
            php::throw(
                Exception::Logic,
                "opcov\\reset() was called while collecting; call opcov\\stop() first",
            );
        }
    });
}

unsafe fn requested_paths<'a>(files: *mut HashTable) -> Result<Vec<&'a str>, u32> {
    // SAFETY: the caller passes the live argument array.
    let used = unsafe { zend::map_used(files) };
    let mut paths = Vec::new();
    for index in 0..used {
        // SAFETY: `index` is below nNumUsed of the live argument array.
        let value = unsafe { opcov_array_value_at(files, index) };
        if value.is_null() {
            continue;
        }
        let mut text: *const c_char = core::ptr::null();
        let mut length = 0_usize;
        // SAFETY: `value` is a live element of the argument array.
        if !unsafe { opcov_read_string(value, &raw mut text, &raw mut length) } {
            return Err(index);
        }
        // SAFETY: a PHP string's bytes stay alive while the argument array does, i.e. this call.
        let bytes = unsafe { core::slice::from_raw_parts(text.cast::<u8>(), length) };
        let Ok(path) = str::from_utf8(bytes) else {
            return Err(index);
        };
        paths.push(path);
    }
    return Ok(paths);
}

impl Engine {
    fn find_file(&self, requested: &str) -> Option<FileId> {
        if let Some(found) = self.files.find(requested) {
            return Some(found);
        }
        let resolved = fs::canonicalize(requested).ok()?;
        return self.files.find(resolved.to_str()?);
    }

    fn file_model(&self, file: FileId) -> Result<Entry, String> {
        let record = self
            .files
            .get(file)
            .ok_or_else(|| return format!("file {file:?} is not recorded"))?;
        let key_file: FilePath = record
            .path
            .parse()
            .map_err(|error| return format!("{}: {error}", record.path))?;
        return report::entry(record, &key_file, [])
            .map_err(|error| return format!("{}: {error}", record.path));
    }
}

unsafe fn emit_entry(target: *mut zval, entry: &Entry) -> Result<(), serde_json::Error> {
    // SAFETY: the caller passes a writable zval; every child is moved into its parent.
    unsafe {
        php::array(target, 2);
        let mut lines = zval::default();
        php::array(&raw mut lines, entry.lines.len());
        for (line, hits) in &entry.lines {
            let mut tests = zval::default();
            php::emit(&raw mut tests, &serde_json::to_value(hits)?);
            php::set_index(&raw mut lines, line.get(), &mut tests);
        }
        php::set(target, "lines", &mut lines);
        let mut functions = zval::default();
        php::array(&raw mut functions, entry.functions.len());
        for (key, function) in &entry.functions {
            let mut model = zval::default();
            php::emit(&raw mut model, &serde_json::to_value(function)?);
            php::set(&raw mut functions, &key.to_string(), &mut model);
        }
        php::set(target, "functions", &mut functions);
    }
    return Ok(());
}

unsafe extern "C" fn executable(execute_data: *mut zend_execute_data, return_value: *mut zval) {
    guard(EntryPoint::Executable, (), || {
        let mut files: *mut HashTable = core::ptr::null_mut();
        // SAFETY: the engine calls this handler with the frame of this call.
        if !unsafe { opcov_parse_array(execute_data, &raw mut files) } {
            return;
        }
        // SAFETY: the argument array is live during this call.
        let requested = match unsafe { requested_paths(files) } {
            Ok(requested) => requested,
            Err(position) => {
                php::throw(
                    Exception::Logic,
                    &format!(
                        "opcov\\executable() expects file paths; element {position} is not a UTF-8 string"
                    ),
                );
                return;
            }
        };
        let models = with_engine(|engine| {
            let mut files = core::mem::take(&mut engine.scratch.files);
            files.clear();
            files.extend(
                requested
                    .iter()
                    .filter_map(|path| return engine.find_file(path)),
            );
            files
                .sort_unstable_by(|left, right| return engine.path(*left).cmp(engine.path(*right)));
            files.dedup();
            let models = files
                .iter()
                .map(|&file| {
                    let model = engine.file_model(file)?;
                    return Ok((engine.path(file).to_owned(), model));
                })
                .collect::<Result<Vec<(String, Entry)>, String>>();
            engine.scratch.files = files;
            return models;
        })
        .unwrap_or_else(|| return Ok(Vec::new()));
        let models = match models {
            Ok(models) => models,
            Err(message) => {
                php::throw(
                    Exception::Runtime,
                    &format!("opcov\\executable() cannot describe {message}"),
                );
                return;
            }
        };
        // SAFETY: the engine passes this call's return slot; every child is moved into it.
        unsafe {
            php::array(return_value, models.len());
            for (path, model) in &models {
                let mut entry = zval::default();
                if let Err(error) = emit_entry(&raw mut entry, model) {
                    php::throw(
                        Exception::Runtime,
                        &format!("opcov\\executable() cannot describe {path}: {error}"),
                    );
                    return;
                }
                php::set(return_value, path, &mut entry);
            }
        }
    });
}

impl Engine {
    fn status_detail(&self) -> (Vec<Value>, Map<String, Value>, Value) {
        let selected = self
            .selected_files()
            .map(|file| return Value::String(self.path(file).to_owned()))
            .collect();
        let skipped = self
            .request
            .iter()
            .flat_map(|request| return &request.decisions)
            .filter_map(|(name, verdict)| {
                return match verdict {
                    Verdict::Skip(reason) => {
                        Some((name.clone(), Value::String(reason.to_string())))
                    }
                    Verdict::Measure(_) | Verdict::Off => None,
                };
            })
            .collect();
        let counters = json!({
            "units": self.units.count_live(),
            "probes_installed": self.counters.probes_installed,
            "probes_fired": self.counters.probes_fired,
            "probes_rearmed": self.counters.probes_rearmed,
            "jump_table_files": self.counters.jump_table_files,
        });
        return (selected, skipped, counters);
    }
}

fn status(engine: Option<&Engine>, reason: Option<&OffReason>) -> Value {
    let (selected, skipped, counters) = engine.map_or_else(
        || return (Vec::new(), Map::new(), json!({})),
        Engine::status_detail,
    );
    return json!({
        "enabled": ini::enabled(),
        "collecting": engine.is_some_and(|engine| return engine.collecting),
        "off_reason": reason.map(ToString::to_string),
        "selected_files": selected,
        "skipped_files": skipped,
        "counters": counters,
    });
}

unsafe extern "C" fn get_status(execute_data: *mut zend_execute_data, return_value: *mut zval) {
    guard(EntryPoint::GetStatus, (), || {
        // SAFETY: the engine calls this handler with the frame of this call.
        if !unsafe { accepts_no_arguments(execute_data) } {
            return;
        }
        let reason = off_reason();
        let emitted = with_engine(|engine| {
            engine.scratch.value = status(Some(engine), reason.as_ref());
            // SAFETY: the engine passes this call's return slot.
            unsafe { php::emit(return_value, &engine.scratch.value) };
            engine.scratch.value = Value::Null;
        });
        if emitted.is_none() {
            // SAFETY: as above; the engine is absent, so the status is built from the reason alone.
            unsafe { php::emit(return_value, &status(None, reason.as_ref())) };
        }
    });
}

impl Engine {
    fn debug_info(&self, requested: &str) -> Value {
        let decision = self
            .request
            .as_ref()
            .and_then(|request| return request.decisions.get(requested));
        let skip = match decision {
            Some(Verdict::Skip(reason)) => Value::String(reason.to_string()),
            Some(Verdict::Off) => Value::String(String::from("collection is off")),
            Some(Verdict::Measure(_)) | None => Value::Null,
        };
        let Some(file) = self.find_file(requested) else {
            return json!({"skip_reason": skip, "executable_lines": [], "functions": {}, "probes": []});
        };
        let Some(record) = self.files.get(file) else {
            return Value::Null;
        };
        let functions: Map<String, Value> = record
            .functions
            .iter()
            .map(|(key, analysis)| {
                return (
                    key.to_string(),
                    serde_json::to_value(analysis).unwrap_or_else(|error| {
                        return Value::String(format!("cannot serialise the analysis: {error}"));
                    }),
                );
            })
            .collect();
        let probes: Vec<Value> = self
            .units
            .live()
            .filter(|unit| return unit.file == file)
            .flat_map(|unit| {
                return unit.probes.iter().map(|probe| {
                    return json!({
                        "function": unit.function.to_string(),
                        "op": probe.op,
                        "line": probe.line.get(),
                        "armed": probe.armed.iter().map(Role::name).collect::<Vec<_>>(),
                    });
                });
            })
            .collect();
        return json!({
            "skip_reason": skip,
            "executable_lines": record.lines.iter().map(|line| return line.get()).collect::<Vec<_>>(),
            "functions": functions,
            "probes": probes,
        });
    }
}

unsafe extern "C" fn get_debug_info(execute_data: *mut zend_execute_data, return_value: *mut zval) {
    guard(EntryPoint::GetDebugInfo, (), || {
        let mut text: *const c_char = core::ptr::null();
        let mut length = 0_usize;
        // SAFETY: the engine calls this handler with the frame of this call.
        if !unsafe { opcov_parse_path(execute_data, &raw mut text, &raw mut length) } {
            return;
        }
        // SAFETY: the argument string stays alive during this call.
        let bytes = unsafe { core::slice::from_raw_parts(text.cast::<u8>(), length) };
        let requested = String::from_utf8_lossy(bytes);
        let emitted = with_engine(|engine| {
            engine.scratch.value = engine.debug_info(&requested);
            // SAFETY: the engine passes this call's return slot.
            unsafe { php::emit(return_value, &engine.scratch.value) };
            engine.scratch.value = Value::Null;
        });
        if emitted.is_none() {
            // SAFETY: as above.
            unsafe { php::array(return_value, 0) };
        }
    });
}

const WRITE_REPORT_NAME: &str = "opcov\\write_report()";
const ROOT_OPTION: &str = "root";
const ABSOLUTE_OPTION: &str = "absolute_paths";

#[derive(Debug, Default)]
struct ReportOptions {
    root: Option<String>,
    absolute: bool,
}

#[derive(Debug)]
struct Refusal {
    kind: Exception,
    message: String,
}

impl Refusal {
    fn new(kind: Exception, message: impl Into<String>) -> Self {
        return Self {
            kind,
            message: format!("{WRITE_REPORT_NAME}: {}", message.into()),
        };
    }

    fn option(kind: Exception, message: &str) -> Self {
        return Self::new(kind, format!("Argument #2 ($options) {message}"));
    }

    fn throw(&self) {
        php::throw(self.kind, &self.message);
    }
}

unsafe fn option_name(key: *const zend_string, number: zend_ulong) -> String {
    // SAFETY: the caller passes a live array key or null.
    return unsafe { zend::bytes(key) }.map_or_else(
        || return number.to_string(),
        |bytes| return format!("\"{}\"", String::from_utf8_lossy(bytes)),
    );
}

unsafe fn type_name(value: *const zval) -> String {
    // SAFETY: the caller passes a live zval; the engine returns a static type name.
    let name = unsafe { core::ffi::CStr::from_ptr(opcov_type_name(value)) };
    return name.to_string_lossy().into_owned();
}

unsafe fn root_option(value: *const zval) -> Result<String, Refusal> {
    let mut text: *const c_char = core::ptr::null();
    let mut length = 0_usize;
    // SAFETY: the caller passes a live array value.
    if !unsafe { opcov_read_string(value, &raw mut text, &raw mut length) } {
        return Err(Refusal::option(
            Exception::Type,
            &format!(
                "key \"{ROOT_OPTION}\" must be of type string, {} given",
                // SAFETY: as above.
                unsafe { type_name(value) }
            ),
        ));
    }
    // SAFETY: a PHP string's bytes stay alive while the argument array does, i.e. this call.
    let bytes = unsafe { core::slice::from_raw_parts(text.cast::<u8>(), length) };
    let root = str::from_utf8(bytes).map_err(|_utf8| {
        return Refusal::option(
            Exception::Value,
            &format!("key \"{ROOT_OPTION}\" must be valid UTF-8"),
        );
    })?;
    if root.is_empty() || root.contains('\0') {
        return Err(Refusal::option(
            Exception::Value,
            &format!("key \"{ROOT_OPTION}\" must be a non-empty path without NUL bytes"),
        ));
    }
    return Ok(root.to_owned());
}

unsafe fn report_options(options: *mut HashTable) -> Result<ReportOptions, Refusal> {
    let mut parsed = ReportOptions::default();
    if options.is_null() {
        return Ok(parsed);
    }
    // SAFETY: the caller passes the live argument array.
    let used = unsafe { zend::map_used(options) };
    for index in 0..used {
        let mut key: *mut zend_string = core::ptr::null_mut();
        let mut number: zend_ulong = 0;
        let mut value: *mut zval = core::ptr::null_mut();
        // SAFETY: `index` is below nNumUsed of the live argument array.
        if !unsafe {
            opcov_array_entry_at(
                options,
                index,
                &raw mut key,
                &raw mut number,
                &raw mut value,
            )
        } {
            continue;
        }
        // SAFETY: the key is null or a live string of the argument array.
        let name = unsafe { zend::bytes(key) };
        match name {
            Some(b"root") => {
                // SAFETY: `value` is a live element of the argument array.
                parsed.root = Some(unsafe { root_option(value) }?);
            }
            Some(b"absolute_paths") => {
                let mut flag = false;
                // SAFETY: as above.
                if !unsafe { opcov_read_bool(value, &raw mut flag) } {
                    return Err(Refusal::option(
                        Exception::Type,
                        &format!(
                            "key \"{ABSOLUTE_OPTION}\" must be of type bool, {} given",
                            // SAFETY: as above.
                            unsafe { type_name(value) }
                        ),
                    ));
                }
                parsed.absolute = flag;
            }
            _ => {
                return Err(Refusal::option(
                    Exception::Value,
                    &format!(
                        "contains the unknown key {}; the known keys are \"{ABSOLUTE_OPTION}\" and \"{ROOT_OPTION}\"",
                        // SAFETY: as above.
                        unsafe { option_name(key, number) }
                    ),
                ));
            }
        }
    }
    if parsed.absolute && parsed.root.is_some() {
        return Err(Refusal::option(
            Exception::Value,
            &format!("key \"{ROOT_OPTION}\" cannot be combined with \"{ABSOLUTE_OPTION}\" => true"),
        ));
    }
    return Ok(parsed);
}

fn working_directory() -> Result<PathBuf, Refusal> {
    let size = usize::try_from(MAXPATHLEN).unwrap_or(4096);
    let mut buffer = vec![0_u8; size];
    // SAFETY: the buffer holds `size` writable bytes; the engine writes a NUL-terminated path.
    let found = unsafe { opcov_getcwd(buffer.as_mut_ptr().cast(), size) };
    if found.is_null() {
        return Err(Refusal::new(
            Exception::Runtime,
            "cannot determine the working directory",
        ));
    }
    let text = core::ffi::CStr::from_bytes_until_nul(&buffer)
        .ok()
        .and_then(|path| return path.to_str().ok())
        .ok_or_else(|| {
            return Refusal::new(
                Exception::Runtime,
                "the working directory is not a valid UTF-8 path",
            );
        })?;
    return Ok(PathBuf::from(text));
}

fn report_paths(options: &ReportOptions, cwd: &Path) -> Result<PathBase, Refusal> {
    if options.absolute {
        return Ok(PathBase::Absolute);
    }
    let requested = options
        .root
        .as_deref()
        .map_or_else(|| return cwd.to_path_buf(), |root| return cwd.join(root));
    let resolved = fs::canonicalize(&requested).map_err(|error| {
        return Refusal::new(
            Exception::Runtime,
            format!(
                "cannot resolve the report root {}: {error}",
                requested.display()
            ),
        );
    })?;
    let text = resolved.to_str().ok_or_else(|| {
        return Refusal::new(
            Exception::Runtime,
            format!("the report root {} is not valid UTF-8", resolved.display()),
        );
    })?;
    let root = Root::new(text.to_owned()).map_err(|error| {
        return Refusal::new(Exception::Runtime, format!("invalid report root: {error}"));
    })?;
    return Ok(PathBase::Relative(root));
}

unsafe fn report_arguments(
    execute_data: *mut zend_execute_data,
) -> Option<Result<(PathBuf, PathBase), Refusal>> {
    let mut text: *const c_char = core::ptr::null();
    let mut length = 0_usize;
    let mut options: *mut HashTable = core::ptr::null_mut();
    // SAFETY: the caller passes the frame of this call.
    if !unsafe {
        opcov_parse_path_and_array(
            execute_data,
            &raw mut text,
            &raw mut length,
            &raw mut options,
        )
    } {
        return None;
    }
    // SAFETY: the path argument's bytes stay alive during this call.
    let bytes = unsafe { core::slice::from_raw_parts(text.cast::<u8>(), length) };
    let parsed = (|| {
        let path = str::from_utf8(bytes).map_err(|_utf8| {
            return Refusal::new(Exception::Value, "Argument #1 ($path) must be valid UTF-8");
        })?;
        if path.is_empty() {
            return Err(Refusal::new(
                Exception::Value,
                "Argument #1 ($path) must not be empty",
            ));
        }
        // SAFETY: the options array is null or live during this call.
        let options = unsafe { report_options(options) }?;
        let cwd = working_directory()?;
        let paths = report_paths(&options, &cwd)?;
        return Ok((cwd.join(path), paths));
    })();
    return Some(parsed);
}

unsafe extern "C" fn write_report(execute_data: *mut zend_execute_data, _return_value: *mut zval) {
    guard(EntryPoint::WriteReport, (), || {
        // SAFETY: the engine calls this handler with the frame of this call.
        let (target, paths) = match unsafe { report_arguments(execute_data) } {
            None => return,
            Some(Err(refusal)) => {
                refusal.throw();
                return;
            }
            Some(Ok(arguments)) => arguments,
        };
        if let Some(reason) = off_reason() {
            refuse(&reason);
            return;
        }
        let built = with_engine(|engine| {
            if engine.collecting {
                return Err(Refusal::new(
                    Exception::Logic,
                    "called while collecting; call opcov\\stop() first",
                ));
            }
            return engine.report(paths).map_err(|error| {
                return Refusal::new(
                    Exception::Runtime,
                    format!("cannot build the report for {}: {error}", target.display()),
                );
            });
        })
        .unwrap_or_else(|| {
            return Err(Refusal::new(
                Exception::CannotCollect,
                format!("{CANNOT_COLLECT}: {}", OffReason::NotEnabledAtStartup),
            ));
        });
        let report = match built {
            Ok(report) => report,
            Err(refusal) => {
                refusal.throw();
                return;
            }
        };
        if let Err(error) = write_atomically(&target, |out| return report.write(out)) {
            Refusal::new(
                Exception::Runtime,
                format!("cannot write the report to {}: {error}", target.display()),
            )
            .throw();
        }
    });
}
