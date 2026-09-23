use super::{RackForgeControllerShared, VstPluginModel, diagnostic, engine::VstParameterValue};
use include_dir::{Dir, include_dir};
use rackforge_core::host_bridge::{HOST_PROTOCOL, PROTOCOL_PLACEHOLDER};
use rackforge_resource_api::{BindResourceRequest, ResourceBrowser};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{borrow::Cow, path::Component};
use wry::http::{Request, Response, StatusCode, header};

static WEB_ASSETS: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/../../web/dist");

const INITIALIZATION_SCRIPT_TEMPLATE: &str = r#"
(() => {
  window.__RACKFORGE_HOST_SHELL__ = 'vst3';
  const protocol = '__RACKFORGE_HOST_PROTOCOL__';
  const publish = message => window.postMessage(message, '*');
  window.RackForgeNativeHost = {
    postMessage(payload) {
      fetch('/__rackforge_vst_bridge__', {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: payload,
      })
        .then(async response => {
          const body = await response.json();
          if (!response.ok) throw new Error(body.error || `VST host returned ${response.status}`);
          publish(body.response);
          for (const event of body.events || []) publish(event);
        })
        .catch(error => {
          let requestId;
          try { requestId = JSON.parse(payload).request_id; } catch (_) {}
          if (!requestId) return;
          publish({
            protocol,
            kind: 'response',
            request_id: requestId,
            ok: false,
            status: 500,
            error: error instanceof Error ? error.message : String(error),
          });
        });
    },
  };
})();
"#;

/// The bridge this host injects, stamped with the shared protocol.
///
/// The script is JavaScript and full of braces, so the value is written in
/// by name rather than by `format!` — see
/// [`PROTOCOL_PLACEHOLDER`](rackforge_core::host_bridge::PROTOCOL_PLACEHOLDER).
pub fn initialization_script() -> String {
    INITIALIZATION_SCRIPT_TEMPLATE.replace(PROTOCOL_PLACEHOLDER, HOST_PROTOCOL)
}

#[derive(Deserialize)]
struct NativeRequest {
    request_id: String,
    method: String,
    #[serde(default)]
    params: Value,
}

pub fn protocol_response(
    request: &Request<Vec<u8>>,
    shared: &RackForgeControllerShared,
) -> Response<Cow<'static, [u8]>> {
    let path = request.uri().path();
    if path == "/__rackforge_vst_bridge__" {
        return bridge_response(request.body(), shared);
    }
    if path.starts_with("/plugin-assets/") {
        return plugin_asset(path, shared);
    }
    static_asset(path)
}

fn bridge_response(
    bytes: &[u8],
    shared: &RackForgeControllerShared,
) -> Response<Cow<'static, [u8]>> {
    let request = match serde_json::from_slice::<NativeRequest>(bytes) {
        Ok(request) => request,
        Err(error) => return json_error(StatusCode::BAD_REQUEST, error.to_string()),
    };
    let result = handle_native_request(&request, shared);
    let body = match result {
        Ok((result, events)) => json!({
            "response": {
                "protocol": HOST_PROTOCOL,
                "kind": "response",
                "request_id": request.request_id,
                "ok": true,
                "result": result,
            },
            "events": events,
        }),
        Err(message) => json!({
            "response": {
                "protocol": HOST_PROTOCOL,
                "kind": "response",
                "request_id": request.request_id,
                "ok": false,
                "status": 409,
                "error": message,
            },
            "events": [],
        }),
    };
    json_response(StatusCode::OK, body)
}

fn handle_native_request(
    request: &NativeRequest,
    shared: &RackForgeControllerShared,
) -> Result<(Value, Vec<Value>), String> {
    match request.method.as_str() {
        "http.request" => handle_http_request(&request.params, shared),
        "session.connect" => Ok((
            Value::Null,
            vec![
                session_event("open", None),
                session_message(snapshot(shared)?),
            ],
        )),
        "session.send" => {
            let payload = request
                .params
                .get("payload")
                .and_then(Value::as_str)
                .ok_or_else(|| "VST session payload is missing".to_owned())?;
            let command: Value = serde_json::from_str(payload)
                .map_err(|error| format!("invalid VST session request: {error}"))?;
            let messages = handle_session_command(&command, shared)?;
            Ok((
                Value::Null,
                messages.into_iter().map(session_message).collect(),
            ))
        }
        "session.close" => Ok((Value::Null, vec![session_event("close", None)])),
        "plugin.select_sound" => {
            validate_instance(&request.params)?;
            let sound_id = request
                .params
                .get("sound_id")
                .and_then(Value::as_str)
                .ok_or_else(|| "VST sound selection has no sound id".to_owned())?;
            shared
                .apply_preset_from_ui(sound_id)
                .ok_or_else(|| format!("unknown VST sound {sound_id:?}"))?;
            Ok((
                json!({ "sound_id": sound_id }),
                vec![session_message(snapshot(shared)?)],
            ))
        }
        "ui.route" => {
            let path = request
                .params
                .get("path")
                .and_then(Value::as_str)
                .ok_or_else(|| "VST UI route has no path".to_owned())?;
            shared.set_ui_route(path)?;
            Ok((json!({ "path": shared.ui_route() }), Vec::new()))
        }
        "ui.haptic" => Ok((Value::Null, Vec::new())),
        method => Err(format!("RackForge VST3 does not support {method} yet")),
    }
}

fn handle_http_request(
    params: &Value,
    shared: &RackForgeControllerShared,
) -> Result<(Value, Vec<Value>), String> {
    let path = params
        .get("path")
        .and_then(Value::as_str)
        .ok_or_else(|| "VST HTTP request has no path".to_owned())?;
    let method = params
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or("GET")
        .to_ascii_uppercase();
    let value = match (method.as_str(), path) {
        ("GET", "/api/v1/auth/status") => json!({
            "status": "ok",
            "pin_managed": false,
            "requires_pin": false,
            "unlocked": true,
            "pin_state": "unclaimed",
            "pin_digits": 4,
            "locked_for": 0,
        }),
        ("GET", "/api/v1/health") => json!({
            "status": "ok",
            "host": "vst3",
            "revision": env!("CARGO_PKG_VERSION"),
            "ui_revision": WEB_ASSETS
                .get_file("ui-revision.txt")
                .and_then(|asset| std::str::from_utf8(asset.contents()).ok())
                .map(str::trim)
                .unwrap_or("unknown"),
        }),
        ("GET", "/api/v1/plugins") => Value::Array(
            shared
                .catalog
                .iter()
                .map(|model| plugin_descriptor(model, shared))
                .collect(),
        ),
        ("GET", "/api/v1/controllers") => json!({ "status": "ok", "controllers": [] }),
        ("GET", requested) if requested.starts_with("/api/v1/plugins/") => {
            let plugin_id = requested.trim_start_matches("/api/v1/plugins/");
            let model = catalog_model(shared, plugin_id)?;
            plugin_descriptor(&model, shared)
        }
        ("POST", requested)
            if requested.starts_with("/api/v1/plugins/") && requested.ends_with("/activate") =>
        {
            let plugin_id = requested
                .trim_start_matches("/api/v1/plugins/")
                .trim_end_matches("/activate")
                .trim_end_matches('/');
            let model = shared.select_plugin_from_ui(plugin_id)?;
            json!({ "status": "active", "plugin_id": model.plugin_id })
        }
        _ => {
            let body = params.get("body").and_then(Value::as_str).unwrap_or("null");
            if let Some(outcome) = handle_resource_request(&method, path, body, shared) {
                outcome?
            } else {
                return Err(format!("RackForge VST3 has no {method} route for {path}"));
            }
        }
    };
    Ok((value, Vec::new()))
}

/// Where an installed file lives for a plug-in: `data/plugins/<id>/<data_path>`.
///
/// The same place the desktop writes and every host's `resources/status` reads,
/// so a cartridge installed from the DAW is the one the desktop app sees.
fn installed_resource_path(
    plugin_id: &str,
    resource_id: &str,
    shared: &RackForgeControllerShared,
) -> Result<std::path::PathBuf, String> {
    let model = shared
        .catalog
        .iter()
        .find(|candidate| candidate.plugin_id == plugin_id)
        .ok_or_else(|| format!("RackForge VST3 does not carry {plugin_id}"))?;
    let requirement = model
        .resources
        .iter()
        .find(|resource| resource.id == resource_id)
        .ok_or_else(|| format!("{plugin_id} declares no resource {resource_id}"))?;
    let relative = requirement
        .data_path
        .as_deref()
        .ok_or_else(|| format!("{resource_id} is not an installable resource"))?;
    // The plug-in wrote this path, so it is checked rather than trusted: it
    // stays inside the plug-in's own directory or it is refused.
    if relative.is_empty()
        || std::path::Path::new(relative)
            .components()
            .any(|component| !matches!(component, std::path::Component::Normal(_)))
    {
        return Err(format!("{resource_id} declares an unusable data path"));
    }
    let root = crate::engine::rackforge_root().map_err(|error| error.to_string())?;
    Ok(root
        .join("data")
        .join("plugins")
        .join(plugin_id)
        .join(relative))
}

/// The storage routes a plug-in's config surface asks this host for.
///
/// They are the same paths the desktop and the Raspberry Pi serve, answered by
/// the same `NativeResourceBrowser`, so a config page cannot tell which host it
/// is talking to -- which is the point: the page is the plug-in's, written once.
///
/// It is RackForge's own explorer that walks these, not an operating-system
/// dialog. The desktop can open one because its routes are answered on a tokio
/// worker, away from the thread that owns its window; here the answer is a
/// synchronous WebView2 protocol callback on the thread that owns the editor,
/// and a modal window opened from it would hold that thread -- inside the DAW's
/// process -- for as long as the dialog stood.
fn handle_resource_request(
    method: &str,
    path: &str,
    body: &str,
    shared: &RackForgeControllerShared,
) -> Option<Result<Value, String>> {
    let browser = match shared.resources.as_ref() {
        Some(browser) => browser,
        // Reached only if the interface asked for a surface this host never
        // offered, so it says so rather than answering as though it had.
        None => {
            return Some(Err(
                "RackForge VST3 has no storage to browse: its resource store could not be opened"
                    .to_owned(),
            ));
        }
    };
    let as_request = |body: &str| -> Result<Value, String> {
        serde_json::from_str::<Value>(body).map_err(|error| error.to_string())
    };
    let known = |model: Option<&str>| -> Result<(), String> {
        let plugin_id = model.unwrap_or_default();
        if shared
            .catalog
            .iter()
            .any(|candidate| candidate.plugin_id == plugin_id)
        {
            Ok(())
        } else {
            Err(format!("RackForge VST3 does not carry {plugin_id}"))
        }
    };
    let outcome = match (method, path) {
        ("GET", "/api/v1/resources/mounts") => browser
            .mounts()
            .map_err(|error| error.to_string())
            .and_then(|mounts| serde_json::to_value(mounts).map_err(|error| error.to_string())),
        ("GET", requested) if requested.starts_with("/api/v1/resources/mounts/") => {
            let mount_id = requested
                .trim_start_matches("/api/v1/resources/mounts/")
                .trim_end_matches("/root");
            browser
                .mount_root(mount_id)
                .map_err(|error| error.to_string())
                .and_then(|root| serde_json::to_value(root).map_err(|error| error.to_string()))
        }
        ("GET", requested) if requested.starts_with("/api/v1/resources/entries/") => {
            let parent = requested.trim_start_matches("/api/v1/resources/entries/");
            browser
                .entries(parent)
                .map_err(|error| error.to_string())
                .and_then(|entries| {
                    serde_json::to_value(entries).map_err(|error| error.to_string())
                })
        }
        ("POST", "/api/v1/resources/bind") => as_request(body).and_then(|request| {
            known(request.get("plugin_id").and_then(Value::as_str))?;
            let request: BindResourceRequest =
                serde_json::from_value(request).map_err(|error| error.to_string())?;
            browser
                .bind(&request)
                .map_err(|error| error.to_string())
                .and_then(|grant| serde_json::to_value(grant).map_err(|error| error.to_string()))
        }),
        ("POST", "/api/v1/resources/grants") => as_request(body).and_then(|request| {
            let plugin_id = request.get("plugin_id").and_then(Value::as_str);
            known(plugin_id)?;
            browser
                .grants(plugin_id.unwrap_or_default())
                .map_err(|error| error.to_string())
                .and_then(|grants| serde_json::to_value(grants).map_err(|error| error.to_string()))
        }),
        ("POST", "/api/v1/resources/status") => as_request(body).and_then(|request| {
            let plugin_id = request.get("plugin_id").and_then(Value::as_str);
            known(plugin_id)?;
            let plugin_id = plugin_id.unwrap_or_default();
            let model = shared
                .catalog
                .iter()
                .find(|candidate| candidate.plugin_id == plugin_id)
                .ok_or_else(|| format!("RackForge VST3 does not carry {plugin_id}"))?;
            Ok(Value::Array(
                model
                    .resources
                    .iter()
                    .filter(|resource| resource.data_path.is_some())
                    .map(|resource| {
                        let installed = installed_resource_path(plugin_id, &resource.id, shared)
                            .is_ok_and(|path| path.is_file());
                        json!({ "resource_id": resource.id, "installed": installed })
                    })
                    .collect(),
            ))
        }),
        ("POST", "/api/v1/resources/load") => as_request(body).and_then(|request| {
            let plugin_id = request.get("plugin_id").and_then(Value::as_str);
            known(plugin_id)?;
            let plugin_id = plugin_id.unwrap_or_default();
            if request.get("preview").and_then(Value::as_bool) == Some(true) {
                // A preview means "let me hear it without keeping it", and
                // hearing it means reaching the sounding instrument, which
                // this side cannot do. Refused rather than silently kept.
                return Err(
                    "RackForge VST3 cannot preview a resource: install it to hear it".to_owned(),
                );
            }
            let target = request
                .get("target_resource_id")
                .and_then(Value::as_str)
                .ok_or_else(|| "the request names no resource".to_owned())?;
            let grant_id = request
                .get("grant_id")
                .and_then(Value::as_str)
                .ok_or_else(|| "the request names no grant".to_owned())?;
            let source = browser
                .resolve_granted_file(
                    plugin_id,
                    grant_id,
                    request.get("entry_id").and_then(Value::as_str),
                )
                .map_err(|error| error.to_string())?;
            let destination = installed_resource_path(plugin_id, target, shared)?;
            install_file(&source, &destination)?;
            shared.reload_component();
            Ok(json!({ "status": "ok" }))
        }),
        ("POST", "/api/v1/resources/clear") => as_request(body).and_then(|request| {
            let plugin_id = request.get("plugin_id").and_then(Value::as_str);
            known(plugin_id)?;
            let plugin_id = plugin_id.unwrap_or_default();
            let target = request
                .get("target_resource_id")
                .and_then(Value::as_str)
                .ok_or_else(|| "the request names no resource".to_owned())?;
            let installed = installed_resource_path(plugin_id, target, shared)?;
            match std::fs::remove_file(&installed) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(format!("removing {}: {error}", installed.display()));
                }
            }
            shared.reload_component();
            Ok(json!({ "status": "ok" }))
        }),
        _ => return None,
    };
    Some(outcome)
}

/// Puts a granted file where the plug-in will read it, whole or not at all.
///
/// Written beside the destination and renamed onto it: a copy interrupted
/// half way -- the DAW quitting, the machine losing power -- must not leave a
/// truncated cartridge that the instrument would load as though it were one.
fn install_file(source: &std::path::Path, destination: &std::path::Path) -> Result<(), String> {
    let parent = destination
        .parent()
        .ok_or_else(|| "the install path has no directory".to_owned())?;
    std::fs::create_dir_all(parent)
        .map_err(|error| format!("creating {}: {error}", parent.display()))?;
    let staged = destination.with_extension("rackforge-installing");
    std::fs::copy(source, &staged)
        .map_err(|error| format!("copying {}: {error}", source.display()))?;
    std::fs::rename(&staged, destination).map_err(|error| {
        let _ = std::fs::remove_file(&staged);
        format!("installing {}: {error}", destination.display())
    })
}

fn handle_session_command(
    command: &Value,
    shared: &RackForgeControllerShared,
) -> Result<Vec<Value>, String> {
    let model = shared
        .model()
        .ok_or_else(|| "RackForge VST3 has no active instrument".to_owned())?;
    let operation = command
        .get("op")
        .and_then(Value::as_str)
        .ok_or_else(|| "VST session request has no operation".to_owned())?;
    match operation {
        "snapshot" => Ok(vec![snapshot(shared)?]),
        "output_meter" => Ok(vec![json!({
            "status": "output_meter",
            "meter": { "left_peak": 0.0, "right_peak": 0.0 },
        })]),
        "performance_snapshot" => Ok(vec![json!({
            "status": "performance_snapshot",
            "snapshot": empty_performance_snapshot(),
        })]),
        "plugin_parameters" => {
            validate_instance(command)?;
            Ok(vec![json!({
                "status": "plugin_parameters",
                "instance_id": "vst3-main",
                "schema": model.schema,
                "values": parameter_values(shared, &model),
            })])
        }
        "set_plugin_parameter" => {
            validate_instance(command)?;
            let index = command
                .get("parameter_index")
                .and_then(Value::as_u64)
                .and_then(|value| u32::try_from(value).ok())
                .ok_or_else(|| "invalid VST plugin parameter index".to_owned())?;
            let value = command
                .get("value")
                .and_then(Value::as_f64)
                .filter(|value| value.is_finite())
                .ok_or_else(|| "invalid VST plugin parameter value".to_owned())?;
            let canonical = shared
                .set_plugin_parameter_from_ui(index, value)
                .ok_or_else(|| format!("plugin parameter {index} is not writable"))?;
            Ok(vec![json!({
                "status": "plugin_parameter_set",
                "instance_id": "vst3-main",
                "parameter_index": index,
                "value": canonical,
            })])
        }
        "plugin_presets" => Ok(vec![json!({
            "status": "plugin_presets",
            "plugin_id": model.plugin_id,
            "presets": [],
        })]),
        // The DAW owns the inputs; the plug-in captures none of its own.
        "audio_input" => Ok(vec![json!({
            "status": "audio_input",
            "input": {
                "availability": "unsupported",
                "device_channels": 0,
                "captured": [],
                "gain_db": 0.0,
                "cable_routing": false,
                "peaks": [],
                "reason": "the DAW routes audio into the plug-in",
            },
        })]),
        "dispatch" => dispatch(command, shared),
        unsupported => Err(format!(
            "RackForge VST3 session does not support {unsupported} yet"
        )),
    }
}

fn dispatch(request: &Value, shared: &RackForgeControllerShared) -> Result<Vec<Value>, String> {
    let envelope = request
        .get("envelope")
        .ok_or_else(|| "VST dispatch request has no envelope".to_owned())?;
    let client_id = envelope
        .get("client_id")
        .and_then(Value::as_str)
        .ok_or_else(|| "VST dispatch request has no client id".to_owned())?;
    let command_id = envelope
        .get("command_id")
        .and_then(Value::as_u64)
        .ok_or_else(|| "VST dispatch request has no command id".to_owned())?;
    let command = envelope
        .get("command")
        .ok_or_else(|| "VST dispatch request has no command".to_owned())?;
    match command.get("type").and_then(Value::as_str) {
        Some("set_master_level") => {
            let level = command
                .get("level")
                .and_then(Value::as_f64)
                .filter(|value| value.is_finite())
                .ok_or_else(|| "invalid VST master level".to_owned())?;
            shared.set_level_from_ui(level / 1000.0);
        }
        Some("set_active_mode") => {
            if command.get("mode").and_then(Value::as_str) != Some("play") {
                return Err("RackForge VST3 only supports PLAY mode".to_owned());
            }
        }
        Some("select_plugin") => {
            if command.get("instance_id").and_then(Value::as_str) != Some("vst3-main") {
                return Err("this RackForge VST3 instance has no such plugin".to_owned());
            }
        }
        Some("select_sound") => {
            if command.get("instance_id").and_then(Value::as_str) != Some("vst3-main") {
                return Err("this RackForge VST3 instance has no such plugin".to_owned());
            }
            let sound_id = command
                .get("sound_id")
                .and_then(Value::as_str)
                .ok_or_else(|| "VST sound selection has no sound id".to_owned())?;
            shared
                .apply_preset_from_ui(sound_id)
                .ok_or_else(|| format!("unknown VST sound {sound_id:?}"))?;
        }
        Some("set_master_pan") => {
            return Err("RackForge VST3 master pan is owned by the DAW mixer".to_owned());
        }
        Some(other) => return Err(format!("RackForge VST3 cannot apply {other} yet")),
        None => return Err("VST dispatch command has no type".to_owned()),
    }
    let revision = shared.revision();
    Ok(vec![
        json!({
            "status": "command_applied",
            "client_id": client_id,
            "command_id": command_id,
            "revision": revision,
            "events": [],
        }),
        snapshot(shared)?,
    ])
}

fn catalog_model(
    shared: &RackForgeControllerShared,
    plugin_id: &str,
) -> Result<std::sync::Arc<VstPluginModel>, String> {
    shared
        .catalog
        .iter()
        .find(|model| model.plugin_id == plugin_id)
        .cloned()
        .ok_or_else(|| format!("Plugin {plugin_id} is not bundled with RackForge VST3"))
}

fn validate_instance(command: &Value) -> Result<(), String> {
    if command.get("instance_id").and_then(Value::as_str) == Some("vst3-main") {
        Ok(())
    } else {
        Err("plugin instance is not active in this RackForge VST3".to_owned())
    }
}

/// Whether this host can actually serve a plug-in's config surface.
///
/// A config page is a page plus the storage routes behind it. Advertising the
/// page alone is a promise the host cannot keep: the interface lights the
/// Config button, the page loads, and its first request fails. The VST3 did
/// exactly that -- `config_available` was the literal `true` -- so the answer
/// is derived from the two things that have to hold, in one place that both
/// the catalogue and the snapshot read.
fn config_available(model: &VstPluginModel, shared: &RackForgeControllerShared) -> bool {
    model.config_entry.is_some() && shared.resources.is_some()
}

fn plugin_descriptor(model: &VstPluginModel, shared: &RackForgeControllerShared) -> Value {
    let asset = |entry: &str| {
        format!(
            "/plugin-assets/{}/{}?v={}",
            model.plugin_id,
            entry.replace('\\', "/"),
            model.version
        )
    };
    let mut surfaces = vec![json!({ "kind": "play", "entry_url": asset(&model.play_entry) })];
    // Listed only when it can be served: the interface reads this list to
    // decide whether to offer Config at all.
    if let Some(config) = &model.config_entry
        && config_available(model, shared)
    {
        surfaces.push(json!({ "kind": "config", "entry_url": asset(config) }));
    }
    let branding = model.branding.as_ref().map(|branding| {
        json!({
            "icon_url": asset(&branding.icon),
            "banner_url": asset(&branding.banner),
            "splash_url": asset(&branding.splash),
            "background_color": branding.background_color,
            "accent_color": branding.accent_color,
        })
    });
    json!({
        "plugin_id": model.plugin_id,
        "plugin_name": model.name,
        "version": model.version,
        "active": true,
        "managed": false,
        "api_version": model.web_api_version,
        "branding": branding,
        "surfaces": surfaces,
        "resources": model.resources,
    })
}

fn snapshot(shared: &RackForgeControllerShared) -> Result<Value, String> {
    let model = shared
        .model()
        .ok_or_else(|| "RackForge VST3 has no active instrument".to_owned())?;
    let sounds = model
        .preset_names
        .iter()
        .map(|(id, name)| {
            json!({
                "id": id,
                "name": name,
                "bank": model.preset_banks.get(id).cloned().flatten(),
                "editable": false,
            })
        })
        .collect::<Vec<_>>();
    let layouts = if config_available(&model, shared) {
        vec!["play", "config"]
    } else {
        vec!["play"]
    };
    Ok(json!({
        "status": "snapshot",
        "snapshot": {
            "schema_version": 14,
            "session_id": "rackforge-vst3",
            "revision": shared.revision(),
            "active_mode": "play",
            "master_level": (shared.level() * 1000.0).round(),
            "master_pan": 0,
            "live": { "mode": "rack" },
            "active_instance_id": "vst3-main",
            "instances": [{
                "instance_id": "vst3-main",
                "plugin_id": model.plugin_id,
                "plugin_name": model.name,
                "ui_layouts": layouts,
                "config_available": config_available(&model, shared),
                "sounds": sounds,
                "selected_sound_id": shared.selected_sound_id(),
            }],
            "parameter_links": [],
        }
    }))
}

fn parameter_values(
    shared: &RackForgeControllerShared,
    model: &VstPluginModel,
) -> Vec<VstParameterValue> {
    shared
        .values
        .read()
        .map(|values| {
            values
                .iter()
                .map(|(index, value)| VstParameterValue {
                    index: *index,
                    value: *value,
                })
                .collect()
        })
        .unwrap_or_else(|_| model.initial_values.clone())
}

fn empty_performance_snapshot() -> Value {
    json!({
        "schema_version": 1,
        "revision": "vst3",
        "library": {
            "schema_version": 1,
            "racks": [],
            "songs": [],
            "setlists": [],
        },
        "live": { "mode": "rack" },
    })
}

fn session_message(message: Value) -> Value {
    session_event(
        "message",
        Some(serde_json::to_string(&message).expect("serialize VST session message")),
    )
}

fn session_event(event: &str, payload: Option<String>) -> Value {
    json!({
        "protocol": HOST_PROTOCOL,
        "kind": "event",
        "channel": "session",
        "event": event,
        "payload": payload,
    })
}

fn plugin_asset(path: &str, shared: &RackForgeControllerShared) -> Response<Cow<'static, [u8]>> {
    let Some(remainder) = path.strip_prefix("/plugin-assets/") else {
        return protocol_error(StatusCode::NOT_FOUND, "plugin asset not found");
    };
    let Some((plugin_id, relative)) = remainder.split_once('/') else {
        return protocol_error(StatusCode::NOT_FOUND, "plugin asset not found");
    };
    let Ok(model) = catalog_model(shared, plugin_id) else {
        return protocol_error(StatusCode::NOT_FOUND, "plugin asset not found");
    };
    let relative = std::path::Path::new(relative);
    if relative
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return protocol_error(StatusCode::BAD_REQUEST, "invalid plugin asset path");
    }
    let path = model.package_root.join(relative);
    let Ok(bytes) = std::fs::read(&path) else {
        diagnostic::write(format!("VST plugin asset not found: {}", path.display()));
        return protocol_error(StatusCode::NOT_FOUND, "plugin asset not found");
    };
    let mime = mime_guess::from_path(&path).first_or_octet_stream();
    let mut response = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, mime.as_ref())
        .header(header::CACHE_CONTROL, "no-store")
        .body(Cow::Owned(bytes))
        .expect("valid VST plugin asset response");
    if mime == mime_guess::mime::TEXT_HTML {
        response.headers_mut().insert(
            header::HeaderName::from_static("content-security-policy"),
            header::HeaderValue::from_static(
                "default-src 'none'; script-src 'self' 'wasm-unsafe-eval'; \
                 style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self'; \
                 connect-src 'self'; media-src 'none'; frame-ancestors 'self'; \
                 base-uri 'none'; form-action 'none'",
            ),
        );
    }
    response
}

fn static_asset(path: &str) -> Response<Cow<'static, [u8]>> {
    let requested = path.trim_start_matches('/');
    let requested = if requested.is_empty() {
        "index.html"
    } else {
        requested
    };
    let asset = WEB_ASSETS
        .get_file(requested)
        .or_else(|| WEB_ASSETS.get_file("index.html"));
    let Some(asset) = asset else {
        return protocol_error(StatusCode::NOT_FOUND, "RackForge interface not found");
    };
    let mime = mime_guess::from_path(asset.path()).first_or_octet_stream();
    let cache = if asset.path().starts_with("assets/") {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, mime.as_ref())
        .header(header::CACHE_CONTROL, cache)
        .body(Cow::Borrowed(asset.contents()))
        .expect("valid embedded RackForge interface response")
}

fn json_response(status: StatusCode, value: Value) -> Response<Cow<'static, [u8]>> {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json; charset=utf-8")
        .header(header::CACHE_CONTROL, "no-store")
        .body(Cow::Owned(
            serde_json::to_vec(&value).expect("serialize VST bridge response"),
        ))
        .expect("valid VST bridge response")
}

fn json_error(status: StatusCode, message: String) -> Response<Cow<'static, [u8]>> {
    json_response(status, json!({ "error": message }))
}

fn protocol_error(status: StatusCode, message: &'static str) -> Response<Cow<'static, [u8]>> {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
        .body(Cow::Borrowed(message.as_bytes()))
        .expect("valid VST protocol error")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_web_build_is_embedded() {
        assert!(WEB_ASSETS.get_file("index.html").is_some());
        assert!(WEB_ASSETS.get_file("ui-revision.txt").is_some());
    }

    #[test]
    fn native_sound_selection_updates_the_vst_instance_and_snapshot() {
        let controller = super::super::RackForgeController::new();
        if controller
            .shared
            .catalog
            .iter()
            .all(|model| model.plugin_id != "org.rackforge.rf-106")
        {
            return;
        }
        let model = controller
            .shared
            .select_plugin_from_ui("org.rackforge.rf-106")
            .expect("select bundled RF-106");
        let sound_id = model
            .preset_names
            .keys()
            .nth(1)
            .expect("RF-106 has multiple sounds")
            .clone();
        let request = NativeRequest {
            request_id: "test.select-sound".to_owned(),
            method: "plugin.select_sound".to_owned(),
            params: json!({
                "instance_id": "vst3-main",
                "sound_id": sound_id,
            }),
        };

        let (result, events) = handle_native_request(&request, &controller.shared)
            .expect("native sound selection succeeds");

        assert_eq!(result["sound_id"], sound_id);
        assert_eq!(
            controller.shared.selected_sound_id().as_deref(),
            Some(sound_id.as_str())
        );
        assert_eq!(events.len(), 1);
        let published: Value = serde_json::from_str(
            events[0]["payload"]
                .as_str()
                .expect("session event contains a serialized snapshot"),
        )
        .expect("session snapshot is valid JSON");
        assert_eq!(
            published["snapshot"]["instances"][0]["selected_sound_id"],
            sound_id
        );
    }

    #[test]
    fn ui_route_is_reused_by_the_next_vst_editor_view() {
        let controller = super::super::RackForgeController::new();
        let request = NativeRequest {
            request_id: "test.ui-route".to_owned(),
            method: "ui.route".to_owned(),
            params: json!({ "path": "/play" }),
        };

        let (result, events) =
            handle_native_request(&request, &controller.shared).expect("valid UI route succeeds");

        assert_eq!(result["path"], "/play");
        assert!(events.is_empty());
        assert_eq!(
            controller.shared.editor_url(),
            "rackforge://localhost/index.html#/play"
        );

        let invalid = NativeRequest {
            request_id: "test.invalid-ui-route".to_owned(),
            method: "ui.route".to_owned(),
            params: json!({ "path": "https://example.com/" }),
        };
        assert!(handle_native_request(&invalid, &controller.shared).is_err());
        assert_eq!(controller.shared.ui_route(), "/play");
    }
}

#[cfg(test)]
mod initialization_script_tests {
    use super::*;

    #[test]
    fn the_injected_script_carries_the_shared_protocol() {
        let script = initialization_script();
        assert!(
            !script.contains(PROTOCOL_PLACEHOLDER),
            "the marker survived: the bridge would stamp envelopes with it"
        );
        assert!(
            script.contains(HOST_PROTOCOL),
            "the injected bridge does not name the protocol at all"
        );
        assert!(
            INITIALIZATION_SCRIPT_TEMPLATE.contains(PROTOCOL_PLACEHOLDER),
            "the template stopped carrying the marker, so the substitution \
             above now proves nothing"
        );
    }

    /// A host may not offer a surface it cannot serve.
    ///
    /// `config_available` was the literal `true`, so the interface lit the
    /// Config button on a host with no storage routes at all: the page opened
    /// and its first request failed. The answer is derived now, and this holds
    /// it to both halves -- the plug-in has a config page, AND this host can
    /// answer for it.
    #[test]
    fn a_config_surface_is_offered_only_when_it_can_be_served() {
        let controller = super::super::RackForgeController::new();
        let Some(model) = controller
            .shared
            .catalog
            .iter()
            .find(|model| model.config_entry.is_some())
            .cloned()
        else {
            return;
        };

        assert!(
            config_available(&model, &controller.shared),
            "a carried config page with storage open must be offered"
        );
        let descriptor = plugin_descriptor(&model, &controller.shared);
        assert!(
            descriptor["surfaces"]
                .as_array()
                .expect("surfaces")
                .iter()
                .any(|surface| surface["kind"] == "config"),
            "the catalogue must list the config surface it can serve"
        );

        let without = RackForgeControllerShared {
            resources: None,
            ..controller.shared.clone()
        };
        assert!(
            !config_available(&model, &without),
            "with no storage the config surface must not be claimed"
        );
        let descriptor = plugin_descriptor(&model, &without);
        assert!(
            !descriptor["surfaces"]
                .as_array()
                .expect("surfaces")
                .iter()
                .any(|surface| surface["kind"] == "config"),
            "a surface that cannot be served must not be listed either"
        );
    }

    /// The storage routes answer over the same bridge the interface uses.
    ///
    /// Not a unit call: this goes through `http.request`, the way the config
    /// page's `hostJson` reaches this host, so a route that is implemented but
    /// never dispatched fails here.
    #[test]
    fn the_storage_routes_answer_over_the_bridge() {
        let controller = super::super::RackForgeController::new();
        if controller.shared.resources.is_none() {
            return;
        }
        let ask = |method: &str, path: &str, body: Value| {
            handle_native_request(
                &NativeRequest {
                    request_id: "test.resources".to_owned(),
                    method: "http.request".to_owned(),
                    params: json!({
                        "path": path,
                        "method": method,
                        "body": if body.is_null() { Value::Null } else { Value::String(body.to_string()) },
                    }),
                },
                &controller.shared,
            )
            .map(|(value, _)| value)
        };

        let mounts = ask("GET", "/api/v1/resources/mounts", Value::Null).expect("mounts answers");
        let mounts = mounts.as_array().expect("mounts is a list").clone();
        assert!(
            !mounts.is_empty(),
            "the platform defaults give this host somewhere to start"
        );
        let mount_id = mounts[0]["id"].as_str().expect("a mount has an id");
        let root = ask(
            "GET",
            &format!("/api/v1/resources/mounts/{mount_id}/root"),
            Value::Null,
        )
        .expect("a mount root answers");
        assert!(root["id"].is_string(), "a root is an entry");

        let Some(model) = controller.shared.catalog.first().cloned() else {
            return;
        };
        let grants = ask(
            "POST",
            "/api/v1/resources/grants",
            json!({ "plugin_id": model.plugin_id }),
        )
        .expect("grants answers for a carried plugin");
        assert!(grants.is_array(), "grants is a list");
    }

    /// A storage route may not be used to reach a plug-in this host never
    /// carried: the bridge is open to the page, and the page is the plug-in's.
    #[test]
    fn the_storage_routes_refuse_a_plugin_this_host_does_not_carry() {
        let controller = super::super::RackForgeController::new();
        if controller.shared.resources.is_none() {
            return;
        }
        let outcome = handle_native_request(
            &NativeRequest {
                request_id: "test.resources.stranger".to_owned(),
                method: "http.request".to_owned(),
                params: json!({
                    "path": "/api/v1/resources/grants",
                    "method": "POST",
                    "body": "{\"plugin_id\":\"org.example.not-carried\"}",
                }),
            },
            &controller.shared,
        );

        let error = outcome.expect_err("an uncarried plugin is refused");
        assert!(
            error.contains("org.example.not-carried"),
            "the refusal names what was asked for: {error}"
        );
    }

    /// An install lands in the plug-in's data directory, whole, and shows.
    ///
    /// Installing means a file at `data/plugins/<id>/<data_path>` -- the same
    /// place the desktop writes and every host's `status` reads -- so a
    /// cartridge installed from the DAW is the one the desktop app sees. This
    /// walks the whole way: nothing installed, a grant bound from a real file,
    /// `load`, and then installed.
    #[test]
    fn a_granted_file_installs_where_every_host_looks_for_it() {
        let controller = super::super::RackForgeController::new();
        let Some(browser) = controller.shared.resources.clone() else {
            return;
        };
        let Some((model, resource)) = controller.shared.catalog.iter().find_map(|model| {
            model
                .resources
                .iter()
                .find(|resource| resource.data_path.is_some())
                .map(|resource| (model.clone(), resource.clone()))
        }) else {
            return;
        };

        let destination =
            installed_resource_path(&model.plugin_id, &resource.id, &controller.shared)
                .expect("a declared resource has an install path");
        let _ = std::fs::remove_file(&destination);
        let ask = |method: &str, path: &str, body: Value| {
            handle_native_request(
                &NativeRequest {
                    request_id: "test.install".to_owned(),
                    method: "http.request".to_owned(),
                    params: json!({
                        "path": path,
                        "method": method,
                        "body": Value::String(body.to_string()),
                    }),
                },
                &controller.shared,
            )
            .map(|(value, _)| value)
        };
        let installed = |answer: &Value| -> bool {
            answer
                .as_array()
                .expect("status is a list")
                .iter()
                .find(|entry| entry["resource_id"] == resource.id.as_str())
                .map(|entry| entry["installed"] == true)
                .expect("the declared resource is reported")
        };

        let before = ask(
            "POST",
            "/api/v1/resources/status",
            json!({ "plugin_id": model.plugin_id }),
        )
        .expect("status answers");
        assert!(!installed(&before), "nothing is installed to begin with");

        // A real file on this machine, reached the way the explorer reaches
        // one: a mount, its root, and an entry inside it.
        let source = std::env::temp_dir().join("rackforge-vst3-install-test.bin");
        std::fs::write(&source, b"RACKFORGE TEST CARTRIDGE").expect("write a file to install");
        let selection = browser
            .register_native_selection(&source)
            .expect("a file on this machine can be selected");
        let grant = browser
            .bind_selection(
                &rackforge_resource_api::BindSelectionRequest {
                    plugin_id: model.plugin_id.clone(),
                    resource_id: resource.id.clone(),
                    selection_id: selection.selection_id.clone(),
                },
                rackforge_resource_api::ResourceEntryKind::File,
            )
            .expect("the selection binds to the declared resource");

        ask(
            "POST",
            "/api/v1/resources/load",
            json!({
                "plugin_id": model.plugin_id,
                "instance_id": "vst3-main",
                "target_resource_id": resource.id,
                "grant_id": grant.grant_id,
                "persist": true,
                "preview": false,
            }),
        )
        .expect("a granted file installs");

        assert!(
            destination.is_file(),
            "the file must be where the plug-in reads it: {}",
            destination.display()
        );
        assert_eq!(
            std::fs::read(&destination).expect("read the installed file"),
            b"RACKFORGE TEST CARTRIDGE",
            "installed whole, not truncated"
        );
        let after = ask(
            "POST",
            "/api/v1/resources/status",
            json!({ "plugin_id": model.plugin_id }),
        )
        .expect("status answers");
        assert!(installed(&after), "and status says so");

        ask(
            "POST",
            "/api/v1/resources/clear",
            json!({
                "plugin_id": model.plugin_id,
                "instance_id": "vst3-main",
                "target_resource_id": resource.id,
            }),
        )
        .expect("an installed resource clears");
        assert!(!destination.is_file(), "clearing removes it");

        let _ = std::fs::remove_file(&source);
        let _ = browser.release_plugin_grants(&model.plugin_id);
    }

    /// A preview is refused rather than quietly kept.
    ///
    /// Preview means "let me hear it without keeping it", and hearing it means
    /// reaching the instrument that is sounding -- which this side cannot do.
    /// Answering `ok` would have installed it for good while the page believed
    /// it had borrowed it.
    #[test]
    fn a_preview_is_refused_because_this_host_cannot_give_one() {
        let controller = super::super::RackForgeController::new();
        if controller.shared.resources.is_none() {
            return;
        }
        let Some(model) = controller.shared.catalog.first().cloned() else {
            return;
        };
        let error = handle_native_request(
            &NativeRequest {
                request_id: "test.preview".to_owned(),
                method: "http.request".to_owned(),
                params: json!({
                    "path": "/api/v1/resources/load",
                    "method": "POST",
                    "body": json!({
                        "plugin_id": model.plugin_id,
                        "instance_id": "vst3-main",
                        "target_resource_id": "anything",
                        "grant_id": "anything",
                        "persist": false,
                        "preview": true,
                    })
                    .to_string(),
                }),
            },
            &controller.shared,
        )
        .expect_err("a preview is refused");

        assert!(
            error.contains("preview"),
            "the refusal says what it cannot do: {error}"
        );
    }
}
