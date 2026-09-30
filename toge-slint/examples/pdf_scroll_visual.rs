//! Shared Slint preview fixture: PDFs, code/text, sheets, audio, SVG and desktop thumbnails.
//! Pass a fixture directory; optionally pass a frame-output directory for offscreen verification.
slint::include_modules!();
#[path = "../src/preview.rs"]
mod preview;

use slint::platform::{PointerEventButton, WindowEvent};
use slint::{ComponentHandle, LogicalPosition, ModelRc, StandardListViewItem, VecModel};
use std::cell::Cell;
use std::rc::Rc;
use std::time::{Duration, Instant};

mod offscreen {
    use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType};
    use slint::platform::{EventLoopProxy, Platform, PlatformError, WindowAdapter};
    use std::io::Write;
    use std::path::PathBuf;
    use std::rc::Rc;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    };
    use std::time::{Duration, Instant};

    struct Proxy {
        send: mpsc::Sender<Box<dyn FnOnce() + Send>>,
        quit: Arc<AtomicBool>,
    }
    impl EventLoopProxy for Proxy {
        fn quit_event_loop(&self) -> Result<(), slint::EventLoopError> {
            self.quit.store(true, Ordering::Release);
            Ok(())
        }
        fn invoke_from_event_loop(
            &self,
            event: Box<dyn FnOnce() + Send>,
        ) -> Result<(), slint::EventLoopError> {
            self.send
                .send(event)
                .map_err(|_| slint::EventLoopError::EventLoopTerminated)
        }
    }

    struct Headless {
        window: Rc<MinimalSoftwareWindow>,
        receive: mpsc::Receiver<Box<dyn FnOnce() + Send>>,
        send: mpsc::Sender<Box<dyn FnOnce() + Send>>,
        quit: Arc<AtomicBool>,
        output: PathBuf,
    }
    impl Platform for Headless {
        fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, PlatformError> {
            Ok(self.window.clone())
        }
        fn new_event_loop_proxy(&self) -> Option<Box<dyn EventLoopProxy>> {
            Some(Box::new(Proxy {
                send: self.send.clone(),
                quit: self.quit.clone(),
            }))
        }
        fn run_event_loop(&self) -> Result<(), PlatformError> {
            let mut pixels = vec![slint::Rgb8Pixel::default(); 900 * 580];
            let mut next = Instant::now();
            let mut index = 0;
            while !self.quit.load(Ordering::Acquire) {
                while let Ok(event) = self.receive.try_recv() {
                    event();
                }
                slint::platform::update_timers_and_animations();
                if Instant::now() >= next {
                    self.window.draw_if_needed(|renderer| {
                        renderer.render(&mut pixels, 900);
                    });
                    let mut file = std::io::BufWriter::new(
                        std::fs::File::create(self.output.join(format!("frame-{index:05}.ppm")))
                            .unwrap(),
                    );
                    file.write_all(b"P6\n900 580\n255\n").unwrap();
                    for pixel in &pixels {
                        file.write_all(&[pixel.r, pixel.g, pixel.b]).unwrap();
                    }
                    index += 1;
                    next += Duration::from_millis(125);
                }
                std::thread::sleep(Duration::from_millis(3));
            }
            Ok(())
        }
    }

    pub fn install(output: PathBuf) {
        std::fs::create_dir_all(&output).unwrap();
        let window = MinimalSoftwareWindow::new(RepaintBufferType::ReusedBuffer);
        window.set_size(slint::PhysicalSize::new(900, 580));
        let (send, receive) = mpsc::channel();
        slint::platform::set_platform(Box::new(Headless {
            window,
            send,
            receive,
            quit: Arc::new(AtomicBool::new(false)),
            output,
        }))
        .unwrap();
    }
}

fn row(name: &str, folder: &str) -> ModelRc<StandardListViewItem> {
    ModelRc::new(VecModel::from(
        [name, folder, "", ""]
            .map(|text| {
                let mut item = StandardListViewItem::default();
                item.text = text.into();
                item
            })
            .to_vec(),
    ))
}
fn click(ui: &AppWindow, x: f32, y: f32) {
    let position = LogicalPosition::new(x, y);
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
fn select(ui: &AppWindow, index: i32) {
    #[allow(clippy::cast_precision_loss)]
    click(ui, 100.0, 122.0 + index as f32 * 37.0);
    assert_eq!(ui.get_selected(), index);
}
fn scroll(ui: &AppWindow, x: f32, y: f32) {
    ui.window().dispatch_event(WindowEvent::PointerScrolled {
        position: LogicalPosition::new(750.0, 330.0),
        delta_x: x,
        delta_y: y,
    });
}
fn key(ui: &AppWindow, key: slint::SharedString) {
    ui.window()
        .dispatch_event(WindowEvent::KeyPressed { text: key.clone() });
    ui.window()
        .dispatch_event(WindowEvent::KeyReleased { text: key });
}
fn assert_pdf_size(ui: &AppWindow) {
    assert!(ui.get_preview_ready(), "{}", ui.get_preview_message());
    let expected = ui.get_preview_pdf_width() * ui.window().scale_factor();
    #[allow(clippy::cast_precision_loss)]
    let actual = ui.get_preview_image().size().width as f32;
    println!("PDF bitmap width={actual}, viewport physical width={expected}");
    assert!((actual - expected).abs() <= 1.0);
}
fn main() -> Result<(), slint::PlatformError> {
    let folder = std::env::args().nth(1).expect("fixture folder");
    if let Some(output) = std::env::args().nth(2) {
        offscreen::install(output.into());
    }
    let ui = AppWindow::new()?;
    ui.set_busy(false);
    ui.set_status("Preview visual verification".into());
    ui.set_index_status("Synthetic local fixtures".into());
    ui.set_rows(ModelRc::new(VecModel::from(
        [
            "a_report.pdf",
            "b_code.rs",
            "c_notes.txt",
            "d_sheet.xlsx",
            "e_sheet.ods",
            "f_audio.wav",
            "g_cover.flac",
            "h_shape.svg",
            "i_cached.unknown",
            "j_broken.pdf",
        ]
        .map(|name| row(name, &folder))
        .to_vec(),
    )));
    ui.on_shortcut_action(|key, _, _, _, _| {
        if key == slint::SharedString::from(slint::platform::Key::DownArrow) {
            "down".into()
        } else if key == slint::SharedString::from(slint::platform::Key::UpArrow) {
            "up".into()
        } else {
            "".into()
        }
    });
    preview::connect(&ui);
    ui.show()?;
    let selected_at = Rc::new(Cell::new(None::<Instant>));
    let poll = slint::Timer::default();
    let weak = ui.as_weak();
    let selected_poll = selected_at.clone();
    poll.start(
        slint::TimerMode::Repeated,
        Duration::from_millis(10),
        move || {
            let ui = weak.upgrade().unwrap();
            if ui.get_preview_ready()
                && let Some(start) = selected_poll.take()
            {
                println!(
                    "First visible PDF after selection: {} ms",
                    start.elapsed().as_millis()
                );
            }
        },
    );
    let step = Rc::new(Cell::new(0));
    let weak = ui.as_weak();
    let timer = slint::Timer::default();
    timer.start(slint::TimerMode::Repeated, Duration::from_secs(1), move || {
        let ui = weak.upgrade().unwrap();
        let stage = step.get() + 1; step.set(stage);
        println!("stage {stage}: selected={} image={} text={} page={} message={}", ui.get_selected(), ui.get_preview_ready(), ui.get_preview_text_ready(), ui.get_preview_pdf_page(), ui.get_preview_message());
        match stage {
            1 => { selected_at.set(Some(Instant::now())); select(&ui, 0); }
            3 => { assert_pdf_size(&ui); assert_eq!(ui.get_preview_pdf_pages(), 2); }
            4 => {
                let size = ui.window().size().to_logical(ui.window().scale_factor());
                let position = LogicalPosition::new(size.width - 304.0, 300.0);
                ui.window().dispatch_event(WindowEvent::PointerPressed { position, button: PointerEventButton::Left });
                let position = LogicalPosition::new(size.width - 604.0, 300.0);
                ui.window().dispatch_event(WindowEvent::PointerMoved { position });
                ui.window().dispatch_event(WindowEvent::PointerReleased { position, button: PointerEventButton::Left });
                assert!(ui.get_preview_width() >= 550.0);
            }
            6 => { assert_pdf_size(&ui); scroll(&ui, 0.0, -950.0); }
            8 => { assert_eq!(ui.get_preview_pdf_page(), 2); assert_eq!(ui.get_preview_pdf_rendered_page(), 2); select(&ui, 1); }
            10 => {
                assert!(ui.get_preview_text_ready()); assert!(!ui.get_preview_wrap());
                assert!(ui.get_preview_text_label().starts_with("Code"));
                println!("Code columns={}, character width={}, content width={}", ui.get_preview_text_columns(), ui.get_preview_character_width(), ui.get_preview_text_content_width());
                click(&ui, 750.0, 300.0); assert!(ui.get_preview_text_has_focus());
                key(&ui, slint::platform::Key::PageDown.into());
                key(&ui, slint::platform::Key::RightArrow.into());
            }
            12 => {
                assert!(ui.get_preview_text_scroll_y() < 0.0);
                assert!(ui.get_preview_text_scroll_x() < 0.0);
                key(&ui, slint::platform::Key::DownArrow.into());
                assert_eq!(ui.get_selected(), 1, "preview arrows must not move result selection");
                scroll(&ui, -160.0, -240.0);
                let size = ui.window().size().to_logical(ui.window().scale_factor());
                click(&ui, size.width - 42.0, 134.0);
                assert!(ui.get_preview_wrap(), "Wrap button must switch the code view");
            }
            13 => { click(&ui, 750.0, 300.0); key(&ui, slint::platform::Key::Home.into()); assert!(ui.get_preview_text_scroll_y().abs() < 0.1); select(&ui, 2); }
            15 => { assert!(ui.get_preview_text_ready()); assert!(ui.get_preview_wrap()); assert!(ui.get_preview_text_scroll_y().abs() < 0.1); scroll(&ui, 0.0, -600.0); }
            16 => { assert!(ui.get_preview_text_scroll_y() < 0.0); select(&ui, 3); }
            18 => { assert!(ui.get_preview_text_ready()); assert!(!ui.get_preview_wrap()); assert!(ui.get_preview_copy_text().contains("Budget")); select(&ui, 4); }
            20 => { assert!(ui.get_preview_copy_text().contains("Inventory")); select(&ui, 5); }
            22 => { assert!(ui.get_preview_audio_active()); assert!(ui.get_preview_audio_details().contains("Sample track")); assert!(!ui.get_preview_ready()); select(&ui, 6); }
            24 => { assert!(ui.get_preview_audio_active()); assert!(ui.get_preview_ready()); select(&ui, 7); }
            26 => { assert!(ui.get_preview_ready(), "{}", ui.get_preview_message()); select(&ui, 8); }
            28 => { assert!(ui.get_preview_ready(), "desktop thumbnail must preview an otherwise unsupported type"); select(&ui, 9); }
            30 => { assert!(!ui.get_preview_ready()); assert!(!ui.get_preview_message().is_empty()); select(&ui, 0); select(&ui, 1); select(&ui, 2); }
            32 => { assert!(ui.get_preview_text_ready()); assert!(!ui.get_preview_pdf_active()); assert_eq!(ui.get_selected(), 2); println!("PASS: PDF size/resize/pages, code colors/focus/keyboard/scroll, text reset/wrap, XLSX/ODS, audio/art, SVG, desktop cache, errors and selection cancellation"); }
            34 => { slint::quit_event_loop().unwrap(); }
            _ => {}
        }
    });
    slint::run_event_loop_until_quit()
}
