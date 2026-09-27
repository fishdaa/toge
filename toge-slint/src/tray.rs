//! Status-area icon (freedesktop StatusNotifierItem). While it is registered,
//! closing the last window leaves the GUI running in the tray.
use crate::instance::Request;
use crate::windows;
use ksni::blocking::{Handle, TrayMethods};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Action {
    Window(Request),
    About,
    Quit,
}

/// Menu entries in display order; `None` is a separator.
const MENU: [Option<(&str, Action)>; 6] = [
    Some(("Show Window", Action::Window(Request::Show))),
    Some(("New Window", Action::Window(Request::NewWindow))),
    Some(("Toggle Window", Action::Window(Request::Toggle))),
    Some(("About Toge", Action::About)),
    None,
    Some(("Quit", Action::Quit)),
];

static RESIDENT: AtomicBool = AtomicBool::new(false);

/// Whether the tray icon is registered, so the GUI may run without windows.
pub fn resident() -> bool {
    RESIDENT.load(Ordering::Relaxed)
}

struct Tray;

impl ksni::Tray for Tray {
    fn id(&self) -> String {
        "toge".into()
    }
    fn title(&self) -> String {
        "Toge".into()
    }
    fn icon_name(&self) -> String {
        "system-search".into()
    }
    fn tool_tip(&self) -> ksni::ToolTip {
        ksni::ToolTip {
            title: "Toge".into(),
            ..Default::default()
        }
    }
    fn activate(&mut self, _x: i32, _y: i32) {
        dispatch(Action::Window(Request::Show));
    }
    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        MENU.iter()
            .map(|entry| match *entry {
                Some((label, action)) => ksni::menu::StandardItem {
                    label: label.into(),
                    activate: Box::new(move |_: &mut Self| dispatch(action)),
                    ..Default::default()
                }
                .into(),
                None => ksni::MenuItem::Separator,
            })
            .collect()
    }
    fn watcher_online(&self) {
        RESIDENT.store(true, Ordering::Relaxed);
    }
    fn watcher_offline(&self, reason: ksni::OfflineReason) -> bool {
        // Keep the service so the icon returns with the panel, but let closing
        // the last window quit while nothing can show it.
        eprintln!("Toge tray icon unavailable: {reason:?}");
        RESIDENT.store(false, Ordering::Relaxed);
        let _ = slint::invoke_from_event_loop(|| {
            if windows::count() == 0 {
                let _ = slint::quit_event_loop();
            }
        });
        true
    }
}

/// Called on the tray thread; window work happens on the Slint event loop.
fn dispatch(action: Action) {
    let _ = slint::invoke_from_event_loop(move || match action {
        Action::Window(request) => windows::handle(request),
        Action::About => windows::show_about(),
        Action::Quit => {
            let _ = slint::quit_event_loop();
        }
    });
}

static HANDLE: Mutex<Option<Handle<Tray>>> = Mutex::new(None);

/// Register the tray icon in the background so D-Bus never delays the first
/// window. Without a StatusNotifierItem host the GUI keeps its window-only
/// behavior.
pub fn start() {
    std::thread::spawn(|| match Tray.spawn() {
        Ok(handle) => {
            RESIDENT.store(true, Ordering::Relaxed);
            *HANDLE.lock().unwrap() = Some(handle);
        }
        Err(error) => eprintln!("Toge tray icon unavailable: {error}"),
    });
}

/// Remove the tray icon before the process exits.
pub fn stop() {
    RESIDENT.store(false, Ordering::Relaxed);
    if let Some(handle) = HANDLE.lock().unwrap().take() {
        handle.shutdown().wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_offers_window_requests_about_and_quit() {
        let actions: Vec<_> = MENU.iter().flatten().map(|(_, action)| *action).collect();
        assert_eq!(
            actions,
            [
                Action::Window(Request::Show),
                Action::Window(Request::NewWindow),
                Action::Window(Request::Toggle),
                Action::About,
                Action::Quit,
            ]
        );
        assert!(MENU[MENU.len() - 2].is_none(), "Quit is set apart");
    }
}
