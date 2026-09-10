//! Process-scoped tokio runtime with a managed lifecycle.
//!
//! The runtime used to be built eagerly in `#[pymodule]` and stored in a
//! `OnceLock` for the life of the process. That shape had two failure modes:
//!
//! * **fork() (#159).** `import rqx` spawned the worker threads in the parent.
//!   A forked child inherits the `Runtime` struct but none of its threads or
//!   its I/O driver, so the first request in the child hangs (Linux) or aborts
//!   (macOS). Prefork servers — gunicorn, `uvicorn --workers` — import the app
//!   in the master and fork, which is exactly that sequence.
//!
//! * **Interpreter shutdown (#99).** Async results are delivered to Python by
//!   attaching to the interpreter from a tokio blocking thread. A future still
//!   in flight when the interpreter finalizes attaches after finalization has
//!   begun; depending on timing that thread is killed mid-write to `sys.stderr`
//!   and the final `flush_std_files()` aborts with `_enter_buffered_busy`, or
//!   it is parked forever inside pyo3's `HangThread` guard.
//!
//! `Runtime` fixes both by owning the tokio runtime instead of leaking it:
//!
//! * It is built on first use, not at import, so a process that only imports
//!   rqx (the prefork master) never creates runtime threads.
//! * Every access checks the caller's PID. A child that inherited a runtime
//!   from its parent gets a fresh one; the inherited corpse is leaked rather
//!   than dropped, because dropping it would try to join threads that do not
//!   exist in the child.
//! * An `atexit` hook (registered in `#[pymodule]`) shuts the runtime down
//!   before the interpreter finalizes. In-flight tasks are dropped, so nothing
//!   tries to attach to a finalizing interpreter, and running blocking tasks
//!   are given a bounded grace period to finish.
//!
//! The async bridge into pyo3-async-runtimes goes through [`Bridge`] rather
//! than `pyo3_async_runtimes::tokio`, because that module's runtime is a
//! set-once global with no reset, which cannot follow a PID change.

use std::cell::OnceCell;
use std::future::Future;
use std::pin::Pin;
use std::ptr;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};
use std::time::Duration;

use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use pyo3_async_runtimes::TaskLocals;
use pyo3_async_runtimes::generic::{self, ContextExt};
use tokio::runtime::{Builder, Handle, Runtime as TokioRuntime};
use tokio::task;

use crate::exceptions::RqxError;

/// How long `shutdown` waits for already-running blocking tasks (the ones
/// delivering results back to Python) before giving up on them.
const SHUTDOWN_GRACE: Duration = Duration::from_secs(5);

/// The process-wide runtime. Cheap to touch on the hot path: one atomic load,
/// one PID read, one `Handle` clone.
pub static RUNTIME: Runtime = Runtime::new();

/// One built tokio runtime and the PID it belongs to.
///
/// Slots are heap-allocated and intentionally never freed. A reader on
/// another thread may hold a `&Slot` at the moment a slot is replaced (after
/// fork) or retired (at shutdown), so freeing would be a use-after-free; the
/// cost is one small allocation per fork generation. The tokio runtime inside
/// is the only thing worth reclaiming, and `shutdown` takes it out through
/// the mutex.
struct Slot {
    pid: u32,
    handle: Handle,
    runtime: Mutex<Option<TokioRuntime>>,
}

pub struct Runtime {
    current: AtomicPtr<Slot>,
    closed: AtomicBool,
}

impl Runtime {
    const fn new() -> Self {
        Self {
            current: AtomicPtr::new(ptr::null_mut()),
            closed: AtomicBool::new(false),
        }
    }

    /// A handle to the runtime for the calling process, building it on first
    /// use and rebuilding it after `fork()`.
    pub fn handle(&self) -> PyResult<Handle> {
        if self.closed.load(Ordering::Acquire) {
            return Err(RqxError::new_err(
                "rqx runtime has been shut down (interpreter is exiting)",
            ));
        }
        let pid = std::process::id();
        let current = self.current.load(Ordering::Acquire);
        if let Some(slot) = Self::slot_for(current, pid) {
            return Ok(slot.handle.clone());
        }
        self.build(current, pid)
    }

    /// Run `fut` to completion on the runtime from a thread that is not
    /// inside one. Callers detach from the interpreter first. The outer
    /// `Err` is a runtime that could not be built or has been shut down.
    pub fn block_on<F: Future>(&self, fut: F) -> PyResult<F::Output> {
        Ok(self.handle()?.block_on(fut))
    }

    /// Convert a Rust future into an asyncio future, spawning it on the
    /// runtime for the calling process.
    pub fn future_into_py<'py, F, T>(&self, py: Python<'py>, fut: F) -> PyResult<Bound<'py, PyAny>>
    where
        F: Future<Output = PyResult<T>> + Send + 'static,
        T: for<'a> IntoPyObject<'a> + Send + 'static,
    {
        // Build (or rebuild after fork) here, where an error can be returned
        // as a Python exception, so that Bridge::spawn never has to.
        self.handle()?;
        generic::future_into_py::<Bridge, F, T>(py, fut)
    }

    /// Shut the runtime down ahead of interpreter finalization. Called from
    /// the `atexit` hook with the GIL released.
    ///
    /// Dropping the runtime cancels every spawned task, so no future
    /// completes — and no blocking thread attaches to Python — after this
    /// returns. Blocking tasks that are already running are waited for up to
    /// `SHUTDOWN_GRACE`; because they may be in the middle of a Python call,
    /// the caller must not hold the GIL.
    pub fn shutdown(&self) {
        self.closed.store(true, Ordering::Release);
        let current = self.current.swap(ptr::null_mut(), Ordering::AcqRel);
        let Some(slot) = Self::slot_for(current, std::process::id()) else {
            // Nothing built, or a runtime inherited across fork: its threads
            // are not ours to join, so it stays leaked.
            return;
        };
        let runtime = slot
            .runtime
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        if let Some(runtime) = runtime {
            runtime.shutdown_timeout(SHUTDOWN_GRACE);
        }
    }

    /// `os.register_at_fork(before=...)` hook, macOS only.
    ///
    /// Apple's Objective-C runtime aborts a forked child the first time it
    /// initializes a class the parent never touched ("+initialize may have
    /// been in progress in another thread when fork() was called"). reqwest's
    /// system-proxy lookup goes through SystemConfiguration / CoreFoundation,
    /// so a child whose parent only imported rqx dies building its first
    /// client. Building one throwaway client in the parent right before it
    /// forks initializes those classes on the parent's side. CPython moved
    /// `multiprocessing` to `spawn` on macOS for the same class of problem.
    ///
    /// Runs once per process: the classes stay initialized after the first
    /// build, and a prefork master forks many times.
    #[cfg(target_os = "macos")]
    pub fn prepare_fork(&self) {
        static ONCE: std::sync::Once = std::sync::Once::new();
        ONCE.call_once(|| {
            let _ = reqwest::Client::builder().build();
        });
    }

    /// The slot behind `current` if it exists and belongs to `pid`.
    fn slot_for<'a>(current: *mut Slot, pid: u32) -> Option<&'a Slot> {
        // SAFETY: slots are only ever created by `Box::leak` in `build` and
        // never freed, so any non-null pointer stored in `current` points to
        // a live `Slot` for the rest of the process.
        unsafe { current.as_ref() }.filter(|slot| slot.pid == pid)
    }

    /// Build a runtime for `pid` and install it, unless another thread won
    /// the race, in which case use theirs.
    fn build(&self, mut seen: *mut Slot, pid: u32) -> PyResult<Handle> {
        loop {
            // Default worker count (= num_cpus). H3 tried worker_threads(1)
            // but regressed throughput at c>=500 by ~20%; see
            // docs/improvements.md.
            let runtime = Builder::new_multi_thread()
                .enable_all()
                .thread_name("rqx-tokio-worker")
                .build()
                .map_err(|e| {
                    PyRuntimeError::new_err(format!("Error initializing Tokio Runtime: {e}"))
                })?;
            let handle = runtime.handle().clone();
            let slot = Box::into_raw(Box::new(Slot {
                pid,
                handle: handle.clone(),
                runtime: Mutex::new(Some(runtime)),
            }));
            let winner =
                match self
                    .current
                    .compare_exchange(seen, slot, Ordering::AcqRel, Ordering::Acquire)
                {
                    Ok(_) => return Ok(handle),
                    Err(winner) => winner,
                };
            // Another thread installed a slot first. Ours was never
            // published, so nobody else can observe it and it can be freed
            // outright.
            // SAFETY: `slot` came from Box::into_raw above and was never
            // shared.
            drop(unsafe { Box::from_raw(slot) });
            if let Some(theirs) = Self::slot_for(winner, pid) {
                return Ok(theirs.handle.clone());
            }
            // The winner is stale (a fork happened in between): retry
            // against it.
            seen = winner;
        }
    }
}

// ────────────────────────────────────────────────────────────────────────
// Bridge — the runtime pyo3-async-runtimes spawns onto.
//
// Mirrors `pyo3_async_runtimes::tokio::TokioRuntime` but resolves the tokio
// handle through `RUNTIME` on every spawn, so a process that forked after
// building the runtime spawns onto its own, not its parent's.
// ────────────────────────────────────────────────────────────────────────

struct Bridge;

tokio::task_local! {
    static TASK_LOCALS: OnceCell<TaskLocals>;
}

impl Bridge {
    fn handle() -> Handle {
        // Result delivery (`spawn_blocking`) runs on a tokio worker, where the
        // ambient handle is the runtime the task already belongs to. Using it
        // keeps delivery working during `shutdown` — after `closed` is set
        // but before the runtime is dropped — and skips a `getpid` per
        // completion. The initial `spawn` comes from a Python thread with no
        // ambient runtime; `Runtime::future_into_py` resolved (and, on error,
        // reported) the slot for this PID just before, so that path cannot
        // fail here.
        if let Ok(ambient) = Handle::try_current() {
            return ambient;
        }
        RUNTIME
            .handle()
            .expect("rqx runtime resolved by Runtime::future_into_py before spawn")
    }
}

impl generic::Runtime for Bridge {
    type JoinError = task::JoinError;
    type JoinHandle = task::JoinHandle<()>;

    fn spawn<F>(fut: F) -> Self::JoinHandle
    where
        F: Future<Output = ()> + Send + 'static,
    {
        Self::handle().spawn(fut)
    }

    fn spawn_blocking<F>(f: F) -> Self::JoinHandle
    where
        F: FnOnce() + Send + 'static,
    {
        Self::handle().spawn_blocking(f)
    }
}

impl ContextExt for Bridge {
    fn scope<F, R>(locals: TaskLocals, fut: F) -> Pin<Box<dyn Future<Output = R> + Send>>
    where
        F: Future<Output = R> + Send + 'static,
    {
        let cell = OnceCell::new();
        cell.set(locals).expect("fresh OnceCell");
        Box::pin(TASK_LOCALS.scope(cell, fut))
    }

    fn get_task_locals() -> Option<TaskLocals> {
        TASK_LOCALS
            .try_with(|c| c.get().cloned())
            .unwrap_or_default()
    }
}
