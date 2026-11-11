use anyhow::{Context, Result};
use eframe::CreationContext;
use wry::{
    NewWindowResponse, Rect, WebContext, WebView, WebViewBuilder,
    dpi::{LogicalPosition, LogicalSize},
};

/// Links the interface may send to the system browser: the project's own
/// pages and RackForge Web, and nothing else. The WebView itself only ever
/// shows the local interface, so a link out opens beside it rather than in
/// place of it.
fn opens_in_system_browser(url: &str) -> bool {
    url.starts_with("https://github.com/kalexis1994/")
        || url.starts_with("https://kalexis1994.github.io/rackforge/")
}

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

/// On Linux, WebKitGTK's cache in the user's cache directory (inside a
/// Flatpak, the app's own).
#[cfg(target_os = "linux")]
fn webview_data_directory() -> std::path::PathBuf {
    std::env::var_os("XDG_CACHE_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| std::path::Path::new(&home).join(".cache")))
        .unwrap_or_else(std::env::temp_dir)
        .join("RackForge")
        .join("WebKit")
}

pub struct DesktopWebView {
    view: WebView,
    current_url: Option<String>,
    visible: bool,
    bounds: Option<(i32, i32, u32, u32)>,
}

impl DesktopWebView {
    pub fn new(creation: &CreationContext<'_>) -> Result<Self> {
        // WebKitGTK is a GTK widget: GTK must be up on this, the window's,
        // thread before the view is made, and its events are then pumped
        // every frame (`pump`). The window is X11's -- see `run` -- because a
        // child WebView cannot be placed in a Wayland surface.
        #[cfg(target_os = "linux")]
        gtk::init().context("starting GTK for the embedded RackForge workspace")?;
        // Only borrowed while the view is built: what the context carries into
        // the WebView is the directory above, and nothing after that reads it.
        #[cfg(desktop_host)]
        let mut context = WebContext::new(Some(webview_data_directory()));
        #[cfg(not(desktop_host))]
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
            // A link that asks for a new window -- About's link to the
            // project -- opens in the system browser; no WebView window of
            // its own is ever made, and nothing else leaves the interface.
            .with_new_window_req_handler(|url, _features| {
                if opens_in_system_browser(&url)
                    && let Err(error) = webbrowser::open(&url)
                {
                    eprintln!("EXTERNAL_LINK_FAILED url={url} error={error}");
                }
                NewWindowResponse::Deny
            })
            .build_as_child(creation)
            .context("creating the embedded RackForge web workspace")?;
        Ok(Self {
            view,
            current_url: None,
            visible: false,
            bounds: None,
        })
    }

    /// Shows the interface over `rect`, in egui's points. WebView2 takes
    /// them as they are, since it scales with the window's DPI; a WebKit child
    /// of an X11 window does not scale, so on Linux they become the pixels
    /// they cover -- on a 150 % display it otherwise filled two-thirds of the
    /// window each way.
    pub fn show(
        &mut self,
        url: &str,
        rect: eframe::egui::Rect,
        pixels_per_point: f32,
    ) -> Result<()> {
        // The bottom pixel row is left to the window: an X11 child that covers
        // its parent entirely makes the X server report the parent fully
        // obscured, winit passes that on as occluded, and eframe stops
        // running frames for an occluded window -- one a second, measured --
        // and every request the interface sends the host waited for one.
        #[cfg(target_os = "linux")]
        let rect = eframe::egui::Rect::from_min_max(
            (rect.min.to_vec2() * pixels_per_point).to_pos2(),
            (rect.max.to_vec2() * pixels_per_point - eframe::egui::vec2(0.0, 1.0)).to_pos2(),
        );
        #[cfg(not(target_os = "linux"))]
        let _ = pixels_per_point;
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

    /// Lets GTK handle what is waiting for it -- the WebView's drawing, input
    /// and network -- for a few milliseconds of the frame. Linux only:
    /// WebView2 runs on the window's own message loop.
    ///
    /// Bounded in time, not by an empty queue: a page that animates keeps the
    /// queue from ever emptying, and draining it held the frame -- and with it
    /// every request the interface sends the host -- until a plugin's panel
    /// gave up waiting.
    #[cfg(target_os = "linux")]
    pub fn pump(&self) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(4);
        while gtk::events_pending() && std::time::Instant::now() < deadline {
            gtk::main_iteration_do(false);
        }
    }
}
