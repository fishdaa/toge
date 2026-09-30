//! Native Slint fixture. Pass a folder containing clip.mp4, notes.txt,
//! and an invalid broken.mp4. Record the window before the first click.
//! Add an output directory as the second argument for offscreen Slint recording.
slint::include_modules!();
#[path = "../src/preview.rs"]
mod preview;

use slint::platform::{PointerEventButton, WindowEvent};
use slint::{ComponentHandle, LogicalPosition, ModelRc, StandardListViewItem, VecModel};
use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

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

fn main() -> Result<(), slint::PlatformError> {
    let folder = std::env::args().nth(1).expect("fixture folder");
    // The visual runner can delay duration probes and fail fallback.mp4's probe.
    let poster_test = std::env::var_os("TOGE_VIDEO_POSTER_TEST").is_some();
    if let Some(output) = std::env::args().nth(2) {
        offscreen::install(output.into());
    }
    let ui = AppWindow::new()?;
    ui.set_busy(false);
    ui.set_status("Video preview visual test".into());
    ui.set_index_status("Local fixture".into());
    ui.set_rows(ModelRc::new(VecModel::from(vec![
        row("clip.mp4", &folder),
        row("notes.txt", &folder),
        row("broken.mp4", &folder),
        row("fallback.mp4", &folder),
    ])));
    preview::connect(&ui);
    ui.show()?;
    let step = Rc::new(Cell::new(0));
    let weak = ui.as_weak();
    let timer = slint::Timer::default();
    timer.start(slint::TimerMode::Repeated, Duration::from_secs(1), move || {
        let ui = weak.upgrade().unwrap();
        let size = ui.window().size().to_logical(ui.window().scale_factor());
        let stage = step.get() + 1;
        step.set(stage);
        println!("stage {stage}: selected={} ready={} frame={} message={}",
            ui.get_selected(), ui.get_preview_ready(), ui.get_preview_video_frame(),
            ui.get_preview_message());
        match stage {
            1 => { click(&ui, 100.0, 122.0); assert_eq!(ui.get_selected(), 0); }
            3 => {
                assert!(ui.get_preview_ready());
                assert!(ui.get_preview_video_active());
                if poster_test {
                    assert_eq!(ui.get_preview_video_frame(), 0);
                    assert!(ui.get_preview_message().contains("Loading video frames"));
                    println!("PASS: first frame visible while duration inspection is delayed");
                } else {
                    assert!((1..=5).contains(&ui.get_preview_video_frame()));
                }
            }
            5 => {
                assert!(ui.get_preview_ready());
                assert!((1..=5).contains(&ui.get_preview_video_frame()));
            }
            7 => {
                assert!(ui.get_preview_ready());
                assert!((1..=5).contains(&ui.get_preview_video_frame()));
                click(&ui, 100.0, 158.0);
                assert_eq!(ui.get_selected(), 1);
            }
            8 => {
                assert!(ui.get_preview_text_ready());
                assert!(!ui.get_preview_video_active());
                assert_eq!(ui.get_preview_video_frame(), 0);
                click(&ui, 100.0, 192.0);
                assert_eq!(ui.get_selected(), 2);
            }
            9 => {
                assert!(!ui.get_preview_ready());
                assert_eq!(ui.get_preview_video_frame(), 0);
                assert!(ui.get_preview_message().contains("Cannot inspect video"));
                click(&ui, 100.0, 122.0);
            }
            11 => {
                assert!(ui.get_preview_ready());
                let position = LogicalPosition::new(size.width - 304.0, 300.0);
                ui.window().dispatch_event(WindowEvent::PointerPressed { position, button: PointerEventButton::Left });
                let position = LogicalPosition::new(size.width - 184.0, 300.0);
                ui.window().dispatch_event(WindowEvent::PointerMoved { position });
                ui.window().dispatch_event(WindowEvent::PointerReleased { position, button: PointerEventButton::Left });
                assert!(ui.get_preview_width() <= 185.0);
            }
            12 => { assert!(ui.get_preview_ready()); ui.hide().unwrap(); }
            13 => { ui.show().unwrap(); }
            14 => {
                assert!(ui.get_preview_video_active());
                // Reselecting after the window is shown starts a fresh five-frame cycle.
                click(&ui, 100.0, 158.0);
                click(&ui, 100.0, 122.0);
            }
            16 => {
                assert!(ui.get_preview_ready());
                assert!((0..=5).contains(&ui.get_preview_video_frame()));
                println!("PASS: sampled-frame loop, selection, error, pane resize, hiding and reselection");
            }
            17 if !poster_test => { slint::quit_event_loop().unwrap(); }
            18 if poster_test => {
                assert!((1..=5).contains(&ui.get_preview_video_frame()));
                click(&ui, 100.0, 226.0);
                assert_eq!(ui.get_selected(), 3);
            }
            20 if poster_test => {
                assert!(ui.get_preview_ready());
                assert_eq!(ui.get_preview_video_frame(), 0);
                assert!(ui.get_preview_message().contains("Loading video frames"));
            }
            22 if poster_test => {
                assert!(ui.get_preview_ready());
                assert_eq!(ui.get_preview_video_frame(), 0);
                assert!(ui.get_preview_message().contains("Cannot inspect video"));
                println!("PASS: first frame retained after sampling failure");
                click(&ui, 100.0, 122.0);
            }
            23 if poster_test => {
                assert!(ui.get_preview_ready());
                assert_eq!(ui.get_preview_video_frame(), 0);
                let key = slint::platform::Key::DownArrow.into();
                ui.window().dispatch_event(WindowEvent::KeyPressed { text: key });
                ui.window().dispatch_event(WindowEvent::KeyReleased {
                    text: slint::platform::Key::DownArrow.into(),
                });
                assert_eq!(ui.get_selected(), 1);
            }
            24 if poster_test => {
                assert!(ui.get_preview_text_ready());
                assert!(!ui.get_preview_video_active());
                assert_eq!(ui.get_preview_video_frame(), 0);
                println!("PASS: keyboard selection cancels delayed sampling and clears the poster");
                slint::quit_event_loop().unwrap();
            }
            _ => {}
        }
    });
    slint::run_event_loop_until_quit()
}
