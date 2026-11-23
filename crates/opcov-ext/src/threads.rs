use std::{
    sync::{
        Mutex, OnceLock, PoisonError,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, ThreadId},
};

use opcov_sys::ZTS;

static OWNER: OnceLock<ThreadId> = OnceLock::new();
static SECOND_THREAD: AtomicBool = AtomicBool::new(false);
static HANDLER_TABLE: Mutex<()> = Mutex::new(());

pub(crate) fn claim() -> bool {
    if !ZTS {
        return true;
    }
    let _table = HANDLER_TABLE.lock().unwrap_or_else(PoisonError::into_inner);
    let current = thread::current().id();
    if *OWNER.get_or_init(|| return current) == current {
        return true;
    }
    SECOND_THREAD.store(true, Ordering::Release);
    return false;
}

pub(crate) fn second_thread_started() -> bool {
    return SECOND_THREAD.load(Ordering::Acquire);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OwnerThread {
    Unclaimed,
    Current,
    Other,
}

impl OwnerThread {
    pub(crate) const fn name(self) -> &'static str {
        return match self {
            Self::Unclaimed => "unclaimed",
            Self::Current => "current",
            Self::Other => "other",
        };
    }
}

pub(crate) fn owner() -> OwnerThread {
    if !ZTS {
        return OwnerThread::Current;
    }
    return match OWNER.get() {
        None => OwnerThread::Unclaimed,
        Some(owner) if *owner == thread::current().id() => OwnerThread::Current,
        Some(_) => OwnerThread::Other,
    };
}

pub(crate) fn on_owner_thread() -> bool {
    return !ZTS || OWNER.get() == Some(&thread::current().id());
}

pub(crate) fn foreign() -> bool {
    return ZTS && second_thread_started() && !on_owner_thread();
}

pub(crate) fn with_handler_table<R>(change: impl FnOnce() -> R) -> Option<R> {
    let _table = HANDLER_TABLE.lock().unwrap_or_else(PoisonError::into_inner);
    if second_thread_started() {
        return None;
    }
    return Some(change());
}
