//! The exported surface a web page drives RackForge through.
//!
//! RackForge already defines a plain WebAssembly ABI for its portable plugins,
//! so the host uses the same shape rather than inventing a second convention or
//! pulling in a bindings generator: the embedder allocates through the module,
//! passes UTF-8 JSON in, and reads UTF-8 JSON out. Audio is the one exception —
//! it is exchanged as a pointer into linear memory, because a block is copied
//! by the audio callback and must not be re-encoded.
//!
//! The JSON on both sides is `ControlRequest` and `ControlResponse`, the same
//! protocol the native gateway speaks over its control socket.

pub mod audio;
pub mod host;

use host::BrowserHost;
use rackforge_control_api::{ControlErrorCode, ControlRequest, ControlResponse};
use std::cell::RefCell;

thread_local! {
    /// The page runs one host. It is created by [`rf_open`] and lives until the
    /// tab does.
    static HOST: RefCell<Option<BrowserHost>> = const { RefCell::new(None) };
    /// The most recent response, kept alive until the embedder has copied it.
    static RESPONSE: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

/// Reserves `length` bytes the embedder may write a request into.
///
/// # Safety
///
/// The returned pointer must be released with [`rf_free`] and the same length.
#[unsafe(no_mangle)]
pub extern "C" fn rf_alloc(length: usize) -> *mut u8 {
    let mut buffer = Vec::<u8>::with_capacity(length);
    let pointer = buffer.as_mut_ptr();
    std::mem::forget(buffer);
    pointer
}

/// Releases a buffer produced by [`rf_alloc`].
///
/// # Safety
///
/// `pointer` must come from [`rf_alloc`] with the same `length`, and must not
/// be used afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_free(pointer: *mut u8, length: usize) {
    if pointer.is_null() {
        return;
    }
    // SAFETY: forwarded from this function's contract.
    drop(unsafe { Vec::from_raw_parts(pointer, 0, length) });
}

/// Boots the host against the storage the embedder has already mounted.
///
/// Returns the length of a JSON status document left in the response buffer,
/// which reports either the warnings collected while loading plugins or the
/// reason the host could not start.
///
/// # Safety
///
/// Must be called once, before any other entry point.
#[unsafe(no_mangle)]
pub extern "C" fn rf_open(sample_rate_hz: f64, maximum_frames: u32, channels: u32) -> i32 {
    match BrowserHost::open(sample_rate_hz, maximum_frames, channels) {
        Ok(opened) => {
            let warnings = opened.warnings().to_vec();
            HOST.with(|host| *host.borrow_mut() = Some(opened));
            publish(&serde_json::json!({ "ok": true, "warnings": warnings }))
        }
        Err(error) => publish(&serde_json::json!({
            "ok": false,
            "error": format!("{error:#}"),
        })),
    }
}

/// Handles one `ControlRequest` and leaves its `ControlResponse` in the
/// response buffer. Returns the response length in bytes.
///
/// # Safety
///
/// `pointer` must address `length` readable bytes of UTF-8 JSON.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_request(pointer: *const u8, length: usize) -> i32 {
    // SAFETY: forwarded from this function's contract.
    let bytes = unsafe { std::slice::from_raw_parts(pointer, length) };
    let response = match serde_json::from_slice::<ControlRequest>(bytes) {
        Ok(request) => HOST.with(|host| match host.borrow_mut().as_mut() {
            Some(host) => host.handle(request),
            None => error_response("the RackForge host is not open"),
        }),
        Err(error) => error_response(format!("unreadable control request: {error}")),
    };
    publish(&response)
}

/// Validates a `.rfplugin` and leaves a JSON description of it in the response
/// buffer, without installing anything. Returns the response length.
///
/// # Safety
///
/// `pointer` must address `length` readable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_inspect_plugin(pointer: *const u8, length: usize) -> i32 {
    // SAFETY: forwarded from this function's contract.
    let archive = unsafe { std::slice::from_raw_parts(pointer, length) };
    HOST.with(|host| match host.borrow().as_ref() {
        Some(host) => match host.inspect_package(archive) {
            Ok(preview) => publish(&serde_json::json!({ "ok": true, "preview": preview })),
            Err(error) => publish(&serde_json::json!({
                "ok": false,
                "error": format!("{error:#}"),
            })),
        },
        None => publish(&serde_json::json!({
            "ok": false,
            "error": "the RackForge host is not open",
        })),
    })
}

/// Installs a `.rfplugin` into the page's plugin store and reloads the
/// session over it. Returns the response length.
///
/// # Safety
///
/// `pointer` must address `length` readable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_install_plugin(pointer: *const u8, length: usize) -> i32 {
    // SAFETY: forwarded from this function's contract.
    let archive = unsafe { std::slice::from_raw_parts(pointer, length) };
    HOST.with(|host| match host.borrow_mut().as_mut() {
        Some(host) => match host.install_package(archive) {
            Ok(installed) => publish(&serde_json::json!({ "ok": true, "installed": installed })),
            Err(error) => publish(&serde_json::json!({
                "ok": false,
                "error": format!("{error:#}"),
            })),
        },
        None => publish(&serde_json::json!({
            "ok": false,
            "error": "the RackForge host is not open",
        })),
    })
}

/// Returns a pointer to the buffer written by the most recent [`rf_open`] or
/// [`rf_request`] call.
#[unsafe(no_mangle)]
pub extern "C" fn rf_response_ptr() -> *const u8 {
    RESPONSE.with(|response| response.borrow().as_ptr())
}

/// Queues one live MIDI message for the next audio block.
#[unsafe(no_mangle)]
pub extern "C" fn rf_push_midi(frame: u32, status: u8, data1: u8, data2: u8, length: u8) {
    HOST.with(|host| {
        if let Some(host) = host.borrow_mut().as_mut() {
            host.push_midi(frame, [status, data1, data2], length.clamp(1, 3));
        }
    });
}

/// Renders one interleaved block and returns a pointer to it. The block holds
/// `frames * channels` `f32` samples and stays valid until the next render.
#[unsafe(no_mangle)]
pub extern "C" fn rf_render(frames: u32) -> *const f32 {
    HOST.with(|host| match host.borrow_mut().as_mut() {
        Some(host) => host.render(frames).as_ptr(),
        None => std::ptr::null(),
    })
}

fn error_response(message: impl Into<String>) -> ControlResponse {
    ControlResponse::Error {
        code: ControlErrorCode::Internal,
        message: message.into(),
        current_revision: None,
    }
}

/// Serializes a value into the response buffer and reports its length.
fn publish<T: serde::Serialize>(value: &T) -> i32 {
    let encoded = serde_json::to_vec(value).unwrap_or_else(|error| {
        format!("{{\"status\":\"error\",\"code\":\"internal\",\"message\":{:?}}}", error.to_string())
            .into_bytes()
    });
    let length = encoded.len();
    RESPONSE.with(|response| *response.borrow_mut() = encoded);
    i32::try_from(length).unwrap_or(i32::MAX)
}
