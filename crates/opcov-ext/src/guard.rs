use core::cell::Cell;
use std::{
    any::Any,
    fmt,
    panic::{self, AssertUnwindSafe},
    sync::OnceLock,
};

use opcov_sys::opcov_bailout;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EntryPoint {
    GetModule,
    Info,
    Version,
    ModuleStartup,
    ModuleShutdown,
    RequestStartup,
    RequestShutdown,
    CompileFile,
    CompileString,
    OpArrayDtor,
    ExceptionHook,
    Probe,
    EnsureCanCollect,
    Start,
    Stop,
    Reset,
    Executable,
    GetStatus,
    GetDebugInfo,
    WriteReport,
    #[cfg(feature = "engine-selftest")]
    EngineSelftest,
    #[cfg(feature = "engine-selftest")]
    SelftestPanic,
    #[cfg(feature = "engine-selftest")]
    SelftestForceWatch,
    #[cfg(feature = "engine-selftest")]
    SelftestPanicInProbe,
    #[cfg(feature = "engine-selftest")]
    SelftestFailSnapshot,
}

impl EntryPoint {
    const fn name(self) -> &'static str {
        return match self {
            Self::GetModule => "get_module",
            Self::Info => "module info",
            Self::Version => "opcov\\version()",
            Self::ModuleStartup => "module startup",
            Self::ModuleShutdown => "module shutdown",
            Self::RequestStartup => "request startup",
            Self::RequestShutdown => "request shutdown",
            Self::CompileFile => "the zend_compile_file hook",
            Self::CompileString => "the zend_compile_string hook",
            Self::OpArrayDtor => "the op_array destructor",
            Self::ExceptionHook => "the zend_throw_exception_hook",
            Self::Probe => "the probe callback",
            Self::EnsureCanCollect => "opcov\\ensure_can_collect()",
            Self::Start => "opcov\\start()",
            Self::Stop => "opcov\\stop()",
            Self::Reset => "opcov\\reset()",
            Self::Executable => "opcov\\executable()",
            Self::GetStatus => "opcov\\get_status()",
            Self::GetDebugInfo => "opcov\\get_debug_info()",
            Self::WriteReport => "opcov\\write_report()",
            #[cfg(feature = "engine-selftest")]
            Self::EngineSelftest => "opcov\\engine_selftest()",
            #[cfg(feature = "engine-selftest")]
            Self::SelftestPanic => "opcov\\selftest_panic()",
            #[cfg(feature = "engine-selftest")]
            Self::SelftestForceWatch => "opcov\\selftest_force_watch()",
            #[cfg(feature = "engine-selftest")]
            Self::SelftestPanicInProbe => "opcov\\selftest_panic_in_probe()",
            #[cfg(feature = "engine-selftest")]
            Self::SelftestFailSnapshot => "opcov\\selftest_fail_snapshot()",
        };
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Disabled {
    entry: EntryPoint,
    message: String,
}

impl fmt::Display for Disabled {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        return write!(f, "panic in {}: {}", self.entry.name(), self.message);
    }
}

#[derive(Debug, Default)]
pub(crate) struct Collection {
    disabled: OnceLock<Disabled>,
}

impl Collection {
    pub(crate) const fn new() -> Self {
        return Self {
            disabled: OnceLock::new(),
        };
    }

    pub(crate) fn disabled(&self) -> Option<&Disabled> {
        return self.disabled.get();
    }

    pub(crate) fn run<R>(&self, entry: EntryPoint, body: impl FnOnce() -> R) -> Option<R> {
        return panic::catch_unwind(AssertUnwindSafe(body))
            .map_err(|payload| {
                self.disabled.get_or_init(|| {
                    return Disabled {
                        entry,
                        message: payload_message(payload.as_ref()),
                    };
                });
            })
            .ok();
    }
}

fn payload_message(payload: &(dyn Any + Send)) -> String {
    if let Some(message) = payload.downcast_ref::<&'static str>() {
        return (*message).to_owned();
    }
    if let Some(message) = payload.downcast_ref::<String>() {
        return message.clone();
    }
    return String::from("panic payload is not a string");
}

pub(crate) static COLLECTION: Collection = Collection::new();

thread_local! {
    static DEPTH: Cell<u32> = const { Cell::new(0) };
    static BAILED_OUT: Cell<bool> = const { Cell::new(false) };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Bailout;

pub(crate) type Caught<T> = Result<T, Bailout>;

pub(crate) fn record(completed: bool) -> bool {
    if !completed {
        BAILED_OUT.set(true);
    }
    return completed;
}

pub(crate) fn caught(completed: bool) -> Caught<()> {
    return if record(completed) {
        Ok(())
    } else {
        Err(Bailout)
    };
}

fn resume_at_outermost() {
    if DEPTH.get() == 0 && BAILED_OUT.replace(false) {
        // SAFETY: a fatal error was caught inside a call into the engine and every Rust frame between
        // that call and the engine's entry into opcov has returned, so the bailout continues across
        // no frame that owns a value with a destructor.
        unsafe { opcov_bailout() };
    }
}

pub(crate) fn resume_bailout() {
    BAILED_OUT.set(true);
    resume_at_outermost();
}

pub(crate) fn guard<R: Copy>(entry: EntryPoint, on_panic: R, body: impl FnOnce() -> R) -> R {
    let depth = DEPTH.get();
    DEPTH.set(depth.saturating_add(1));
    let result = COLLECTION.run(entry, body).unwrap_or_else(|| {
        COLLECTION.run(entry, crate::engine::after_panic);
        return on_panic;
    });
    DEPTH.set(depth);
    resume_at_outermost();
    return result;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn returns_the_body_result_and_stays_enabled_without_a_panic() {
        let collection = Collection::new();
        assert_eq!(collection.run(EntryPoint::Version, || return 7), Some(7));
        assert_eq!(collection.disabled(), None);
    }

    #[test]
    fn a_panic_returns_nothing_and_disables_collection_with_the_reason() {
        let collection = Collection::new();
        let result = collection.run(EntryPoint::Info, || -> u8 { panic!("boom {}", 42) });
        assert_eq!(result, None);
        assert_eq!(
            collection.disabled().map(ToString::to_string),
            Some(String::from("panic in module info: boom 42"))
        );
    }

    #[test]
    fn the_first_panic_reason_is_kept() {
        let collection = Collection::new();
        collection.run(EntryPoint::Info, || panic!("first"));
        collection.run(EntryPoint::Version, || panic!("second"));
        assert_eq!(
            collection.disabled().map(ToString::to_string),
            Some(String::from("panic in module info: first"))
        );
    }

    #[test]
    fn a_non_string_payload_is_named() {
        let collection = Collection::new();
        collection.run(EntryPoint::Version, || panic::panic_any(5_u8));
        assert_eq!(
            collection.disabled().map(ToString::to_string),
            Some(String::from(
                "panic in opcov\\version(): panic payload is not a string"
            ))
        );
    }
}
