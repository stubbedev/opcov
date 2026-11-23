use std::{
    any::Any,
    fmt,
    panic::{self, AssertUnwindSafe},
    sync::OnceLock,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EntryPoint {
    GetModule,
    Info,
    Version,
    #[cfg(feature = "engine-selftest")]
    EngineSelftest,
    #[cfg(feature = "engine-selftest")]
    SelftestPanic,
}

impl EntryPoint {
    const fn name(self) -> &'static str {
        return match self {
            Self::GetModule => "get_module",
            Self::Info => "module info",
            Self::Version => "opcov\\version()",
            #[cfg(feature = "engine-selftest")]
            Self::EngineSelftest => "opcov\\engine_selftest()",
            #[cfg(feature = "engine-selftest")]
            Self::SelftestPanic => "opcov\\selftest_panic()",
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

    pub(crate) fn guard<R>(&self, entry: EntryPoint, on_panic: R, body: impl FnOnce() -> R) -> R {
        return panic::catch_unwind(AssertUnwindSafe(body)).unwrap_or_else(|payload| {
            self.disabled.get_or_init(|| {
                return Disabled {
                    entry,
                    message: payload_message(payload.as_ref()),
                };
            });
            return on_panic;
        });
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

pub(crate) fn guard<R>(entry: EntryPoint, on_panic: R, body: impl FnOnce() -> R) -> R {
    return COLLECTION.guard(entry, on_panic, body);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn returns_the_body_result_and_stays_enabled_without_a_panic() {
        let collection = Collection::new();
        assert_eq!(collection.guard(EntryPoint::Version, 0, || return 7), 7);
        assert_eq!(collection.disabled(), None);
    }

    #[test]
    fn a_panic_returns_the_fallback_and_disables_collection_with_the_reason() {
        let collection = Collection::new();
        let result = collection.guard(EntryPoint::Info, "fallback", || panic!("boom {}", 42));
        assert_eq!(result, "fallback");
        assert_eq!(
            collection.disabled().map(ToString::to_string),
            Some(String::from("panic in module info: boom 42"))
        );
    }

    #[test]
    fn the_first_panic_reason_is_kept() {
        let collection = Collection::new();
        collection.guard(EntryPoint::Info, (), || panic!("first"));
        collection.guard(EntryPoint::Version, (), || panic!("second"));
        assert_eq!(
            collection.disabled().map(ToString::to_string),
            Some(String::from("panic in module info: first"))
        );
    }

    #[test]
    fn a_non_string_payload_is_named() {
        let collection = Collection::new();
        collection.guard(EntryPoint::Version, (), || panic::panic_any(5_u8));
        assert_eq!(
            collection.disabled().map(ToString::to_string),
            Some(String::from(
                "panic in opcov\\version(): panic payload is not a string"
            ))
        );
    }
}
