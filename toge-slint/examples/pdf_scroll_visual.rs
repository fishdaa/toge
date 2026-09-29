slint::include_modules!();
#[path = "../src/preview.rs"]
mod preview;

use slint::{ComponentHandle, ModelRc, StandardListViewItem, VecModel};
use std::rc::Rc;

fn item(text: &str) -> StandardListViewItem {
    let mut item = StandardListViewItem::default();
    item.text = text.into();
    item
}

fn row(name: &str, folder: &str) -> ModelRc<StandardListViewItem> {
    ModelRc::from(Rc::new(VecModel::from(vec![
        item(name),
        item(folder),
        item(""),
        item(""),
    ])))
}

fn main() -> Result<(), slint::PlatformError> {
    let folder = std::env::args().nth(1).expect("fixture folder");
    let ui = AppWindow::new()?;
    ui.set_rows(ModelRc::from(Rc::new(VecModel::from(vec![
        row("a_report.pdf", &folder),
        row("b_notes.txt", &folder),
    ]))));
    preview::connect(&ui);
    ui.show()?;
    ui.set_selected(0);
    let weak = ui.as_weak();
    slint::Timer::single_shot(std::time::Duration::from_secs(2), move || {
        if let Some(ui) = weak.upgrade() {
            ui.window().dispatch_event(slint::platform::WindowEvent::PointerScrolled {
                position: slint::LogicalPosition { x: 750.0, y: 300.0 },
                delta_x: 0.0,
                delta_y: -720.0,
            });
        }
    });
    let weak = ui.as_weak();
    slint::Timer::single_shot(std::time::Duration::from_secs(4), move || {
        if let Some(ui) = weak.upgrade() {
            ui.window().dispatch_event(slint::platform::WindowEvent::PointerScrolled {
                position: slint::LogicalPosition { x: 750.0, y: 300.0 },
                delta_x: 0.0,
                delta_y: -720.0,
            });
        }
    });
    let weak = ui.as_weak();
    slint::Timer::single_shot(std::time::Duration::from_secs(6), move || {
        if let Some(ui) = weak.upgrade() {
            ui.window().dispatch_event(slint::platform::WindowEvent::PointerScrolled {
                position: slint::LogicalPosition { x: 750.0, y: 300.0 },
                delta_x: 0.0,
                delta_y: 720.0,
            });
        }
    });
    let weak = ui.as_weak();
    slint::Timer::single_shot(std::time::Duration::from_secs(8), move || {
        if let Some(ui) = weak.upgrade() {
            ui.set_selected(1);
        }
    });
    ui.run()
}
