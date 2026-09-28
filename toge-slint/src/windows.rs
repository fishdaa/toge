//! Search windows. Each window has its own query, daemon session and worker;
//! table settings and the About window are shared.
use crate::instance::Request;
use crate::{AboutWindow, AppWindow, preferences, worker};
use slint::ComponentHandle;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

struct Entry {
    id: u64,
    ui: AppWindow,
    mailbox: Arc<worker::Mailbox>,
}

#[derive(Default)]
struct App {
    /// Most recently opened first. Toggle and Show act on the first window.
    windows: Vec<Entry>,
    next_id: u64,
    about: Option<AboutWindow>,
    preferences: Option<Rc<RefCell<preferences::UiState>>>,
}

thread_local! {
    static APP: RefCell<App> = RefCell::default();
}

pub fn handle(request: Request) {
    let result = match request {
        Request::Show => show(),
        Request::NewWindow => open().map(|_| ()),
        Request::Toggle => toggle(),
    };
    if let Err(error) = result {
        eprintln!("Could not open a Toge window: {error}");
    }
}

/// Number of open windows, visible or hidden.
pub fn count() -> usize {
    APP.with_borrow(|app| app.windows.len())
}

fn current() -> Option<AppWindow> {
    APP.with_borrow(|app| app.windows.first().map(|entry| entry.ui.clone_strong()))
}

fn show() -> Result<(), slint::PlatformError> {
    match current() {
        Some(ui) => reveal(&ui),
        None => open().map(|_| ()),
    }
}

fn toggle() -> Result<(), slint::PlatformError> {
    match current() {
        Some(ui) if ui.window().is_visible() => ui.hide(),
        Some(ui) => reveal(&ui),
        None => open().map(|_| ()),
    }
}

fn reveal(ui: &AppWindow) -> Result<(), slint::PlatformError> {
    ui.window().set_minimized(false);
    ui.show()
}

/// Open and show a new search window listing the whole index.
pub fn open() -> Result<AppWindow, slint::PlatformError> {
    let ui = AppWindow::new()?;
    ui.set_rows(slint::ModelRc::from(Rc::new(
        crate::model::Results::default(),
    )));
    let mailbox = Arc::new(worker::Mailbox::default());
    let state = APP.with_borrow_mut(|app| {
        app.preferences
            .get_or_insert_with(|| Rc::new(RefCell::new(preferences::load(&preferences::path()))))
            .clone()
    });
    preferences::connect(&ui, preferences::path(), state, mailbox.clone());
    ui.on_about(show_about);
    ui.on_new_window(|| handle(Request::NewWindow));
    let id = APP.with_borrow_mut(|app| {
        app.next_id += 1;
        app.next_id
    });
    ui.window().on_close_requested(move || {
        close(id);
        slint::CloseRequestResponse::HideWindow
    });
    for immediate in [false, true] {
        let m = mailbox.clone();
        let weak = ui.as_weak();
        let callback = move |text: slint::SharedString| {
            m.submit(text.to_string(), immediate);
            if let Some(ui) = weak.upgrade() {
                ui.set_busy(true);
                ui.set_has_error(false);
                ui.set_status("Searching…".into());
            }
        };
        if immediate {
            ui.on_submit(callback);
        } else {
            ui.on_query_edited(callback);
        }
    }
    let weak = ui.as_weak();
    let last_click = RefCell::new(None::<(String, std::time::Instant)>);
    ui.on_row_clicked(move |index| {
        if let Some(ui) = weak.upgrade() {
            let Some(path) = worker::results(&ui).path(index) else {
                return;
            };
            let now = std::time::Instant::now();
            let previous = last_click.borrow_mut().take();
            if previous.is_some_and(|(p, at)| {
                p == path && now.duration_since(at) < std::time::Duration::from_millis(400)
            }) {
                ui.invoke_action("open".into());
            } else {
                *last_click.borrow_mut() = Some((path, now));
            }
        }
    });
    crate::actions::connect(&ui);
    ui.show()?;
    worker::start(mailbox.clone(), ui.as_weak());
    mailbox.submit(String::new(), true);
    APP.with_borrow_mut(|app| {
        app.windows.insert(
            0,
            Entry {
                id,
                ui: ui.clone_strong(),
                mailbox,
            },
        );
    });
    Ok(ui)
}

/// Close one window for good; the GUI exits once no window remains, unless
/// the tray icon keeps it running.
fn close(id: u64) {
    let entry = APP.with_borrow_mut(|app| {
        let index = app.windows.iter().position(|entry| entry.id == id)?;
        Some(app.windows.remove(index))
    });
    let Some(entry) = entry else { return };
    // Discards the daemon's copy of this window's results and stops its threads.
    entry.mailbox.close();
    // The component is still running its close handler; drop it afterwards.
    slint::Timer::single_shot(std::time::Duration::ZERO, move || drop(entry));
    if count() == 0 && !crate::tray::resident() {
        let _ = slint::quit_event_loop();
    }
}

/// Close every window's session before the process exits.
pub fn shutdown() {
    let windows = APP.with_borrow_mut(|app| {
        app.about = None;
        std::mem::take(&mut app.windows)
    });
    for entry in windows {
        entry.mailbox.close();
    }
}

pub fn show_about() {
    let about =
        match APP.with_borrow(|app| app.about.as_ref().map(slint::ComponentHandle::clone_strong)) {
            Some(ui) => ui,
            None => match AboutWindow::new() {
                Ok(ui) => {
                    APP.with_borrow_mut(|app| app.about = Some(ui.clone_strong()));
                    ui
                }
                Err(error) => {
                    eprintln!("Could not open About: {error}");
                    return;
                }
            },
        };
    let _ = about.show();
}
