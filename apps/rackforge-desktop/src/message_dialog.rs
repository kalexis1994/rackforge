//! Message boxes: a notice, or a yes-or-no question.
//!
//! Windows draws them through rfd. Linux draws them through GTK on the main
//! thread, the thread the webview's GTK already runs on. rfd's GTK backend
//! runs GTK on a thread of its own, and one process cannot run GTK on two;
//! its portal backend has no message box at all and falls back to `zenity`,
//! which a Flatpak runtime does not carry. File dialogs are another matter:
//! they go through rfd's portal backend, which any thread may call.

#[derive(Clone, Copy, Debug)]
pub(crate) enum Level {
    Info,
    Warning,
    Error,
}

#[cfg(windows)]
pub(crate) fn show(level: Level, title: &str, message: &str) {
    dialog(level, title, message)
        .set_buttons(rfd::MessageButtons::Ok)
        .show();
}

#[cfg(windows)]
pub(crate) fn ask(level: Level, title: &str, message: &str) -> bool {
    dialog(level, title, message)
        .set_buttons(rfd::MessageButtons::YesNo)
        .show()
        == rfd::MessageDialogResult::Yes
}

#[cfg(windows)]
fn dialog(level: Level, title: &str, message: &str) -> rfd::MessageDialog {
    rfd::MessageDialog::new()
        .set_title(title)
        .set_description(message)
        .set_level(match level {
            Level::Info => rfd::MessageLevel::Info,
            Level::Warning => rfd::MessageLevel::Warning,
            Level::Error => rfd::MessageLevel::Error,
        })
}

#[cfg(target_os = "linux")]
pub(crate) fn show(level: Level, title: &str, message: &str) {
    run(level, title, message, gtk::ButtonsType::Ok);
}

#[cfg(target_os = "linux")]
pub(crate) fn ask(level: Level, title: &str, message: &str) -> bool {
    run(level, title, message, gtk::ButtonsType::YesNo) == gtk::ResponseType::Yes
}

/// Runs a modal GTK message dialog to its answer. Called on the main thread
/// only: from the window's frame, or before the window exists.
#[cfg(target_os = "linux")]
fn run(level: Level, title: &str, message: &str, buttons: gtk::ButtonsType) -> gtk::ResponseType {
    use gtk::prelude::*;

    if let Err(error) = gtk::init() {
        eprintln!("DESKTOP_DIALOG_UNAVAILABLE title={title:?} reason={error}");
        return gtk::ResponseType::None;
    }
    let dialog = gtk::MessageDialog::new(
        None::<&gtk::Window>,
        gtk::DialogFlags::MODAL,
        match level {
            Level::Info => gtk::MessageType::Info,
            Level::Warning => gtk::MessageType::Warning,
            Level::Error => gtk::MessageType::Error,
        },
        buttons,
        message,
    );
    dialog.set_title(title);
    let response = dialog.run();
    dialog.close();
    // The dialog's own teardown arrives as events; drain them so it leaves
    // the screen now rather than at the next frame.
    while gtk::events_pending() {
        gtk::main_iteration_do(false);
    }
    response
}
