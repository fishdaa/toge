mod client;
mod format;
mod model;
mod worker;
use slint::{ComponentHandle, Model};
use std::{rc::Rc, sync::Arc};
slint::include_modules!();

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let ui = AppWindow::new()?;
    ui.set_rows(slint::ModelRc::from(Rc::new(model::Results::default())));
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
    let mailbox = Arc::new(worker::Mailbox::default());
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
    ui.on_sort_results(move |column, ascending| {
        if let Some(ui) = weak.upgrade() {
            let model = ui.get_rows();
            let results = model.as_any().downcast_ref::<model::Results>().unwrap();
            results.sort(column, ascending);
            ui.invoke_select_row(-1);
        }
    });
    let weak = ui.as_weak();
    let last_click = std::cell::RefCell::new(None::<(String, std::time::Instant)>);
    ui.on_row_clicked(move |index| {
        if let Some(ui) = weak.upgrade() {
            let model = ui.get_rows();
            let results = model.as_any().downcast_ref::<model::Results>().unwrap();
            let Some(path) = results.path(index) else {
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
    let weak = ui.as_weak();
    ui.on_action(move |action| {
        let Some(ui) = weak.upgrade() else {
            return;
        };
        let model = ui.get_rows();
        let results = model.as_any().downcast_ref::<model::Results>().unwrap();
        let Some(path) = results.path(ui.get_selected()) else {
            return;
        };
        let weak = ui.as_weak();
        std::thread::spawn(move || {
            if let Err(e) = file_action(&action, &path) {
                let _ = weak.upgrade_in_event_loop(move |ui| {
                    ui.set_status(format!("Action failed: {e}").into())
                });
            }
        });
    });
    let result = ui.run();
    mailbox.close();
    result?;
    Ok(())
}
fn file_action(action: &str, path: &str) -> std::io::Result<()> {
    use std::process::{Command, Stdio};
    if action == "copy" {
        use std::io::Write;
        for (program, args) in [
            ("wl-copy", vec![]),
            ("xclip", vec!["-selection", "clipboard"]),
            ("xsel", vec!["--clipboard", "--input"]),
        ] {
            let Ok(mut child) = Command::new(program)
                .args(args)
                .stdin(Stdio::piped())
                .spawn()
            else {
                continue;
            };
            child.stdin.take().unwrap().write_all(path.as_bytes())?;
            if child.wait()?.success() {
                return Ok(());
            }
        }
        return Err(std::io::Error::other(
            "Install wl-clipboard (Wayland) or xclip/xsel (X11)",
        ));
    }
    let target = if action == "parent" {
        std::path::Path::new(path)
            .parent()
            .unwrap_or(std::path::Path::new(path))
    } else {
        std::path::Path::new(path)
    };
    if Command::new("xdg-open").arg(target).status()?.success() {
        Ok(())
    } else {
        Err(std::io::Error::other("xdg-open failed"))
    }
}
