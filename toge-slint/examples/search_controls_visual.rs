//! Native search-controls verification against an isolated, real daemon.
//! Run with `TOGE_SOCKET` and XDG paths pointing at the visual fixture profile.
#![allow(
    dead_code,
    reason = "reuse the real app modules without launching its tray or portal integration"
)]

#[path = "../src/actions.rs"]
mod actions;
#[path = "../src/client.rs"]
mod client;
#[path = "../src/format.rs"]
mod format;
#[path = "../src/global_shortcuts.rs"]
mod global_shortcuts;
#[path = "../src/instance.rs"]
mod instance;
#[path = "../src/model.rs"]
mod model;
#[path = "../src/preferences.rs"]
mod preferences;
#[path = "../src/preview.rs"]
mod preview;
#[path = "../src/search_controls.rs"]
mod search_controls;
#[path = "../src/shortcuts.rs"]
mod shortcuts;
#[path = "../src/tray.rs"]
mod tray;
#[path = "../src/windows.rs"]
mod windows;
#[path = "../src/worker.rs"]
mod worker;
slint::include_modules!();

use slint::platform::{Key, PointerEventButton, WindowEvent};
use slint::{ComponentHandle, LogicalPosition, LogicalSize};
use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

fn key(ui: &AppWindow, text: slint::SharedString) {
    ui.window()
        .dispatch_event(WindowEvent::KeyPressed { text: text.clone() });
    ui.window()
        .dispatch_event(WindowEvent::KeyReleased { text });
}

fn click(ui: &AppWindow, position: LogicalPosition) {
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

fn replace_query(ui: &AppWindow, text: &str) {
    ui.invoke_focus_search();
    ui.window().dispatch_event(WindowEvent::KeyPressed {
        text: Key::Control.into(),
    });
    key(ui, "a".into());
    ui.window().dispatch_event(WindowEvent::KeyReleased {
        text: Key::Control.into(),
    });
    key(ui, Key::Backspace.into());
    for ch in text.chars() {
        key(ui, ch.to_string().into());
    }
}

fn focus_control(ui: &AppWindow, tabs: usize) {
    ui.invoke_focus_search();
    for _ in 0..tabs {
        key(ui, Key::Tab.into());
    }
}

fn ready(ui: &AppWindow, total: usize) {
    assert!(!ui.get_busy(), "still busy: {}", ui.get_status());
    assert!(!ui.get_has_error(), "{}", ui.get_status());
    assert_eq!(
        worker::results(ui).total(),
        total,
        "query={}",
        ui.get_query_text()
    );
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = std::env::args().nth(1).expect("fixture root");
    let ui = windows::open()?;
    let timer = slint::Timer::default();
    let step = Rc::new(Cell::new(0));
    let weak = ui.as_weak();
    timer.start(slint::TimerMode::Repeated, Duration::from_secs(1), move || {
        let ui = weak.upgrade().unwrap();
        let stage = step.get() + 1;
        step.set(stage);
        println!("stage {stage}: query={} rows={} busy={} error={} status={}", ui.get_query_text(), worker::results(&ui).total(), ui.get_busy(), ui.get_has_error(), ui.get_status());
        match stage {
            1 => { assert!(!ui.get_busy()); replace_query(&ui, "Report"); assert!(ui.get_busy(), "typing must enter the loading state"); }
            2 => { ready(&ui, 513); focus_control(&ui, 2); key(&ui, " ".into()); assert!(ui.get_match_case()); }
            3 => { ready(&ui, 257); focus_control(&ui, 3); key(&ui, " ".into()); assert!(ui.get_match_whole_word()); }
            4 => { ready(&ui, 256); replace_query(&ui, "^Report-[0-9]+\\.pdf$"); focus_control(&ui, 5); key(&ui, " ".into()); assert!(ui.get_regex_enabled()); }
            5 => { ready(&ui, 512); focus_control(&ui, 2); key(&ui, " ".into()); }
            6 => { ready(&ui, 256); replace_query(&ui, ""); focus_control(&ui, 1); key(&ui, Key::Return.into()); }
            7 => { key(&ui, Key::DownArrow.into()); assert_eq!(ui.get_filter_preset(), 1); }
            8 => { ready(&ui, 1026); key(&ui, Key::DownArrow.into()); assert_eq!(ui.get_filter_preset(), 2); }
            9 => { ready(&ui, 2); key(&ui, Key::DownArrow.into()); assert_eq!(ui.get_filter_preset(), 3); }
            10 | 12 | 13 => { ready(&ui, 128); key(&ui, Key::DownArrow.into()); }
            11 => { ready(&ui, 514); key(&ui, Key::DownArrow.into()); }
            14 => { ready(&ui, 128); key(&ui, Key::Escape.into()); replace_query(&ui, "file: Nested"); }
            15 => { ready(&ui, 0); focus_control(&ui, 4); key(&ui, " ".into()); assert!(ui.get_match_path()); }
            16 => { ready(&ui, 1); replace_query(&ui, &format!("parent:\"{root}\" ext:pdf")); }
            17 => { ready(&ui, 513); assert_eq!(ui.get_filter_preset(), 8); replace_query(&ui, "attrib:H"); }
            18 => { ready(&ui, 1); replace_query(&ui, "child:foo"); }
            19 => { assert!(ui.get_has_error()); assert!(ui.get_has_query_error()); assert!(ui.get_status().contains("unsupported search filter: child:")); assert!(!ui.get_status().contains("reconnect")); replace_query(&ui, "video:"); }
            20 => { ready(&ui, 128); assert_eq!(ui.get_filter_preset(), 6); replace_query(&ui, "regex:["); }
            21 => { assert!(ui.get_has_error()); assert!(ui.get_has_query_error()); assert!(ui.get_regex_enabled()); click(&ui, LogicalPosition::new(484.0, 80.0)); assert!(!ui.get_regex_enabled()); }
            22 => { ready(&ui, 0); replace_query(&ui, "file:"); }
            23 => { ready(&ui, 1026); click(&ui, LogicalPosition::new(120.0, 210.0)); key(&ui, Key::PageDown.into()); }
            24 => { assert!(ui.get_selected() > 0); ui.window().dispatch_event(WindowEvent::PointerScrolled { position: LogicalPosition::new(150.0, 350.0), delta_x: 0.0, delta_y: -700.0 }); }
            25 => {
                if std::env::var_os("NIRI_SOCKET").is_some() {
                    let output = std::process::Command::new("niri").args(["msg", "--json", "windows"]).output().unwrap();
                    assert!(output.status.success());
                    let windows: Vec<serde_json::Value> = serde_json::from_slice(&output.stdout).unwrap();
                    let window = windows.iter().find(|window| window["pid"].as_u64() == Some(u64::from(std::process::id()))).expect("native fixture window");
                    let id = window["id"].as_u64().unwrap().to_string();
                    println!("Resizing native fixture window {id}");
                    assert!(std::process::Command::new("niri").args(["msg", "action", "move-window-to-floating", "--id", &id]).status().unwrap().success());
                    assert!(std::process::Command::new("niri").args(["msg", "action", "set-window-width", "--id", &id, "560"]).status().unwrap().success());
                }
                ui.window().set_size(LogicalSize::new(560.0, 720.0));
            }
            26 => {
                if std::env::var_os("NIRI_SOCKET").is_some() {
                    let output = std::process::Command::new("niri").args(["msg", "--json", "windows"]).output().unwrap();
                    let windows: Vec<serde_json::Value> = serde_json::from_slice(&output.stdout).unwrap();
                    let window = windows.iter().find(|window| window["pid"].as_u64() == Some(u64::from(std::process::id()))).unwrap();
                    println!("Native window layout: {}", window["layout"]);
                    let id = window["id"].as_u64().unwrap().to_string();
                    assert!(std::process::Command::new("niri").args(["msg", "action", "set-window-width", "--id", &id, "560"]).status().unwrap().success());
                }
            }
            27 => { let size = ui.window().size().to_logical(ui.window().scale_factor()); assert!(size.width < 700.0, "narrow fixture width={}", size.width); focus_control(&ui, 2); key(&ui, " ".into()); assert!(ui.get_match_case()); }
            28 => { ready(&ui, 1026); println!("PASS: native search controls, all presets, typed synchronization, quoted parent/hidden filters, error recovery, loading, focus, keyboard navigation, scrolling and narrow layout"); }
            30 => { windows::shutdown(); slint::quit_event_loop().unwrap(); }
            _ => {}
        }
    });
    slint::run_event_loop_until_quit()?;
    Ok(())
}
