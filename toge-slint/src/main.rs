mod access;
mod actions;
mod client;
mod format;
mod model;
mod preferences;
mod worker;
use slint::ComponentHandle;
use std::{rc::Rc, sync::Arc};
slint::include_modules!();

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args
        .first()
        .is_some_and(|arg| arg == "--request-watcher-access")
    {
        if args.len() != 2 {
            return Err("Usage: toge-slint --request-watcher-access path/to/toged".into());
        }
        return access::request(std::path::Path::new(&args[1]));
    }
    let ui = AppWindow::new()?;
    ui.set_rows(slint::ModelRc::from(Rc::new(model::Results::default())));
    let mailbox = Arc::new(worker::Mailbox::default());
    preferences::connect(&ui, preferences::path(), mailbox.clone());
    let about = Rc::new(std::cell::RefCell::new(None::<AboutWindow>));
    ui.on_about(move || {
        let mut window = about.borrow_mut();
        if window.is_none() {
            match AboutWindow::new() {
                Ok(ui) => *window = Some(ui),
                Err(error) => {
                    eprintln!("Could not open About: {error}");
                    return;
                }
            }
        }
        if let Some(ui) = window.as_ref() {
            let _ = ui.show();
        }
    });
    ui.window().on_close_requested(|| {
        let _ = slint::quit_event_loop();
        slint::CloseRequestResponse::HideWindow
    });
    ui.show()?;
    worker::start(mailbox.clone(), ui.as_weak());
    mailbox.submit(String::new(), true);
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
    let last_click = std::cell::RefCell::new(None::<(String, std::time::Instant)>);
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
    actions::connect(&ui);
    let result = ui.run();
    mailbox.close();
    result?;
    Ok(())
}
