slint::include_modules!();
#[path = "../src/preview.rs"]
mod preview;

use slint::platform::{PointerEventButton, WindowEvent};
use slint::{ComponentHandle, LogicalPosition, ModelRc, StandardListViewItem, VecModel};
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

fn pointer(ui: &AppWindow, x: f32, y: f32) {
    let position = LogicalPosition { x, y };
    ui.window()
        .dispatch_event(WindowEvent::PointerMoved { position });
    ui.window().dispatch_event(WindowEvent::PointerPressed {
        position,
        button: PointerEventButton::Left,
    });
    ui.window().dispatch_event(WindowEvent::PointerReleased {
        position,
        button: PointerEventButton::Left,
    });
}
fn scroll(ui: &AppWindow, delta_x: f32, delta_y: f32) {
    ui.window().dispatch_event(WindowEvent::PointerScrolled {
        position: LogicalPosition {
            x: f32::from(u16::try_from(ui.window().size().width).unwrap_or(u16::MAX)) - 150.0,
            y: 300.0,
        },
        delta_x,
        delta_y,
    });
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
    let weak = ui.as_weak();
    slint::Timer::single_shot(std::time::Duration::from_millis(100), move || {
        if let Some(ui) = weak.upgrade() {
            ui.set_preview_width(600.0);
        }
    });
    ui.set_selected(0);
    let weak = ui.as_weak();
    slint::Timer::single_shot(std::time::Duration::from_secs(2), move || {
        if let Some(ui) = weak.upgrade() {
            pointer(
                &ui,
                f32::from(u16::try_from(ui.window().size().width).unwrap_or(u16::MAX)) - 68.0,
                120.0,
            ); // Zoom in from Fit
        }
    });
    let weak = ui.as_weak();
    slint::Timer::single_shot(std::time::Duration::from_secs(3), move || {
        if let Some(ui) = weak.upgrade() {
            scroll(&ui, -300.0, 0.0);
        }
    });
    let weak = ui.as_weak();
    slint::Timer::single_shot(std::time::Duration::from_secs(5), move || {
        if let Some(ui) = weak.upgrade() {
            pointer(
                &ui,
                f32::from(u16::try_from(ui.window().size().width).unwrap_or(u16::MAX)) - 32.0,
                120.0,
            ); // Return to smooth Fit
        }
    });
    let weak = ui.as_weak();
    slint::Timer::single_shot(std::time::Duration::from_secs(6), move || {
        if let Some(ui) = weak.upgrade() {
            scroll(&ui, 0.0, -500.0);
        }
    });
    let weak = ui.as_weak();
    slint::Timer::single_shot(std::time::Duration::from_secs(8), move || {
        if let Some(ui) = weak.upgrade() {
            scroll(&ui, 0.0, -1500.0);
        }
    });
    let weak = ui.as_weak();
    slint::Timer::single_shot(std::time::Duration::from_secs(11), move || {
        if let Some(ui) = weak.upgrade() {
            ui.set_selected(1);
        }
    });
    ui.run()
}
