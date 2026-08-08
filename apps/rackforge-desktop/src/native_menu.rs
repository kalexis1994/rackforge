use anyhow::{Result, bail};
use eframe::CreationContext;
use std::sync::OnceLock;
use std::sync::mpsc::{self, Receiver, Sender};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    CreateSolidBrush, DT_LEFT, DT_NOPREFIX, DT_SINGLELINE, DT_VCENTER, DeleteObject, DrawTextW,
    FillRect, GetStockObject, InvalidateRect, NULL_PEN, RoundRect, SelectObject, SetBkMode,
    SetTextColor, TRANSPARENT,
};
use windows_sys::Win32::UI::Controls::{
    DRAWITEMSTRUCT, MEASUREITEMSTRUCT, ODS_DISABLED, ODS_GRAYED, ODS_SELECTED, ODT_MENU,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CallWindowProcW, CreateMenu, CreatePopupMenu, DrawMenuBar, EnableMenuItem,
    GWLP_WNDPROC, HMENU, MENUINFO, MF_BYCOMMAND, MF_ENABLED, MF_GRAYED, MF_OWNERDRAW, MF_POPUP,
    MF_SEPARATOR, MIM_BACKGROUND, SetMenu, SetMenuInfo, SetWindowLongPtrW, WM_COMMAND, WM_DRAWITEM,
    WM_MEASUREITEM, WNDPROC,
};
use wry::raw_window_handle::{HasWindowHandle, RawWindowHandle};

const FILE_INSTALL_PLUGIN: usize = 1001;
const FILE_OPEN_ROOT: usize = 1002;
const FILE_EXIT: usize = 1003;
const SETTINGS_OPEN: usize = 1004;
const VIEW_RELOAD: usize = 1101;
const VIEW_OPEN_BROWSER: usize = 1102;
const VIEW_DEVTOOLS: usize = 1103;
const VIEW_AUDIO_MIDI: usize = 1104;
const HELP_ABOUT: usize = 1201;

static MENU_SENDER: OnceLock<Sender<NativeMenuCommand>> = OnceLock::new();
static ORIGINAL_WINDOW_PROC: OnceLock<isize> = OnceLock::new();

#[repr(C)]
struct MenuLabel {
    text: Vec<u16>,
    top_level: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMenuCommand {
    InstallPlugin,
    OpenRoot,
    Settings,
    Exit,
    Reload,
    OpenBrowser,
    DeveloperTools,
    AudioMidiStatus,
    About,
}

pub struct NativeMenu {
    window: HWND,
    handle: HMENU,
    file_menu: HMENU,
    receiver: Receiver<NativeMenuCommand>,
    visible: bool,
    install_enabled: bool,
}

impl NativeMenu {
    pub fn new(creation: &CreationContext<'_>) -> Result<Self> {
        let window = match creation.window_handle()?.as_raw() {
            RawWindowHandle::Win32(handle) => handle.hwnd.get() as HWND,
            _ => bail!("RackForge native menu requires a Windows window handle"),
        };
        let (sender, receiver) = mpsc::channel();
        MENU_SENDER
            .set(sender)
            .map_err(|_| anyhow::anyhow!("RackForge native menu was initialized twice"))?;

        // SAFETY: all handles are created and attached on the UI thread. Menu
        // strings remain valid for the duration of each synchronous call.
        let (handle, file_menu) = unsafe {
            let menu = CreateMenu();
            let file = CreatePopupMenu();
            let settings = CreatePopupMenu();
            let view = CreatePopupMenu();
            let help = CreatePopupMenu();
            if menu.is_null()
                || file.is_null()
                || settings.is_null()
                || view.is_null()
                || help.is_null()
            {
                bail!("Windows could not create the RackForge application menu");
            }

            append_item(file, FILE_INSTALL_PLUGIN, "Install Plugin…")?;
            append_item(file, FILE_OPEN_ROOT, "Open RackForge Root")?;
            AppendMenuW(file, MF_SEPARATOR, 0, std::ptr::null());
            append_item(file, FILE_EXIT, "Exit")?;

            append_item(settings, SETTINGS_OPEN, "Settings…")?;

            append_item(view, VIEW_RELOAD, "Reload")?;
            append_item(view, VIEW_OPEN_BROWSER, "Open in Browser")?;
            append_item(view, VIEW_AUDIO_MIDI, "Audio & MIDI Status")?;
            if cfg!(debug_assertions) {
                append_item(view, VIEW_DEVTOOLS, "Developer Tools")?;
            }

            append_item(help, HELP_ABOUT, "About RackForge")?;
            append_popup(menu, file, "File")?;
            append_popup(menu, settings, "Settings")?;
            append_popup(menu, view, "View")?;
            append_popup(menu, help, "Help")?;
            apply_background(menu)?;
            apply_background(file)?;
            apply_background(settings)?;
            apply_background(view)?;
            apply_background(help)?;

            let previous = SetWindowLongPtrW(
                window,
                GWLP_WNDPROC,
                menu_window_proc as *const () as usize as isize,
            );
            if previous == 0 {
                bail!("Windows could not connect the RackForge application menu");
            }
            ORIGINAL_WINDOW_PROC
                .set(previous)
                .map_err(|_| anyhow::anyhow!("RackForge window procedure was initialized twice"))?;
            (menu, file)
        };

        Ok(Self {
            window,
            handle,
            file_menu,
            receiver,
            visible: false,
            install_enabled: true,
        })
    }

    pub fn show(&mut self) -> Result<()> {
        if !self.visible {
            // SAFETY: window and menu remain owned by this application.
            if unsafe { SetMenu(self.window, self.handle) } == 0 {
                bail!("Windows could not display the RackForge application menu");
            }
            unsafe { DrawMenuBar(self.window) };
            self.visible = true;
        }
        Ok(())
    }

    pub fn hide(&mut self) -> Result<()> {
        if self.visible {
            // SAFETY: detaching a menu from our live top-level window is valid.
            if unsafe { SetMenu(self.window, std::ptr::null_mut()) } == 0 {
                bail!("Windows could not hide the RackForge application menu");
            }
            unsafe { DrawMenuBar(self.window) };
            self.visible = false;
        }
        Ok(())
    }

    pub fn drain(&self) -> impl Iterator<Item = NativeMenuCommand> + '_ {
        self.receiver.try_iter()
    }

    pub fn set_install_enabled(&mut self, enabled: bool) {
        if self.install_enabled == enabled {
            return;
        }
        let flags = MF_BYCOMMAND | if enabled { MF_ENABLED } else { MF_GRAYED };
        // SAFETY: file_menu belongs to the live menu attached to this window.
        unsafe {
            EnableMenuItem(self.file_menu, FILE_INSTALL_PLUGIN as u32, flags);
            DrawMenuBar(self.window);
        }
        self.install_enabled = enabled;
    }
}

unsafe fn append_item(menu: HMENU, id: usize, label: &str) -> Result<()> {
    let data = Box::into_raw(Box::new(MenuLabel {
        text: wide(label),
        top_level: false,
    }));
    if unsafe { AppendMenuW(menu, MF_OWNERDRAW, id, data.cast()) } == 0 {
        bail!("adding {label:?} to the RackForge application menu");
    }
    Ok(())
}

unsafe fn append_popup(menu: HMENU, popup: HMENU, label: &str) -> Result<()> {
    let data = Box::into_raw(Box::new(MenuLabel {
        text: wide(label),
        top_level: true,
    }));
    if unsafe { AppendMenuW(menu, MF_POPUP | MF_OWNERDRAW, popup as usize, data.cast()) } == 0 {
        bail!("adding a popup to the RackForge application menu");
    }
    Ok(())
}

unsafe fn apply_background(menu: HMENU) -> Result<()> {
    let brush = unsafe { CreateSolidBrush(rgb(9, 24, 34)) };
    let info = MENUINFO {
        cbSize: std::mem::size_of::<MENUINFO>() as u32,
        fMask: MIM_BACKGROUND,
        dwStyle: 0,
        cyMax: 0,
        hbrBack: brush,
        dwContextHelpID: 0,
        dwMenuData: 0,
    };
    if unsafe { SetMenuInfo(menu, &info) } == 0 {
        bail!("applying the RackForge application menu theme");
    }
    Ok(())
}

const fn rgb(red: u32, green: u32, blue: u32) -> u32 {
    red | (green << 8) | (blue << 16)
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

unsafe extern "system" fn menu_window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_MEASUREITEM && lparam != 0 {
        let measure = unsafe { &mut *(lparam as *mut MEASUREITEMSTRUCT) };
        if measure.CtlType == ODT_MENU && measure.itemData != 0 {
            let label = unsafe { &*(measure.itemData as *const MenuLabel) };
            let characters = label.text.len().saturating_sub(1) as u32;
            measure.itemWidth =
                characters.saturating_mul(8) + if label.top_level { 28 } else { 42 };
            measure.itemHeight = if label.top_level { 29 } else { 34 };
            return 1;
        }
    }
    if message == WM_DRAWITEM && lparam != 0 {
        let draw = unsafe { &mut *(lparam as *mut DRAWITEMSTRUCT) };
        if draw.CtlType == ODT_MENU && draw.itemData != 0 {
            let label = unsafe { &*(draw.itemData as *const MenuLabel) };
            let selected = draw.itemState & ODS_SELECTED != 0;
            let disabled = draw.itemState & (ODS_DISABLED | ODS_GRAYED) != 0;
            let background = rgb(9, 24, 34);
            let foreground = if selected {
                rgb(2, 16, 22)
            } else if disabled {
                rgb(92, 112, 121)
            } else {
                rgb(226, 242, 245)
            };
            let background_brush = unsafe { CreateSolidBrush(background) };
            unsafe {
                FillRect(draw.hDC, &draw.rcItem, background_brush);
                DeleteObject(background_brush);
            }
            if selected {
                let accent = unsafe { CreateSolidBrush(rgb(92, 226, 245)) };
                let old_brush = unsafe { SelectObject(draw.hDC, accent) };
                let old_pen = unsafe { SelectObject(draw.hDC, GetStockObject(NULL_PEN)) };
                let inset_x = if label.top_level { 3 } else { 5 };
                unsafe {
                    RoundRect(
                        draw.hDC,
                        draw.rcItem.left + inset_x,
                        draw.rcItem.top + 3,
                        draw.rcItem.right - inset_x,
                        draw.rcItem.bottom - 3,
                        12,
                        12,
                    );
                    SelectObject(draw.hDC, old_pen);
                    SelectObject(draw.hDC, old_brush);
                    DeleteObject(accent);
                }
            }
            unsafe {
                SetBkMode(draw.hDC, TRANSPARENT as i32);
                SetTextColor(draw.hDC, foreground);
            }
            let mut text_rect = draw.rcItem;
            text_rect.left += if label.top_level { 14 } else { 21 };
            text_rect.right -= 14;
            unsafe {
                DrawTextW(
                    draw.hDC,
                    label.text.as_ptr(),
                    label.text.len().saturating_sub(1) as i32,
                    &mut text_rect,
                    DT_LEFT | DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX,
                )
            };
            return 1;
        }
    }
    if message == WM_COMMAND {
        let command = match wparam & 0xffff {
            FILE_INSTALL_PLUGIN => Some(NativeMenuCommand::InstallPlugin),
            FILE_OPEN_ROOT => Some(NativeMenuCommand::OpenRoot),
            SETTINGS_OPEN => Some(NativeMenuCommand::Settings),
            FILE_EXIT => Some(NativeMenuCommand::Exit),
            VIEW_RELOAD => Some(NativeMenuCommand::Reload),
            VIEW_OPEN_BROWSER => Some(NativeMenuCommand::OpenBrowser),
            VIEW_DEVTOOLS => Some(NativeMenuCommand::DeveloperTools),
            VIEW_AUDIO_MIDI => Some(NativeMenuCommand::AudioMidiStatus),
            HELP_ABOUT => Some(NativeMenuCommand::About),
            _ => None,
        };
        if let Some(command) = command {
            if let Some(sender) = MENU_SENDER.get() {
                let _ = sender.send(command);
            }
            unsafe { InvalidateRect(window, std::ptr::null(), 0) };
            return 0;
        }
    }

    let Some(previous) = ORIGINAL_WINDOW_PROC.get().copied() else {
        return 0;
    };
    // SAFETY: the stored pointer is the WNDPROC returned by
    // SetWindowLongPtrW for this same live window.
    let previous: WNDPROC = unsafe { std::mem::transmute(previous) };
    unsafe { CallWindowProcW(previous, window, message, wparam, lparam) }
}
