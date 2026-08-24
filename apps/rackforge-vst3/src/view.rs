use super::{RackForgeControllerShared, diagnostic};
use std::{ffi::CStr, sync::Mutex};
use vst3::{Class, ComPtr, ComRef, Steinberg::*};

const DEFAULT_WIDTH: i32 = 920;
const DEFAULT_HEIGHT: i32 = 620;
const MINIMUM_WIDTH: i32 = 620;
const MINIMUM_HEIGHT: i32 = 420;
const MAXIMUM_WIDTH: i32 = 1800;
const MAXIMUM_HEIGHT: i32 = 1200;

pub struct RackForgeView {
    shared: RackForgeControllerShared,
    size: Mutex<ViewRect>,
    frame: Mutex<Option<ComPtr<IPlugFrame>>>,
    #[cfg(windows)]
    webview: Mutex<Option<wry::WebView>>,
    #[cfg(windows)]
    web_context: Mutex<Option<wry::WebContext>>,
}

impl RackForgeView {
    pub fn new(shared: RackForgeControllerShared) -> Self {
        Self {
            shared,
            size: Mutex::new(ViewRect {
                left: 0,
                top: 0,
                right: DEFAULT_WIDTH,
                bottom: DEFAULT_HEIGHT,
            }),
            frame: Mutex::new(None),
            #[cfg(windows)]
            webview: Mutex::new(None),
            #[cfg(windows)]
            web_context: Mutex::new(None),
        }
    }
}

impl Class for RackForgeView {
    type Interfaces = (IPlugView,);
}

impl IPlugViewTrait for RackForgeView {
    unsafe fn isPlatformTypeSupported(&self, platform: FIDString) -> tresult {
        if platform.is_null() {
            diagnostic::write("view.isPlatformTypeSupported rejected null platform");
            return kInvalidArgument;
        }
        #[cfg(windows)]
        {
            let platform = unsafe { CStr::from_ptr(platform) }.to_string_lossy();
            diagnostic::write(format!(
                "view.isPlatformTypeSupported platform={platform:?}"
            ));
            if platform.as_bytes() == b"HWND" {
                return kResultTrue;
            }
        }
        kResultFalse
    }

    unsafe fn attached(&self, parent: *mut std::ffi::c_void, platform: FIDString) -> tresult {
        diagnostic::write(format!("view.attached parent={parent:p}"));
        if parent.is_null() || unsafe { self.isPlatformTypeSupported(platform) } != kResultTrue {
            diagnostic::write("view.attached rejected parent or platform");
            return kInvalidArgument;
        }
        #[cfg(windows)]
        {
            let Ok(mut slot) = self.webview.lock() else {
                return kInternalError;
            };
            let Ok(mut context_slot) = self.web_context.lock() else {
                return kInternalError;
            };
            if slot.is_some() {
                return kResultFalse;
            }
            let Ok(size) = self.size.lock() else {
                return kInternalError;
            };
            let width = (size.right - size.left).max(1) as u32;
            let height = (size.bottom - size.top).max(1) as u32;
            drop(size);
            let Some(parent) = RawParentWindow::new(parent) else {
                return kInvalidArgument;
            };
            let Some(model) = self.shared.model.clone() else {
                diagnostic::write("view.attached has no active plugin model");
                return kResultFalse;
            };
            let html = render_shell(&self.shared, &model);
            let package_root = model.package_root.clone();
            let shared = self.shared.clone();
            let data_directory = webview_data_directory();
            if let Err(error) = std::fs::create_dir_all(&data_directory) {
                diagnostic::write(format!(
                    "view.attached could not create WebView2 data directory {}: {error}",
                    data_directory.display()
                ));
                return kResultFalse;
            }
            diagnostic::write(format!(
                "view.attached WebView2 data directory={}",
                data_directory.display()
            ));
            let mut context = wry::WebContext::new(Some(data_directory));
            let builder = wry::WebViewBuilder::new_with_web_context(&mut context)
                .with_custom_protocol(
                    "rackforge".into(),
                    move |_webview_id, request| {
                        protocol_response(request.uri().path(), html.as_bytes(), &package_root)
                    },
                )
                .with_url("rackforge://localhost/index.html")
                .with_initialization_script(
                    r#"
                    (() => {
                      const report = (phase, detail) => {
                        try {
                          window.ipc.postMessage(JSON.stringify({ op: 'diagnostic', phase, detail }));
                        } catch (_) {}
                      };
                      window.addEventListener('DOMContentLoaded', () => report(
                        'dom_content_loaded',
                        `${document.body?.children.length ?? -1} children; ${window.innerWidth}x${window.innerHeight}`
                      ));
                      window.addEventListener('error', event => report(
                        'javascript_error',
                        `${event.message || 'unknown error'} @ ${event.filename || 'inline'}:${event.lineno || 0}`
                      ));
                    })();
                    "#,
                )
                .with_bounds(wry::Rect {
                    position: wry::dpi::LogicalPosition::new(0, 0).into(),
                    size: wry::dpi::LogicalSize::new(width, height).into(),
                })
                .with_on_page_load_handler(|event, url| {
                    let event = match event {
                        wry::PageLoadEvent::Started => "started",
                        wry::PageLoadEvent::Finished => "finished",
                    };
                    diagnostic::write(format!("view.page_load event={event} url={url:?}"));
                })
                .with_ipc_handler(move |request| {
                    let Ok(message) = serde_json::from_str::<UiMessage>(request.body()) else {
                        return;
                    };
                    match message {
                        UiMessage::SetMasterLevel { value } => shared.set_level_from_ui(value),
                        UiMessage::SetPluginParameter {
                            parameter_index,
                            value,
                        } => {
                            if shared
                                .set_plugin_parameter_from_ui(parameter_index, value)
                                .is_none()
                            {
                                diagnostic::write(format!(
                                    "view rejected plugin parameter index={parameter_index} value={value}"
                                ));
                            }
                        }
                        UiMessage::SelectSound { sound_id } => {
                            if shared.apply_preset_from_ui(&sound_id).is_none() {
                                diagnostic::write(format!(
                                    "view rejected plugin sound {sound_id:?}"
                                ));
                            }
                        }
                        UiMessage::Diagnostic { phase, detail } => diagnostic::write(format!(
                            "view.javascript phase={phase:?} detail={detail:?}"
                        )),
                    }
                });
            match builder.build_as_child(&parent) {
                Ok(view) => {
                    *slot = Some(view);
                    *context_slot = Some(context);
                    diagnostic::write("view.attached WebView2 child created");
                    kResultOk
                }
                Err(error) => {
                    diagnostic::write(format!("view.attached WebView2 failed: {error:#}"));
                    kResultFalse
                }
            }
        }
        #[cfg(not(windows))]
        {
            let _ = parent;
            kNotImplemented
        }
    }

    unsafe fn removed(&self) -> tresult {
        diagnostic::write("view.removed");
        #[cfg(windows)]
        {
            if let Ok(mut view) = self.webview.lock() {
                *view = None;
            }
            if let Ok(mut context) = self.web_context.lock() {
                *context = None;
            }
        }
        kResultOk
    }

    unsafe fn onWheel(&self, _distance: f32) -> tresult {
        kResultFalse
    }

    unsafe fn onKeyDown(&self, _key: char16, _key_code: int16, _modifiers: int16) -> tresult {
        kResultFalse
    }

    unsafe fn onKeyUp(&self, _key: char16, _key_code: int16, _modifiers: int16) -> tresult {
        kResultFalse
    }

    unsafe fn getSize(&self, size: *mut ViewRect) -> tresult {
        if size.is_null() {
            return kInvalidArgument;
        }
        let Ok(current) = self.size.lock() else {
            return kInternalError;
        };
        unsafe {
            *size = *current;
        }
        kResultOk
    }

    unsafe fn onSize(&self, new_size: *mut ViewRect) -> tresult {
        if new_size.is_null() {
            return kInvalidArgument;
        }
        let mut constrained = unsafe { *new_size };
        constrain(&mut constrained);
        if let Ok(mut current) = self.size.lock() {
            *current = constrained;
        } else {
            return kInternalError;
        }
        #[cfg(windows)]
        if let Ok(view) = self.webview.lock()
            && let Some(view) = view.as_ref()
        {
            let width = (constrained.right - constrained.left).max(1) as u32;
            let height = (constrained.bottom - constrained.top).max(1) as u32;
            if view
                .set_bounds(wry::Rect {
                    position: wry::dpi::LogicalPosition::new(0, 0).into(),
                    size: wry::dpi::LogicalSize::new(width, height).into(),
                })
                .is_err()
            {
                return kResultFalse;
            }
        }
        kResultOk
    }

    unsafe fn onFocus(&self, _state: TBool) -> tresult {
        kResultOk
    }

    unsafe fn setFrame(&self, frame: *mut IPlugFrame) -> tresult {
        let owned = unsafe { ComRef::from_raw(frame) }.map(|frame| frame.to_com_ptr());
        let Ok(mut current) = self.frame.lock() else {
            return kInternalError;
        };
        *current = owned;
        kResultOk
    }

    unsafe fn canResize(&self) -> tresult {
        kResultTrue
    }

    unsafe fn checkSizeConstraint(&self, rect: *mut ViewRect) -> tresult {
        if rect.is_null() {
            return kInvalidArgument;
        }
        constrain(unsafe { &mut *rect });
        kResultOk
    }
}

#[cfg(windows)]
fn webview_data_directory() -> std::path::PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("RackForge")
        .join("VST3")
        .join("WebView2")
}

fn constrain(rect: &mut ViewRect) {
    let width = (rect.right - rect.left).clamp(MINIMUM_WIDTH, MAXIMUM_WIDTH);
    let height = (rect.bottom - rect.top).clamp(MINIMUM_HEIGHT, MAXIMUM_HEIGHT);
    rect.right = rect.left + width;
    rect.bottom = rect.top + height;
}

#[derive(serde::Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
enum UiMessage {
    SetMasterLevel { value: f64 },
    SetPluginParameter { parameter_index: u32, value: f64 },
    SelectSound { sound_id: String },
    Diagnostic { phase: String, detail: String },
}

#[cfg(windows)]
fn render_shell(shared: &RackForgeControllerShared, model: &super::VstPluginModel) -> String {
    let values = shared
        .values
        .read()
        .map(|values| {
            values
                .iter()
                .map(|(index, value)| super::engine::VstParameterValue {
                    index: *index,
                    value: *value,
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_else(|_| model.initial_values.clone());
    include_str!("ui.html")
        .replace("__PLUGIN_NAME__", &html_escape(&model.name))
        .replace("__PLUGIN_VERSION__", &html_escape(&model.version))
        .replace("__PLUGIN_DESCRIPTION__", &html_escape(&model.description))
        .replace("__PLUGIN_ACCENT__", &html_escape(&model.accent_color))
        .replace(
            "__PLUGIN_ENTRY_JSON__",
            &serde_json::to_string(&format!("/plugin/{}", model.play_entry)).unwrap(),
        )
        .replace(
            "__PLUGIN_ID_JSON__",
            &serde_json::to_string(&model.plugin_id).unwrap(),
        )
        .replace(
            "__PLUGIN_SCHEMA_JSON__",
            &serde_json::to_string(&model.schema).unwrap(),
        )
        .replace(
            "__PLUGIN_VALUES_JSON__",
            &serde_json::to_string(&values).unwrap(),
        )
        .replace(
            "__PLUGIN_PRESETS_JSON__",
            &serde_json::to_string(&model.preset_values).unwrap(),
        )
        .replace("__RACKFORGE_INITIAL_LEVEL__", &shared.level().to_string())
}

#[cfg(windows)]
fn protocol_response(
    request_path: &str,
    shell: &[u8],
    package_root: &std::path::Path,
) -> wry::http::Response<std::borrow::Cow<'static, [u8]>> {
    use std::path::Component;
    use wry::http::{Response, StatusCode, header};

    if matches!(request_path, "/" | "/index.html") {
        return Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
            .header(header::CACHE_CONTROL, "no-store")
            .body(std::borrow::Cow::Owned(shell.to_vec()))
            .unwrap();
    }
    let Some(relative) = request_path.strip_prefix("/plugin/") else {
        return protocol_error(StatusCode::NOT_FOUND, "asset not found");
    };
    let relative = std::path::Path::new(relative);
    if relative
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return protocol_error(StatusCode::BAD_REQUEST, "invalid plugin asset path");
    }
    let path = package_root.join(relative);
    let Ok(bytes) = std::fs::read(&path) else {
        diagnostic::write(format!("view plugin asset not found: {}", path.display()));
        return protocol_error(StatusCode::NOT_FOUND, "plugin asset not found");
    };
    Response::builder()
        .status(StatusCode::OK)
        .header(
            header::CONTENT_TYPE,
            mime_guess::from_path(&path)
                .first_or_octet_stream()
                .as_ref(),
        )
        .header(header::CACHE_CONTROL, "no-store")
        .body(std::borrow::Cow::Owned(bytes))
        .unwrap()
}

#[cfg(windows)]
fn protocol_error(
    status: wry::http::StatusCode,
    message: &'static str,
) -> wry::http::Response<std::borrow::Cow<'static, [u8]>> {
    wry::http::Response::builder()
        .status(status)
        .header(wry::http::header::CONTENT_TYPE, "text/plain; charset=utf-8")
        .body(std::borrow::Cow::Borrowed(message.as_bytes()))
        .unwrap()
}

#[cfg(windows)]
fn html_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

#[cfg(windows)]
struct RawParentWindow {
    handle: std::num::NonZeroIsize,
}

#[cfg(windows)]
impl RawParentWindow {
    fn new(parent: *mut std::ffi::c_void) -> Option<Self> {
        Some(Self {
            handle: std::num::NonZeroIsize::new(parent as isize)?,
        })
    }
}

#[cfg(windows)]
impl raw_window_handle::HasWindowHandle for RawParentWindow {
    fn window_handle(
        &self,
    ) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError> {
        let handle = raw_window_handle::Win32WindowHandle::new(self.handle);
        Ok(unsafe {
            raw_window_handle::WindowHandle::borrow_raw(raw_window_handle::RawWindowHandle::Win32(
                handle,
            ))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn size_constraints_keep_editor_usable() {
        let mut tiny = ViewRect {
            left: 10,
            top: 20,
            right: 11,
            bottom: 21,
        };
        constrain(&mut tiny);
        assert_eq!(tiny.right - tiny.left, MINIMUM_WIDTH);
        assert_eq!(tiny.bottom - tiny.top, MINIMUM_HEIGHT);
    }
}
