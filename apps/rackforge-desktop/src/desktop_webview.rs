use anyhow::{Context, Result};
use eframe::CreationContext;
use wry::{
    Rect, WebView, WebViewBuilder,
    dpi::{LogicalPosition, LogicalSize},
};

pub struct DesktopWebView {
    view: WebView,
    current_url: Option<String>,
    visible: bool,
    bounds: Option<(i32, i32, u32, u32)>,
}

impl DesktopWebView {
    pub fn new(creation: &CreationContext<'_>) -> Result<Self> {
        let view = WebViewBuilder::new()
            .with_html(
                "<!doctype html><html><body style=\"margin:0;background:#e9e7e1\"></body></html>",
            )
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
