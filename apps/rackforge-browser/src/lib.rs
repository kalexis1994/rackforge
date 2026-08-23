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
pub mod controller;
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

/// Leaves this host's declared capabilities in the response buffer, so a page
/// can hide what it cannot do and CI can probe what it claims. Returns the
/// response length.
#[unsafe(no_mangle)]
pub extern "C" fn rf_capabilities() -> i32 {
    use rackforge_host_capabilities::{Capability, Host};

    let capabilities: Vec<serde_json::Value> = Capability::ALL
        .iter()
        .map(|capability| {
            let support = Host::Browser
                .support(*capability)
                .expect("every capability is declared for every host");
            serde_json::json!({
                "id": capability.id(),
                "summary": capability.summary(),
                "supported": support.is_supported(),
                "state": support.mark(),
                "reason": support.reason(),
            })
        })
        .collect();
    publish(&serde_json::json!({ "ok": true, "capabilities": capabilities }))
}

/// Leaves the loaded plugin catalog in the response buffer as JSON, in the
/// shape the interface expects from a RackForge gateway. Returns its length.
#[unsafe(no_mangle)]
pub extern "C" fn rf_plugin_catalog() -> i32 {
    HOST.with(|host| match host.borrow().as_ref() {
        Some(host) => publish(&serde_json::json!({
            "ok": true,
            "catalog": host.plugin_catalog(),
        })),
        None => publish(&serde_json::json!({
            "ok": false,
            "error": "the RackForge host is not open",
        })),
    })
}

/// Removes an installed plugin and reloads the session without it. Returns the
/// response length.
///
/// # Safety
///
/// `pointer` must address `length` readable bytes of UTF-8.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_uninstall_plugin(pointer: *const u8, length: usize) -> i32 {
    // SAFETY: forwarded from this function's contract.
    let bytes = unsafe { std::slice::from_raw_parts(pointer, length) };
    let request = match serde_json::from_slice::<UninstallRequest>(bytes) {
        Ok(request) => request,
        Err(error) => {
            return publish(&serde_json::json!({
                "ok": false,
                "error": format!("unreadable removal request: {error}"),
            }));
        }
    };
    let options = rackforge_repository::PluginUserDataRemovalOptions {
        presets: request.delete_presets,
        plugin_data: request.delete_plugin_data,
    };
    HOST.with(|host| match host.borrow_mut().as_mut() {
        Some(host) => match host.uninstall_package(&request.plugin_id, options) {
            Ok(removed) => publish(&serde_json::json!({ "ok": true, "removed": removed })),
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

/// Enables or disables an installed plugin and reloads the browser session.
/// Returns the response length.
///
/// # Safety
///
/// `pointer` must address `length` readable bytes of UTF-8 JSON.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_set_plugin_active(pointer: *const u8, length: usize) -> i32 {
    // SAFETY: forwarded from this function's contract.
    let bytes = unsafe { std::slice::from_raw_parts(pointer, length) };
    let request = match serde_json::from_slice::<PluginActivationRequest>(bytes) {
        Ok(request) => request,
        Err(error) => {
            return publish(&serde_json::json!({
                "ok": false,
                "error": format!("unreadable activation request: {error}"),
            }));
        }
    };
    HOST.with(|host| match host.borrow_mut().as_mut() {
        Some(host) => match host.set_package_active(&request.plugin_id, request.active) {
            Ok(changed) => publish(&serde_json::json!({ "ok": true, "changed": changed })),
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

/// Installs a file into a plugin's private storage. The request is a JSON
/// header followed by a newline and then the file's bytes, so one call carries
/// both without a second allocation. Returns the response length.
///
/// # Safety
///
/// `pointer` must address `length` readable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_import_resource(pointer: *const u8, length: usize) -> i32 {
    // SAFETY: forwarded from this function's contract.
    let bytes = unsafe { std::slice::from_raw_parts(pointer, length) };
    let Some(split) = bytes.iter().position(|byte| *byte == b'\n') else {
        return publish(&serde_json::json!({
            "ok": false,
            "error": "the resource request has no header",
        }));
    };
    let header = match serde_json::from_slice::<ResourceImport>(&bytes[..split]) {
        Ok(header) => header,
        Err(error) => {
            return publish(&serde_json::json!({
                "ok": false,
                "error": format!("unreadable resource request: {error}"),
            }));
        }
    };
    let payload = &bytes[split + 1..];
    HOST.with(|host| match host.borrow_mut().as_mut() {
        Some(host) => match host.import_resource(&header.plugin_id, &header.resource_id, payload) {
            Ok(imported) => publish(&serde_json::json!({ "ok": true, "imported": imported })),
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

/// Reports which declared resources a plugin has installed. Returns the
/// response length.
///
/// # Safety
///
/// `pointer` must address `length` readable bytes of UTF-8.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rf_resource_status(pointer: *const u8, length: usize) -> i32 {
    // SAFETY: forwarded from this function's contract.
    let bytes = unsafe { std::slice::from_raw_parts(pointer, length) };
    let Ok(plugin_id) = std::str::from_utf8(bytes) else {
        return publish(&serde_json::json!({
            "ok": false,
            "error": "the plugin id is not valid UTF-8",
        }));
    };
    HOST.with(|host| match host.borrow().as_ref() {
        Some(host) => match host.resource_status(plugin_id) {
            Ok(resources) => publish(&serde_json::json!({ "ok": true, "resources": resources })),
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

/// Which plugin resource an imported file belongs to.
#[derive(serde::Deserialize)]
struct ResourceImport {
    plugin_id: String,
    resource_id: String,
}

/// What the interface asks to be removed along with a plugin's package.
#[derive(serde::Deserialize)]
struct UninstallRequest {
    plugin_id: String,
    #[serde(default)]
    delete_presets: bool,
    #[serde(default)]
    delete_plugin_data: bool,
}

#[derive(serde::Deserialize)]
struct PluginActivationRequest {
    plugin_id: String,
    active: bool,
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

/// Announces that the page has opened the certified KeyLab MIDI input/output
/// pair with SysEx permission.
#[unsafe(no_mangle)]
pub extern "C" fn rf_controller_connect() {
    HOST.with(|host| {
        if let Some(host) = host.borrow_mut().as_mut() {
            host.controller_connect();
        }
    });
}

/// Restores the keyboard before the browser releases its MIDI output.
#[unsafe(no_mangle)]
pub extern "C" fn rf_controller_disconnect() {
    HOST.with(|host| {
        if let Some(host) = host.borrow_mut().as_mut() {
            host.controller_disconnect();
        }
    });
}

/// Delivers MIDI known to originate at the certified KeyLab endpoint.
#[unsafe(no_mangle)]
pub extern "C" fn rf_push_controller_midi(status: u8, data1: u8, data2: u8, length: u8) {
    HOST.with(|host| {
        if let Some(host) = host.borrow_mut().as_mut() {
            host.push_controller_midi([status, data1, data2], length.clamp(1, 3));
        }
    });
}

#[unsafe(no_mangle)]
pub extern "C" fn rf_controller_output_pending() -> u32 {
    HOST.with(|host| {
        host.borrow()
            .as_ref()
            .is_some_and(BrowserHost::controller_output_pending) as u32
    })
}

/// Returns and clears the pending Web MIDI output plan.
#[unsafe(no_mangle)]
pub extern "C" fn rf_controller_output() -> i32 {
    HOST.with(|host| match host.borrow_mut().as_mut() {
        Some(host) => publish(&host.drain_controller_output()),
        None => publish(&Vec::<controller::BrowserControllerOutput>::new()),
    })
}

/// Applies the one setting currently exposed by the bundled controller.
#[unsafe(no_mangle)]
pub extern "C" fn rf_controller_set_color(red: u8, green: u8, blue: u8) {
    HOST.with(|host| {
        if let Some(host) = host.borrow_mut().as_mut() {
            host.set_controller_color([red, green, blue]);
        }
    });
}

/// Describes the bundled controller from its embedded `.rfcontroller`
/// manifest, keeping the web catalog version in lockstep with native builds.
#[unsafe(no_mangle)]
pub extern "C" fn rf_controller_catalog() -> i32 {
    HOST.with(|host| match host.borrow().as_ref() {
        Some(host) => publish(&host.controller_catalog()),
        None => publish(&serde_json::json!({
            "controllers": [],
            "error": "the RackForge host is not open",
        })),
    })
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
        format!(
            "{{\"status\":\"error\",\"code\":\"internal\",\"message\":{:?}}}",
            error.to_string()
        )
        .into_bytes()
    });
    let length = encoded.len();
    RESPONSE.with(|response| *response.borrow_mut() = encoded);
    i32::try_from(length).unwrap_or(i32::MAX)
}
