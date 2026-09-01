//! One WebView2 environment per process, shared by every editor view.
//!
//! The editor failed to open a SECOND time in every FL Studio session that
//! reopened it: `view.attached WebView2 failed: HRESULT(0x80070057)`, six
//! times running in the session that later crashed, each on a fresh view FL
//! had created a few hundred milliseconds after closing the last. The first
//! open always worked.
//!
//! WebView2 lets one process create exactly one environment per user-data
//! folder, and refuses a second one with E_INVALIDARG while the first is still
//! shutting down. On Windows wry's `WebContext` holds nothing but the path --
//! `WebContextImpl` is a unit struct -- so every `build_as_child` called
//! `CreateCoreWebView2EnvironmentWithOptions` afresh, and a host that reopens
//! the editor quickly asked for a second environment before the first had
//! gone. Sharing the `WebContext` changes nothing; the lever wry does expose
//! is `WebViewBuilderExtWindows::with_environment`, which is what WebView2 is
//! designed for: one environment, as many controllers as you like.
//!
//! So the first view to build successfully hands its environment here, every
//! later view builds on it, and `ExitDll` releases it before Wasmtime's
//! handlers come out -- it is a process-global thing registered from this DLL,
//! and those must leave in the exit hook, in order, while the code is mapped.
//!
//! Two things this is careful about. The environment is apartment-affine:
//! WebView2 must be driven from the thread that created it. Hosts open the
//! editor from their UI thread, but that is their contract and not ours to
//! assume, so the creating thread is recorded and an attach from any other
//! thread does NOT get the cached environment -- it creates its own, which is
//! the behaviour before this module existed, and says so in the log. And a
//! COM interface pointer is `!Send`; holding it in a static is sound only
//! under that same rule, which `Held`'s `unsafe impl` states and the thread
//! check enforces.

use std::sync::Mutex;

use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Environment;

use crate::diagnostic;

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetCurrentThreadId() -> u32;
}

fn current_thread() -> u32 {
    // SAFETY: no arguments, no pointers, always succeeds.
    unsafe { GetCurrentThreadId() }
}

struct Held {
    environment: ICoreWebView2Environment,
    thread: u32,
}

// SAFETY: the environment is only ever handed out on the thread recorded in
// `thread` (see `for_this_thread`), and only released from `release`, which
// `ExitDll` calls on the host's main thread after every view is gone. The
// `Mutex` serialises the bookkeeping; the thread check does the COM part.
unsafe impl Send for Held {}

static ENVIRONMENT: Mutex<Option<Held>> = Mutex::new(None);

/// The decision `for_this_thread` makes, kept pure so it can be tested
/// without COM: an environment may be reused only by the thread that made it.
pub fn may_reuse(cached_thread: Option<u32>, current: u32) -> bool {
    cached_thread == Some(current)
}

/// The shared environment, if one exists and this is its thread.
pub fn for_this_thread() -> Option<ICoreWebView2Environment> {
    let guard = ENVIRONMENT
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let current = current_thread();
    match guard.as_ref() {
        Some(held) if may_reuse(Some(held.thread), current) => {
            diagnostic::write(format!("webview environment reused thread={current}"));
            Some(held.environment.clone())
        }
        Some(held) => {
            diagnostic::write(format!(
                "webview environment NOT reused: created on thread {} but attached from {current}; creating a fresh one",
                held.thread
            ));
            None
        }
        None => None,
    }
}

/// Keeps the environment a freshly built view came with, if none is kept yet.
pub fn adopt(environment: ICoreWebView2Environment) {
    let mut guard = ENVIRONMENT
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if guard.is_none() {
        let thread = current_thread();
        diagnostic::write(format!("webview environment adopted thread={thread}"));
        *guard = Some(Held {
            environment,
            thread,
        });
    }
}

/// Lets the environment go. Called from `ExitDll`, before Wasmtime unloads.
pub fn release() -> &'static str {
    let mut guard = ENVIRONMENT
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    match guard.take() {
        Some(held) => {
            let current = current_thread();
            if held.thread != current {
                diagnostic::write(format!(
                    "webview environment released from thread {current}, not its own {}",
                    held.thread
                ));
            }
            drop(held);
            "released"
        }
        None => "none held",
    }
}

#[cfg(test)]
mod tests {
    use super::may_reuse;

    /// Only the thread that created the environment may drive it.
    #[test]
    fn only_the_creating_thread_reuses_the_environment() {
        assert!(may_reuse(Some(7), 7));
        assert!(!may_reuse(Some(7), 8));
        assert!(!may_reuse(None, 7));
    }
}
