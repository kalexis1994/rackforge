use anyhow::{Context, Result};
use eframe::CreationContext;
use wry::{
    Rect, WebContext, WebView, WebViewBuilder,
    dpi::{LogicalPosition, LogicalSize},
};

/// Where the WebView keeps what it caches between runs.
///
/// Left to itself, WebView2 writes this beside the executable, in a folder
/// named after it — seventy megabytes of browser profile appearing next to a
/// self-contained binary, and unwritable at all if that binary lives anywhere
/// a user cannot write, which on Windows is where installed programs live.
/// The VST3 host has always placed it deliberately; this one had not, and the
/// two now agree.
#[cfg(windows)]
fn webview_data_directory() -> std::path::PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("RackForge")
        .join("Desktop")
        .join("WebView2")
}

pub struct DesktopWebView {
    view: WebView,
    current_url: Option<String>,
    visible: bool,
    bounds: Option<(i32, i32, u32, u32)>,
}

impl DesktopWebView {
    pub fn new(creation: &CreationContext<'_>) -> Result<Self> {
        // Only borrowed while the view is built: what the context carries into
        // the WebView is the directory above, and nothing after that reads it.
        #[cfg(windows)]
        let mut context = WebContext::new(Some(webview_data_directory()));
        #[cfg(not(windows))]
        let mut context = WebContext::new(None);
        let view = WebViewBuilder::new_with_web_context(&mut context)
            // The chassis colour, not a near-white. This is what shows in the
            // gap whenever the WebView's bounds and the panel disagree by a
            // pixel, or before the interface has painted — at #e9e7e1 that
            // read as white light leaking in at the top of the window.
            .with_html(
                "<!doctype html><html><body style=\"margin:0;background:#c9c1b3\"></body></html>",
            )
            .with_initialization_script("window.__RACKFORGE_HOST_SHELL__ = 'desktop';")
            .with_visible(false)
            .with_bounds(Rect {
                position: LogicalPosition::new(0, 0).into(),
                size: LogicalSize::new(1, 1).into(),
            })
            .with_navigation_handler(|url| {
                url.starts_with("http://127.0.0.1:") || url == "about:blank"
            })
            .build_as_child(creation)
            .context("creating the embedded RackForge WebView2 workspace")?;
        Ok(Self {
            view,
            current_url: None,
            visible: false,
            bounds: None,
        })
    }

    pub fn show(&mut self, url: &str, rect: eframe::egui::Rect) -> Result<()> {
        if self.current_url.as_deref() != Some(url) {
            self.view
                .load_url(url)
                .with_context(|| format!("opening embedded RackForge Web UI at {url}"))?;
            self.current_url = Some(url.to_owned());
        }

        let x = rect.min.x.round() as i32;
        let y = rect.min.y.round() as i32;
        let width = rect.width().round().max(1.0) as u32;
        let height = rect.height().round().max(1.0) as u32;
        let bounds = (x, y, width, height);
        if self.bounds != Some(bounds) {
            self.view.set_bounds(Rect {
                position: LogicalPosition::new(x, y).into(),
                size: LogicalSize::new(width, height).into(),
            })?;
            self.bounds = Some(bounds);
        }
        if !self.visible {
            self.view.set_visible(true)?;
            self.visible = true;
        }
        Ok(())
    }

    pub fn hide(&mut self) -> Result<()> {
        if self.visible {
            self.view.set_visible(false)?;
            self.visible = false;
        }
        Ok(())
    }

    pub fn reload(&self) -> Result<()> {
        self.view.reload().context("reloading RackForge Web UI")
    }
}
