use super::{RackForgeControllerShared, VstPluginModel, diagnostic, engine::VstParameterValue};
use include_dir::{Dir, include_dir};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{borrow::Cow, path::Component};
use wry::http::{Request, Response, StatusCode, header};

static WEB_ASSETS: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/../../web/dist");

pub const INITIALIZATION_SCRIPT: &str = r#"
(() => {
  window.__RACKFORGE_HOST_SHELL__ = 'vst3';
  const protocol = 'rackforge.host@1';
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
                "protocol": "rackforge.host@1",
                "kind": "response",
                "request_id": request.request_id,
                "ok": true,
                "result": result,
            },
            "events": events,
        }),
        Err(message) => json!({
            "response": {
                "protocol": "rackforge.host@1",
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
                .map(|model| plugin_descriptor(model))
                .collect(),
        ),
        ("GET", "/api/v1/controllers") => json!({ "status": "ok", "controllers": [] }),
        ("GET", requested) if requested.starts_with("/api/v1/plugins/") => {
            let plugin_id = requested.trim_start_matches("/api/v1/plugins/");
            let model = catalog_model(shared, plugin_id)?;
            plugin_descriptor(&model)
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
        _ => return Err(format!("RackForge VST3 has no {method} route for {path}")),
    };
    Ok((value, Vec::new()))
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

fn plugin_descriptor(model: &VstPluginModel) -> Value {
    let asset = |entry: &str| {
        format!(
            "/plugin-assets/{}/{}?v={}",
            model.plugin_id,
            entry.replace('\\', "/"),
            model.version
        )
    };
    let mut surfaces = vec![json!({ "kind": "play", "entry_url": asset(&model.play_entry) })];
    if let Some(config) = &model.config_entry {
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
    let layouts = if model.config_entry.is_some() {
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
                "config_available": model.config_entry.is_some(),
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
        "protocol": "rackforge.host@1",
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
