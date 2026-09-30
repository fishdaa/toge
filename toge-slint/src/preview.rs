//! Bounded previews using installed system tools. Work stays off the UI thread;
//! one worker per window keeps only the newest selection.
use slint::{ComponentHandle, Rgba8Pixel, SharedPixelBuffer};
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{self, Read};
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

const MAX_IMAGE_BYTES: u64 = 32 * 1024 * 1024;
const MAX_PDF_BYTES: u64 = 64 * 1024 * 1024;
const MAX_DOCUMENT_BYTES: u64 = 64 * 1024 * 1024;
const MAX_TEXT_BYTES: usize = 64 * 1024;
// Keeps each wrapped Text item far below the software renderer's i16 limit.
const TEXT_CHUNK_CHARS: usize = 1_024;
const MAX_SVG_BYTES: u64 = 4 * 1024 * 1024;
const MAX_PIXELS: u64 = 24_000_000;
const MAX_SIDE: u32 = 8_192;
const PREVIEW_SIDE: u32 = 1_200;
// Supersample PDF glyphs before the Fit image is downscaled.
const PDF_RENDER_SIDE: u32 = 1_800;
const MAX_RENDER_BYTES: u64 = 10 * 1024 * 1024;
const RENDER_TIMEOUT: Duration = Duration::from_secs(8);
const DOCUMENT_TIMEOUT: Duration = Duration::from_secs(10);
const SETTLE: Duration = Duration::from_millis(75);

type Pixels = SharedPixelBuffer<Rgba8Pixel>;
const VIDEO_SIDE: u32 = 640;
const VIDEO_FRAMES: usize = 5;
const VIDEO_FRAMES_F32: f32 = 5.0;
const VIDEO_FRAME_INTERVAL: Duration = Duration::from_millis(850);

struct Request {
    path: String,
    page: u32,
    serial: u64,
}

enum Preview {
    Image(Pixels),
    Video {
        pixels: Pixels,
        frame: usize,
    },
    Pdf {
        page: u32,
        pages: u32,
        previous: Option<PdfPage>,
        current: PdfPage,
        next: Option<PdfPage>,
    },
    Text(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Raster,
    Svg,
    Pdf,
    Video,
    Document,
    Text,
    Unsupported,
}

fn kind(path: &str) -> Kind {
    let file = Path::new(path);
    if file
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            matches!(
                name.to_ascii_lowercase().as_str(),
                ".gitignore" | ".env" | "dockerfile"
            )
        })
    {
        return Kind::Text;
    }
    let ext = file
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "png" | "jpg" | "jpeg" | "webp" | "gif" | "bmp" | "ico" | "tif" | "tiff" | "tga"
        | "pnm" | "pbm" | "pgm" | "ppm" | "avif" | "heic" | "heif" => Kind::Raster,
        "svg" => Kind::Svg,
        "pdf" => Kind::Pdf,
        "mp4" | "mkv" | "webm" | "mov" | "avi" | "m4v" | "mpg" | "mpeg" | "ogv" | "3gp" | "mts"
        | "m2ts" | "wmv" | "flv" | "vob" => Kind::Video,
        "doc" | "docx" | "odt" | "rtf" => Kind::Document,
        "txt" | "text" | "md" | "markdown" | "log" | "csv" | "tsv" | "json" | "jsonl" | "toml"
        | "yaml" | "yml" | "xml" | "html" | "htm" | "css" | "js" | "jsx" | "ts" | "tsx" | "rs"
        | "py" | "sh" | "bash" | "zsh" | "c" | "h" | "cc" | "cpp" | "hpp" | "go" | "java"
        | "kt" | "swift" | "rb" | "php" | "sql" | "ini" | "conf" | "config" | "env" => Kind::Text,
        _ => Kind::Unsupported,
    }
}

type Pending = Arc<Mutex<Option<(u64, Result<Preview, &'static str>)>>>;

// The request callback is owned by the window. Its destruction must also
// interrupt streaming; a dropped Slint weak handle silently skips callbacks.
struct CancelOnDrop(Arc<AtomicU64>);
impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::AcqRel);
    }
}

fn publish(
    weak: &slint::Weak<crate::AppWindow>,
    pending: &Pending,
    serial: &Arc<AtomicU64>,
    id: u64,
    result: Result<Preview, &'static str>,
) -> bool {
    if serial.load(Ordering::Acquire) != id {
        return true;
    }
    let mut slot = pending.lock().unwrap();
    let scheduled = slot.is_some();
    *slot = Some((id, result));
    drop(slot);
    if scheduled {
        return true;
    }
    let pending = pending.clone();
    let serial = serial.clone();
    weak.upgrade_in_event_loop(move |ui| {
        let Some((id, result)) = pending.lock().unwrap().take() else {
            return;
        };
        if serial.load(Ordering::Acquire) != id {
            return;
        }
        if !ui.window().is_visible() && ui.get_preview_video_active() {
            serial.fetch_add(1, Ordering::AcqRel);
            return;
        }
        match result {
            Ok(Preview::Video { pixels, frame }) => {
                ui.set_preview_image(slint::Image::from_rgba8(pixels));
                ui.set_preview_video_frame(i32::try_from(frame + 1).unwrap_or(1));
                ui.set_preview_ready(true);
                ui.set_preview_message("".into());
            }
            Ok(Preview::Image(pixels)) => {
                ui.set_preview_image(slint::Image::from_rgba8(pixels));
                ui.set_preview_ready(true);
                ui.set_preview_message("".into());
            }
            Ok(Preview::Pdf {
                page,
                pages,
                previous,
                current,
                next,
            }) => {
                ui.set_preview_pdf_rendered_page(i32::try_from(page).unwrap_or(1));
                ui.set_preview_pdf_pages(i32::try_from(pages).unwrap_or(1));
                ui.set_preview_pdf_previous_ready(previous.is_some());
                if let Some(previous) = previous {
                    ui.set_preview_pdf_previous_image(slint::Image::from_rgba8(previous.full));
                    ui.set_preview_pdf_previous_fit_image(slint::Image::from_rgba8(previous.fit));
                } else {
                    ui.set_preview_pdf_previous_image(slint::Image::default());
                    ui.set_preview_pdf_previous_fit_image(slint::Image::default());
                }
                ui.set_preview_image(slint::Image::from_rgba8(current.full));
                ui.set_preview_pdf_fit_image(slint::Image::from_rgba8(current.fit));
                ui.set_preview_pdf_next_ready(next.is_some());
                if let Some(next) = next {
                    ui.set_preview_pdf_next_image(slint::Image::from_rgba8(next.full));
                    ui.set_preview_pdf_next_fit_image(slint::Image::from_rgba8(next.fit));
                } else {
                    ui.set_preview_pdf_next_image(slint::Image::default());
                    ui.set_preview_pdf_next_fit_image(slint::Image::default());
                }
                ui.set_preview_ready(true);
                ui.set_preview_message("".into());
            }
            Ok(Preview::Text(contents)) => {
                ui.set_preview_text(text_chunks(&contents));
                ui.set_preview_text_ready(true);
                ui.set_preview_message("".into());
            }
            Err(message) => {
                ui.set_preview_ready(false);
                ui.set_preview_video_frame(0);
                ui.set_preview_message(message.into());
            }
        }
    })
    .is_ok()
}

pub fn connect(ui: &crate::AppWindow) {
    let (send, receive) = mpsc::channel::<Request>();
    let serial = Arc::new(AtomicU64::new(0));
    let weak = ui.as_weak();
    let worker_serial = serial.clone();
    // Keep at most one decoded result waiting for the UI thread. A backed-up
    // event loop must not retain every image visited while scrolling.
    let pending = Arc::new(Mutex::new(None::<(u64, Result<Preview, &'static str>)>));
    let worker_pending = pending.clone();
    std::thread::spawn(move || {
        let mut pdf_cache = PdfCache::default();
        while let Ok(mut request) = receive.recv() {
            // Wait briefly for keyboard navigation to settle, retaining only the
            // last selection. A decode already in progress is discarded by serial.
            while let Ok(next) = receive.recv_timeout(SETTLE) {
                request = next;
            }
            if worker_serial.load(Ordering::Acquire) != request.serial {
                continue;
            }
            if kind(&request.path) == Kind::Video {
                let result = cycle_video_frames(&request, &worker_serial, |frame| {
                    publish(
                        &weak,
                        &worker_pending,
                        &worker_serial,
                        request.serial,
                        Ok(frame),
                    )
                });
                if let Err(message) = result {
                    publish(
                        &weak,
                        &worker_pending,
                        &worker_serial,
                        request.serial,
                        Err(message),
                    );
                }
                continue;
            }
            let result = load(
                &request.path,
                request.page,
                request.serial,
                &worker_serial,
                &mut pdf_cache,
            );
            if worker_serial.load(Ordering::Acquire) != request.serial {
                continue;
            }
            if !publish(
                &weak,
                &worker_pending,
                &worker_serial,
                request.serial,
                result,
            ) {
                break;
            }
        }
    });

    let send_scroll = send.clone();
    let serial_scroll = serial.clone();
    let pending_scroll = pending.clone();
    let cancel_on_drop = CancelOnDrop(serial.clone());
    let weak = ui.as_weak();
    ui.on_preview_requested(move |path, page| {
        let _keep_alive = &cancel_on_drop;
        let Some(ui) = weak.upgrade() else { return };
        let id = serial.fetch_add(1, Ordering::AcqRel) + 1;
        pending.lock().unwrap().take();
        ui.set_preview_ready(false);
        ui.set_preview_text_ready(false);
        ui.set_preview_text(slint::ModelRc::default());
        ui.set_preview_image(slint::Image::default());
        ui.set_preview_pdf_previous_image(slint::Image::default());
        ui.set_preview_pdf_next_image(slint::Image::default());
        ui.set_preview_pdf_fit_image(slint::Image::default());
        ui.set_preview_pdf_previous_fit_image(slint::Image::default());
        ui.set_preview_pdf_next_fit_image(slint::Image::default());
        ui.set_preview_pdf_previous_ready(false);
        ui.set_preview_pdf_next_ready(false);
        ui.set_preview_pdf_rendered_page(0);
        let path = path.to_string();
        let file_kind = kind(&path);
        ui.set_preview_pdf_active(!path.is_empty() && file_kind == Kind::Pdf);
        ui.set_preview_video_active(!path.is_empty() && file_kind == Kind::Video);
        ui.set_preview_video_frame(0);
        ui.set_preview_pdf_page(page.max(1));
        if page <= 1 {
            ui.set_preview_pdf_pages(0);
        }
        if path.is_empty() {
            ui.set_preview_message(
                if ui.get_selected() >= 0 {
                    "Loading result…"
                } else {
                    "Select a file"
                }
                .into(),
            );
        } else {
            let message = match file_kind {
                Kind::Raster | Kind::Svg => "Loading image…",
                Kind::Pdf => "Loading PDF…",
                Kind::Video => "Loading video frames…",
                Kind::Document => "Loading document…",
                Kind::Text => "Loading text…",
                Kind::Unsupported => "Preview unavailable for this file type",
            };
            ui.set_preview_message(message.into());
            if file_kind != Kind::Unsupported {
                let _ = send.send(Request {
                    path,
                    page: u32::try_from(page.max(1)).unwrap_or(1),
                    serial: id,
                });
            }
        }
    });

    let weak = ui.as_weak();
    ui.on_preview_pdf_scrolled(move |path, position| {
        let Some(ui) = weak.upgrade() else { return };
        let pages = ui.get_preview_pdf_pages();
        if pages < 1 || path.is_empty() {
            return;
        }
        // The position is bounded by the scroll range and the page count.
        #[allow(clippy::cast_possible_truncation)]
        let page = (position.floor() as i32 + 1).clamp(1, pages);
        if page == ui.get_preview_pdf_page() {
            return;
        }
        ui.set_preview_pdf_page(page);
        ui.set_preview_message(format!("Loading page {page}…").into());
        let id = serial_scroll.fetch_add(1, Ordering::AcqRel) + 1;
        pending_scroll.lock().unwrap().take();
        let _ = send_scroll.send(Request {
            path: path.to_string(),
            page: u32::try_from(page).unwrap_or(1),
            serial: id,
        });
    });
}

#[derive(Clone)]
struct PdfPage {
    full: Pixels,
    fit: Pixels,
}

#[derive(Default)]
struct PdfCache {
    path: PathBuf,
    pages: u32,
    images: BTreeMap<u32, PdfPage>,
}

fn load(
    path: &str,
    page: u32,
    request: u64,
    serial: &AtomicU64,
    pdf_cache: &mut PdfCache,
) -> Result<Preview, &'static str> {
    let path = Path::new(path);
    let metadata = std::fs::metadata(path).map_err(|_| "File is unavailable")?;
    if !metadata.is_file() {
        return Err("Folder preview unavailable");
    }
    match kind(path.to_str().unwrap_or("")) {
        Kind::Raster | Kind::Svg => load_image(path, request, serial).map(Preview::Image),
        Kind::Pdf => load_pdf_preview(path, page, request, serial, pdf_cache),
        Kind::Video => unreachable!("video uses the frame cycle worker"),
        Kind::Document => load_document(path, request, serial),
        Kind::Text => load_text(path).map(Preview::Text),
        Kind::Unsupported => Err("Preview unavailable for this file type"),
    }
}

// Keep scratch files private and remove them even when a preview is cancelled.
struct ScratchDir(PathBuf);

impl ScratchDir {
    fn new() -> io::Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        for _ in 0..100 {
            let id = NEXT.fetch_add(1, Ordering::Relaxed);
            let path =
                std::env::temp_dir().join(format!("toge-preview-{}-{id}", std::process::id()));
            let mut builder = fs::DirBuilder::new();
            builder.mode(0o700);
            match builder.create(&path) {
                Ok(()) => return Ok(Self(path)),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error),
            }
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "preview scratch names exhausted",
        ))
    }

    fn join(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for ScratchDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

// A single process helper carries the bounds and errors for every backend.
#[allow(clippy::too_many_arguments)]
fn run_command(
    mut command: Command,
    monitor: &Path,
    max_bytes: u64,
    timeout: Duration,
    request: u64,
    serial: &AtomicU64,
    unavailable: &'static str,
    failed: &'static str,
) -> Result<(), &'static str> {
    command.stderr(Stdio::null());
    let mut child = command.spawn().map_err(|_| unavailable)?;
    let started = Instant::now();
    loop {
        if serial.load(Ordering::Acquire) != request {
            let _ = child.kill();
            let _ = child.wait();
            return Err("Preview cancelled");
        }
        if started.elapsed() > timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Err("Preview timed out");
        }
        if fs::metadata(monitor).is_ok_and(|info| info.len() > max_bytes) {
            let _ = child.kill();
            let _ = child.wait();
            return Err("Preview output too large");
        }
        match child.try_wait() {
            Ok(Some(status)) if status.success() => {
                return if fs::metadata(monitor).is_ok_and(|info| info.len() <= max_bytes) {
                    Ok(())
                } else {
                    Err(failed)
                };
            }
            Ok(Some(_)) => return Err(failed),
            Ok(None) => std::thread::sleep(Duration::from_millis(25)),
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(failed);
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn run_stdout(
    mut command: Command,
    output: &Path,
    max_bytes: u64,
    timeout: Duration,
    request: u64,
    serial: &AtomicU64,
    unavailable: &'static str,
    failed: &'static str,
) -> Result<(), &'static str> {
    let file = File::create(output).map_err(|_| "Cannot prepare preview")?;
    command.stdout(Stdio::from(file));
    run_command(
        command,
        output,
        max_bytes,
        timeout,
        request,
        serial,
        unavailable,
        failed,
    )
}

fn read_output(path: &Path, limit: u64) -> Result<Vec<u8>, &'static str> {
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(|_| "Cannot read preview")?
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Cannot read preview")?;
    if bytes.len() as u64 > limit {
        return Err("Preview output too large");
    }
    Ok(bytes)
}

fn dimensions(
    path: &Path,
    scratch: &ScratchDir,
    request: u64,
    serial: &AtomicU64,
) -> Result<(u32, u32), &'static str> {
    let output = scratch.join("dimensions.txt");
    let mut command = Command::new("ffprobe");
    command.args([
        "-v",
        "error",
        "-select_streams",
        "v:0",
        "-show_entries",
        "stream=width,height",
        "-of",
        "csv=p=0:s=x",
    ]);
    command.arg(path);
    run_stdout(
        command,
        &output,
        128,
        RENDER_TIMEOUT,
        request,
        serial,
        "Install FFmpeg to preview images",
        "Cannot inspect image",
    )?;
    let text = fs::read_to_string(output).map_err(|_| "Cannot inspect image")?;
    let (width, height) = text.trim().split_once('x').ok_or("Cannot inspect image")?;
    let width: u32 = width.parse().map_err(|_| "Cannot inspect image")?;
    let height: u32 = height.parse().map_err(|_| "Cannot inspect image")?;
    if width == 0 || height == 0 {
        return Err("Cannot inspect image");
    }
    if width > MAX_SIDE || height > MAX_SIDE || u64::from(width) * u64::from(height) > MAX_PIXELS {
        return Err("Image dimensions too large to preview");
    }
    Ok((width, height))
}

fn scaled_dimensions(width: u32, height: u32) -> (u32, u32) {
    if width <= PREVIEW_SIDE && height <= PREVIEW_SIDE {
        (width, height)
    } else if width >= height {
        (
            PREVIEW_SIDE,
            (u64::from(height) * u64::from(PREVIEW_SIDE) / u64::from(width))
                .max(1)
                .try_into()
                .unwrap(),
        )
    } else {
        (
            (u64::from(width) * u64::from(PREVIEW_SIDE) / u64::from(height))
                .max(1)
                .try_into()
                .unwrap(),
            PREVIEW_SIDE,
        )
    }
}

struct VideoInfo {
    width: u32,
    height: u32,
    duration: f32,
}

fn parse_video_info(text: &str) -> Result<VideoInfo, &'static str> {
    let mut width = 0;
    let mut height = 0;
    let mut duration = 0.0_f32;
    let mut rotation = 0_i32;
    for line in text.lines() {
        if let Some((key, value)) = line.split_once('=') {
            match key {
                "width" => width = value.parse().unwrap_or(0),
                "height" => height = value.parse().unwrap_or(0),
                "duration" => {
                    if let Ok(seconds) = value.parse::<f32>()
                        && seconds.is_finite()
                        && seconds > 0.0
                    {
                        duration = seconds;
                    }
                }
                "rotation" => rotation = value.parse().unwrap_or(0),
                _ => {}
            }
        }
    }
    if width == 0 || height == 0 || duration <= 0.0 {
        return Err("Cannot inspect video stream or duration");
    }
    if width > MAX_SIDE || height > MAX_SIDE || u64::from(width) * u64::from(height) > MAX_PIXELS {
        return Err("Video dimensions too large to preview");
    }
    if rotation.rem_euclid(180) == 90 {
        std::mem::swap(&mut width, &mut height);
    }
    let largest = width.max(height).max(VIDEO_SIDE);
    width = (width * VIDEO_SIDE / largest).max(1);
    height = (height * VIDEO_SIDE / largest).max(1);
    Ok(VideoInfo {
        width,
        height,
        duration,
    })
}

fn random_seed() -> u64 {
    let mut bytes = [0_u8; 8];
    if File::open("/dev/urandom")
        .and_then(|mut file| file.read_exact(&mut bytes))
        .is_ok()
    {
        return u64::from_ne_bytes(bytes);
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    now.as_secs() ^ u64::from(now.subsec_nanos())
}

fn sample_positions(duration: f32, seed: u64) -> [f32; VIDEO_FRAMES] {
    let mut state = seed;
    // Pick one random instant from each fifth, then display them in time order.
    // Stay clear of the reported end: sparse videos may have no frame there.
    let span = duration * 0.90;
    std::array::from_fn(|index| {
        state = state.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut bits = state;
        bits = (bits ^ (bits >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        bits = (bits ^ (bits >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        bits ^= bits >> 31;
        #[allow(clippy::cast_precision_loss)]
        let fraction = ((bits >> 40) as u32) as f32 / 16_777_216.0;
        #[allow(clippy::cast_precision_loss)]
        let segment = index as f32 + fraction;
        segment * span / VIDEO_FRAMES_F32
    })
}

fn decode_video_frame(
    path: &Path,
    info: &VideoInfo,
    position: f32,
    scratch: &ScratchDir,
    request: u64,
    serial: &AtomicU64,
) -> Result<Pixels, &'static str> {
    let output = scratch.join("video-frame.rgba");
    let mut command = Command::new("ffmpeg");
    command
        .args(["-nostdin", "-v", "error", "-threads", "1", "-ss"])
        .arg(format!("{position:.6}"))
        .arg("-i")
        .arg(path)
        .args([
            "-map",
            "0:v:0",
            "-an",
            "-sn",
            "-dn",
            "-frames:v",
            "1",
            "-vf",
        ])
        .arg(format!(
            "scale={}:{}:flags=bilinear,setsar=1",
            info.width, info.height
        ))
        .args([
            "-threads", "1", "-pix_fmt", "rgba", "-f", "rawvideo", "pipe:1",
        ]);
    let frame_bytes = u64::from(info.width) * u64::from(info.height) * 4;
    run_stdout(
        command,
        &output,
        frame_bytes,
        RENDER_TIMEOUT,
        request,
        serial,
        "Install FFmpeg for video previews",
        "Cannot decode video frame",
    )?;
    let bytes = read_output(&output, frame_bytes)?;
    if bytes.len() != usize::try_from(frame_bytes).unwrap() {
        return Err("Cannot decode video frame");
    }
    let mut pixels = Pixels::new(info.width, info.height);
    pixels.make_mut_bytes().copy_from_slice(&bytes);
    Ok(pixels)
}

fn cycle_video_frames(
    request: &Request,
    serial: &AtomicU64,
    mut emit: impl FnMut(Preview) -> bool,
) -> Result<(), &'static str> {
    let path = Path::new(&request.path);
    if !fs::metadata(path).is_ok_and(|metadata| metadata.is_file()) {
        return Err("Video file is unavailable");
    }
    let scratch = ScratchDir::new().map_err(|_| "Cannot prepare video preview")?;
    let output = scratch.join("video-info.txt");
    let mut probe = Command::new("ffprobe");
    probe
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=width,height,duration:stream_side_data=rotation:format=duration",
            "-of",
            "default=noprint_wrappers=1",
            "-i",
        ])
        .arg(path);
    run_stdout(
        probe,
        &output,
        4096,
        RENDER_TIMEOUT,
        request.serial,
        serial,
        "Install FFmpeg (ffmpeg and ffprobe) for video previews",
        "Cannot inspect video",
    )?;
    let info = parse_video_info(&fs::read_to_string(output).map_err(|_| "Cannot inspect video")?)?;
    let positions = sample_positions(info.duration, random_seed());
    let mut frames = Vec::with_capacity(VIDEO_FRAMES);
    let mut first_shown = Instant::now();
    for (index, position) in positions.into_iter().enumerate() {
        if serial.load(Ordering::Acquire) != request.serial {
            return Err("Preview cancelled");
        }
        let pixels = decode_video_frame(path, &info, position, &scratch, request.serial, serial)?;
        if index == 0 {
            first_shown = Instant::now();
            if !emit(Preview::Video {
                pixels: pixels.clone(),
                frame: 0,
            }) {
                return Ok(());
            }
        }
        frames.push(pixels);
    }
    let mut index = 1;
    let mut next = first_shown + VIDEO_FRAME_INTERVAL;
    loop {
        while Instant::now() < next {
            if serial.load(Ordering::Acquire) != request.serial {
                return Err("Preview cancelled");
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        if serial.load(Ordering::Acquire) != request.serial {
            return Err("Preview cancelled");
        }
        if !emit(Preview::Video {
            pixels: frames[index].clone(),
            frame: index,
        }) {
            return Ok(());
        }
        index = (index + 1) % VIDEO_FRAMES;
        next = Instant::now() + VIDEO_FRAME_INTERVAL;
    }
}

fn load_image(path: &Path, request: u64, serial: &AtomicU64) -> Result<Pixels, &'static str> {
    let max_file_bytes = if kind(path.to_str().unwrap_or("")) == Kind::Svg {
        MAX_SVG_BYTES
    } else {
        MAX_IMAGE_BYTES
    };
    if fs::metadata(path).map_err(|_| "File is unavailable")?.len() > max_file_bytes {
        return Err("Image too large to preview");
    }
    let scratch = ScratchDir::new().map_err(|_| "Cannot prepare image preview")?;
    let mut last_error = match load_image_ffmpeg(path, &scratch, request, serial) {
        Ok(pixels) => return Ok(pixels),
        Err("Image dimensions too large to preview") => {
            return Err("Image dimensions too large to preview");
        }
        Err("Install FFmpeg to preview images") => {
            "Install FFmpeg or ImageMagick to preview images"
        }
        Err(error) => error,
    };
    if serial.load(Ordering::Acquire) != request {
        return Err("Preview cancelled");
    }
    for executable in ["magick", "convert", "gm"] {
        match load_image_magick(path, &scratch, executable, request, serial) {
            Ok(pixels) => return Ok(pixels),
            Err(error) if error != "ImageMagick unavailable" => last_error = error,
            Err(_) => {}
        }
        if serial.load(Ordering::Acquire) != request {
            return Err("Preview cancelled");
        }
    }
    Err(last_error)
}

fn load_image_ffmpeg(
    path: &Path,
    scratch: &ScratchDir,
    request: u64,
    serial: &AtomicU64,
) -> Result<Pixels, &'static str> {
    let (width, height) = dimensions(path, scratch, request, serial)?;
    let (width, height) = scaled_dimensions(width, height);
    let output = scratch.join("frame.pam");
    let mut command = Command::new("ffmpeg");
    command.args(["-nostdin", "-v", "error", "-threads", "1", "-i"]);
    command.arg(path);
    command.args(["-frames:v", "1", "-vf"]);
    command.arg(format!("scale={width}:{height}:flags=bilinear"));
    command.args([
        "-pix_fmt",
        "rgba",
        "-f",
        "image2pipe",
        "-vcodec",
        "pam",
        "pipe:1",
    ]);
    run_stdout(
        command,
        &output,
        MAX_RENDER_BYTES,
        RENDER_TIMEOUT,
        request,
        serial,
        "Install FFmpeg to preview images",
        "Cannot decode image",
    )?;
    parse_pam(&read_output(&output, MAX_RENDER_BYTES)?)
}

fn load_image_magick(
    path: &Path,
    scratch: &ScratchDir,
    executable: &str,
    request: u64,
    serial: &AtomicU64,
) -> Result<Pixels, &'static str> {
    let output = scratch.join("frame.ppm");
    let mut command = Command::new(executable);
    if executable == "gm" {
        command.arg("convert");
        command.arg(path);
        command.args([
            "-thumbnail",
            "1200x1200>",
            "-depth",
            "8",
            "-compress",
            "None",
            "ppm:-",
        ]);
    } else {
        // ImageMagick bounds cache use before producing the thumbnail.
        command.args([
            "-limit", "memory", "128MiB", "-limit", "map", "128MiB", "-limit", "disk", "64MiB",
        ]);
        command.arg(path);
        command.args([
            "-auto-orient",
            "-thumbnail",
            "1200x1200>",
            "-background",
            "white",
            "-alpha",
            "background",
            "-depth",
            "8",
            "-compress",
            "none",
            "ppm:-",
        ]);
    }
    run_stdout(
        command,
        &output,
        MAX_RENDER_BYTES,
        RENDER_TIMEOUT,
        request,
        serial,
        "ImageMagick unavailable",
        "Cannot decode image",
    )?;
    parse_ppm(&read_output(&output, MAX_RENDER_BYTES)?)
}

fn load_pdf_preview(
    path: &Path,
    page: u32,
    request: u64,
    serial: &AtomicU64,
    cache: &mut PdfCache,
) -> Result<Preview, &'static str> {
    if cache.path != path {
        cache.path = path.to_path_buf();
        cache.pages = pdf_page_count(path, request, serial).unwrap_or(1);
        cache.images.clear();
    }
    if page == 0 || page > cache.pages {
        return Err("PDF page unavailable");
    }
    for number in [page, page.saturating_sub(1), page.saturating_add(1)] {
        if number == 0 || number > cache.pages || cache.images.contains_key(&number) {
            continue;
        }
        match load_pdf(path, number, request, serial) {
            Ok(full) => {
                let fit = smooth_pdf_fit(&full);
                cache.images.insert(number, PdfPage { full, fit });
            }
            Err(error) if number == page => return Err(error),
            Err(_) => {}
        }
    }
    cache.images.retain(|number, _| number.abs_diff(page) <= 1);
    Ok(Preview::Pdf {
        page,
        pages: cache.pages,
        previous: cache.images.get(&page.saturating_sub(1)).cloned(),
        current: cache
            .images
            .get(&page)
            .cloned()
            .ok_or("Cannot render PDF")?,
        next: cache.images.get(&page.saturating_add(1)).cloned(),
    })
}

// Downscale once with a low-pass filter before Slint fits the page into
// the narrow pane. A single large reduction in the software renderer leaves
// small glyphs harsh and uneven.
fn smooth_pdf_fit(full: &Pixels) -> Pixels {
    // The widest supported preview pane displays a page at about 560 px.
    // Prefilter to that width so Slint does not shrink tiny glyphs again there.
    const FIT_WIDTH: u32 = 560;
    if full.width() <= FIT_WIDTH {
        return full.clone();
    }
    let height = u32::try_from(
        (u64::from(full.height()) * u64::from(FIT_WIDTH) + u64::from(full.width() / 2))
            / u64::from(full.width()),
    )
    .unwrap()
    .max(1);
    let source = image::RgbaImage::from_raw(full.width(), full.height(), full.as_bytes().to_vec())
        .expect("PDF pixel buffer has its declared dimensions");
    let resized = image::imageops::resize(
        &source,
        FIT_WIDTH,
        height,
        image::imageops::FilterType::Gaussian,
    );
    SharedPixelBuffer::clone_from_slice(resized.as_raw(), FIT_WIDTH, height)
}

fn parse_pdf_page_count(output: &str) -> Option<u32> {
    output.lines().find_map(|line| {
        let (label, value) = line.split_once(':')?;
        (label.trim() == "Pages")
            .then(|| value.trim().parse::<u32>().ok())
            .flatten()
            .filter(|count| *count > 0 && i32::try_from(*count).is_ok())
    })
}

fn pdf_page_count(path: &Path, request: u64, serial: &AtomicU64) -> Option<u32> {
    let scratch = ScratchDir::new().ok()?;
    let output = scratch.join("info.txt");
    for mut command in [Command::new("pdfinfo"), Command::new("mutool")] {
        if command.get_program() == "mutool" {
            command.arg("info");
        }
        command.arg(path);
        if run_stdout(
            command,
            &output,
            64 * 1024,
            RENDER_TIMEOUT,
            request,
            serial,
            "PDF metadata tool unavailable",
            "Cannot read PDF page count",
        )
        .is_ok()
        {
            let bytes = read_output(&output, 64 * 1024).ok()?;
            if let Some(count) = parse_pdf_page_count(&String::from_utf8_lossy(&bytes)) {
                return Some(count);
            }
        }
        if serial.load(Ordering::Acquire) != request {
            return None;
        }
    }
    None
}

fn load_pdf(
    path: &Path,
    page: u32,
    request: u64,
    serial: &AtomicU64,
) -> Result<Pixels, &'static str> {
    if fs::metadata(path).map_err(|_| "File is unavailable")?.len() > MAX_PDF_BYTES {
        return Err("PDF too large to preview");
    }
    let scratch = ScratchDir::new().map_err(|_| "Cannot prepare PDF preview")?;
    let prefix = scratch.join("page");
    let output = scratch.join("page.ppm");
    let mut poppler = Command::new("pdftoppm");
    let page = page.to_string();
    poppler.args(["-f", &page, "-l", &page, "-singlefile", "-scale-to"]);
    poppler.arg(PDF_RENDER_SIDE.to_string());
    poppler.arg(path).arg(&prefix).stdout(Stdio::null());
    let poppler_result = run_command(
        poppler,
        &output,
        MAX_RENDER_BYTES,
        RENDER_TIMEOUT,
        request,
        serial,
        "Poppler unavailable",
        "Cannot render PDF",
    );
    if poppler_result.is_ok()
        && let Ok(pixels) =
            read_output(&output, MAX_RENDER_BYTES).and_then(|bytes| parse_ppm(&bytes))
    {
        return Ok(pixels);
    }
    if serial.load(Ordering::Acquire) != request {
        return Err("Preview cancelled");
    }
    let mut mupdf = Command::new("mutool");
    mupdf.args(["draw", "-q", "-F", "ppm", "-o"]);
    mupdf
        .arg(&output)
        .arg("-w")
        .arg(PDF_RENDER_SIDE.to_string())
        .arg("-h")
        .arg(PDF_RENDER_SIDE.to_string())
        .args(["-m", "134217728"]);
    mupdf.arg(path).arg(&page).stdout(Stdio::null());
    if let Err(error) = run_command(
        mupdf,
        &output,
        MAX_RENDER_BYTES,
        RENDER_TIMEOUT,
        request,
        serial,
        "Install Poppler or MuPDF to preview PDFs",
        "Cannot render PDF",
    ) {
        return if error == "Install Poppler or MuPDF to preview PDFs"
            && poppler_result != Err("Poppler unavailable")
        {
            Err(poppler_result.err().unwrap_or("Cannot render PDF"))
        } else {
            Err(error)
        };
    }
    parse_ppm(&read_output(&output, MAX_RENDER_BYTES)?)
}

fn parse_pam(bytes: &[u8]) -> Result<Pixels, &'static str> {
    if !bytes.starts_with(b"P7\n") {
        return Err("Invalid image output");
    }
    let end = bytes
        .windows(7)
        .position(|window| window == b"ENDHDR\n")
        .map(|index| index + 7)
        .ok_or("Invalid image output")?;
    if end > 256 {
        return Err("Invalid image output");
    }
    let header = std::str::from_utf8(&bytes[..end]).map_err(|_| "Invalid image output")?;
    let mut width = None;
    let mut height = None;
    let mut depth = None;
    let mut maxval = None;
    let mut tuple = None;
    for line in header.lines() {
        if let Some((key, value)) = line.split_once(' ') {
            match key {
                "WIDTH" => width = value.parse::<u32>().ok(),
                "HEIGHT" => height = value.parse::<u32>().ok(),
                "DEPTH" => depth = value.parse::<u32>().ok(),
                "MAXVAL" => maxval = value.parse::<u32>().ok(),
                "TUPLTYPE" => tuple = Some(value),
                _ => {}
            }
        }
    }
    if depth != Some(4) || maxval != Some(255) || tuple != Some("RGB_ALPHA") {
        return Err("Invalid image output");
    }
    pixels_from_rgba(
        width.ok_or("Invalid image output")?,
        height.ok_or("Invalid image output")?,
        &bytes[end..],
    )
}

fn ppm_token<'a>(bytes: &'a [u8], position: &mut usize) -> Option<&'a [u8]> {
    while *position < bytes.len() {
        match bytes[*position] {
            b'#' => {
                while *position < bytes.len() && bytes[*position] != b'\n' {
                    *position += 1;
                }
            }
            c if c.is_ascii_whitespace() => *position += 1,
            _ => break,
        }
    }
    let start = *position;
    while *position < bytes.len() && !bytes[*position].is_ascii_whitespace() {
        *position += 1;
    }
    (start < *position).then_some(&bytes[start..*position])
}

fn parse_ppm(bytes: &[u8]) -> Result<Pixels, &'static str> {
    let mut position = 0;
    if ppm_token(bytes, &mut position) != Some(&b"P6"[..]) {
        return Err("Invalid PDF image output");
    }
    let width =
        std::str::from_utf8(ppm_token(bytes, &mut position).ok_or("Invalid PDF image output")?)
            .map_err(|_| "Invalid PDF image output")?
            .parse::<u32>()
            .map_err(|_| "Invalid PDF image output")?;
    let height =
        std::str::from_utf8(ppm_token(bytes, &mut position).ok_or("Invalid PDF image output")?)
            .map_err(|_| "Invalid PDF image output")?
            .parse::<u32>()
            .map_err(|_| "Invalid PDF image output")?;
    if ppm_token(bytes, &mut position) != Some(&b"255"[..])
        || !bytes.get(position).is_some_and(u8::is_ascii_whitespace)
    {
        return Err("Invalid PDF image output");
    }
    if bytes[position] == b'\r' && bytes.get(position + 1) == Some(&b'\n') {
        position += 2;
    } else {
        position += 1;
    }
    pixels_from_rgb(width, height, &bytes[position..])
}

fn checked_pixel_count(
    width: u32,
    height: u32,
    channels: usize,
    length: usize,
) -> Result<(), &'static str> {
    if width == 0 || height == 0 || width > PDF_RENDER_SIDE || height > PDF_RENDER_SIDE {
        return Err("Invalid preview dimensions");
    }
    let expected = usize::try_from(width).unwrap() * usize::try_from(height).unwrap() * channels;
    if length != expected {
        return Err("Invalid preview pixels");
    }
    Ok(())
}

fn pixels_from_rgba(width: u32, height: u32, rgba: &[u8]) -> Result<Pixels, &'static str> {
    checked_pixel_count(width, height, 4, rgba.len())?;
    Ok(SharedPixelBuffer::clone_from_slice(rgba, width, height))
}

fn pixels_from_rgb(width: u32, height: u32, rgb: &[u8]) -> Result<Pixels, &'static str> {
    checked_pixel_count(width, height, 3, rgb.len())?;
    let mut rgba = Vec::with_capacity(rgb.len() / 3 * 4);
    for pixel in rgb.chunks_exact(3) {
        rgba.extend_from_slice(&[pixel[0], pixel[1], pixel[2], 255]);
    }
    pixels_from_rgba(width, height, &rgba)
}
const PACKAGE_TEXT_SCRIPT: &str = r"
import sys, zipfile, xml.etree.ElementTree as ET
path, kind = sys.argv[1:]
member = 'word/document.xml' if kind == 'docx' else 'content.xml'
with zipfile.ZipFile(path) as archive:
    info = archive.getinfo(member)
    if info.file_size > 8 * 1024 * 1024:
        raise ValueError('document XML too large')
    data = archive.read(member)
root = ET.fromstring(data)
word = '{http://schemas.openxmlformats.org/wordprocessingml/2006/main}'
odt = '{urn:oasis:names:tc:opendocument:xmlns:text:1.0}'
limit = 64 * 1024
output = bytearray()
truncated = False
def add(value):
    global truncated
    encoded = value.encode('utf-8')
    room = limit - len(output)
    if len(encoded) > room:
        output.extend(encoded[:room].decode('utf-8', 'ignore').encode('utf-8'))
        truncated = True
    else:
        output.extend(encoded)
for paragraph in root.iter():
    if kind == 'docx':
        if paragraph.tag != word + 'p':
            continue
        for node in paragraph.iter():
            if node.tag == word + 't': add(node.text or '')
            elif node.tag == word + 'tab': add('\t')
            elif node.tag in (word + 'br', word + 'cr'): add('\n')
            if truncated: break
    else:
        if paragraph.tag not in (odt + 'p', odt + 'h'):
            continue
        for node in paragraph.iter():
            if node.text: add(node.text)
            if node.tag == odt + 's':
                count = min(int(node.get(odt + 'c', '1')), 100)
                add(' ' * count)
            elif node.tag == odt + 'tab': add('\t')
            if node is not paragraph and node.tail: add(node.tail)
            if truncated: break
    add('\n')
    if truncated: break
if truncated: output.extend(b'\n... Preview limited to the first 64 KiB')
sys.stdout.buffer.write(bytes(output).strip())
";

fn load_document(path: &Path, request: u64, serial: &AtomicU64) -> Result<Preview, &'static str> {
    if fs::metadata(path).map_err(|_| "File is unavailable")?.len() > MAX_DOCUMENT_BYTES {
        return Err("Document too large to preview");
    }
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if matches!(extension.as_str(), "doc" | "docx" | "odt")
        && let Ok(pixels) = load_embedded_document_thumbnail(path, request, serial)
    {
        return Ok(Preview::Image(pixels));
    }
    if let Some(pixels) = render_document_page(path, request, serial) {
        return Ok(Preview::Image(pixels));
    }
    if serial.load(Ordering::Acquire) != request {
        return Err("Document preview cancelled");
    }
    let text = match extension.as_str() {
        "docx" | "odt" => load_package_text(path, &extension, request, serial)?,
        "doc" | "rtf" => load_legacy_document_text(path, request, serial)?,
        _ => return Err("Document type is unsupported"),
    };
    if text.trim().is_empty() {
        return Err("No readable document text");
    }
    Ok(Preview::Text(text))
}

fn load_embedded_document_thumbnail(
    path: &Path,
    request: u64,
    serial: &AtomicU64,
) -> Result<Pixels, &'static str> {
    let scratch = ScratchDir::new().map_err(|_| "Cannot prepare document preview")?;
    let thumbnail = scratch.join("thumbnail.png");
    let mut command = Command::new("gsf-office-thumbnailer");
    command
        .arg("-i")
        .arg(path)
        .arg("-o")
        .arg(&thumbnail)
        .arg("-s")
        .arg(PREVIEW_SIDE.to_string())
        .stdout(Stdio::null());
    run_command(
        command,
        &thumbnail,
        MAX_RENDER_BYTES,
        RENDER_TIMEOUT,
        request,
        serial,
        "Office thumbnailer unavailable",
        "No embedded document thumbnail",
    )?;
    let pixels = load_image(&thumbnail, request, serial)?;
    // Some document templates contain an empty placeholder thumbnail.
    // Prefer readable text to displaying a blank page for those files.
    let visible_pixels = pixels
        .as_bytes()
        .chunks_exact(4)
        .filter(|pixel| pixel[0] <= 245 || pixel[1] <= 245 || pixel[2] <= 245)
        .take(64)
        .count();
    if visible_pixels < 64 {
        return Err("Blank embedded document thumbnail");
    }
    Ok(pixels)
}

fn render_document_page(path: &Path, request: u64, serial: &AtomicU64) -> Option<Pixels> {
    let scratch = ScratchDir::new().ok()?;
    let output_dir = scratch.join("output");
    fs::create_dir(&output_dir).ok()?;
    let profile = format!("file://{}", scratch.join("profile").display());
    let mut pdf = output_dir.join(path.file_stem()?);
    pdf.set_extension("pdf");
    for executable in ["soffice", "libreoffice", "lowriter"] {
        let mut command = Command::new(executable);
        command.arg(format!("-env:UserInstallation={profile}"));
        command.args(["--headless", "--convert-to", "pdf", "--outdir"]);
        command.arg(&output_dir).arg(path).stdout(Stdio::null());
        if run_command(
            command,
            &pdf,
            MAX_PDF_BYTES,
            DOCUMENT_TIMEOUT,
            request,
            serial,
            "LibreOffice unavailable",
            "Cannot convert document",
        )
        .is_ok()
        {
            return load_pdf(&pdf, 1, request, serial).ok();
        }
        if serial.load(Ordering::Acquire) != request {
            return None;
        }
    }
    None
}

fn load_package_text(
    path: &Path,
    extension: &str,
    request: u64,
    serial: &AtomicU64,
) -> Result<String, &'static str> {
    let scratch = ScratchDir::new().map_err(|_| "Cannot prepare document preview")?;
    let output = scratch.join("text.txt");
    let mut python = Command::new("python3");
    python.args(["-I", "-c", PACKAGE_TEXT_SCRIPT]);
    python.arg(path).arg(extension);
    let limit = u64::try_from(MAX_TEXT_BYTES + 128).unwrap();
    match run_stdout(
        python,
        &output,
        limit,
        DOCUMENT_TIMEOUT,
        request,
        serial,
        "Python 3 unavailable",
        "Cannot read document package",
    ) {
        Ok(()) => {
            let bytes = read_output(&output, limit)?;
            return String::from_utf8(bytes).map_err(|_| "Document text is not UTF-8");
        }
        Err("Preview cancelled") => return Err("Preview cancelled"),
        Err(_) => {}
    }
    // On smaller installations, unzip and libxml2 can still provide the text.
    let member = if extension == "docx" {
        "word/document.xml"
    } else {
        "content.xml"
    };
    let xml = scratch.join("document.xml");
    let mut unzip = Command::new("unzip");
    unzip.arg("-p").arg(path).arg(member);
    run_stdout(
        unzip,
        &xml,
        8 * 1024 * 1024,
        DOCUMENT_TIMEOUT,
        request,
        serial,
        "Install Python 3 or unzip and xmllint to preview DOCX/ODT files",
        "Cannot read document package",
    )?;
    let mut xmllint = Command::new("xmllint");
    xmllint.args(["--nonet", "--xpath", "string(//*[local-name()='body'])"]);
    xmllint.arg(&xml);
    run_stdout(
        xmllint,
        &output,
        8 * 1024 * 1024,
        DOCUMENT_TIMEOUT,
        request,
        serial,
        "Install Python 3 or unzip and xmllint to preview DOCX/ODT files",
        "Cannot read document XML",
    )?;
    let mut bytes = Vec::new();
    File::open(output)
        .map_err(|_| "Cannot read document text")?
        .take(u64::try_from(MAX_TEXT_BYTES + 1).unwrap())
        .read_to_end(&mut bytes)
        .map_err(|_| "Cannot read document text")?;
    let truncated = bytes.len() > MAX_TEXT_BYTES;
    bytes.truncate(MAX_TEXT_BYTES);
    let valid = match std::str::from_utf8(&bytes) {
        Ok(_) => bytes.len(),
        Err(error) if truncated && error.error_len().is_none() => error.valid_up_to(),
        Err(_) => return Err("Document text is not UTF-8"),
    };
    bytes.truncate(valid);
    let mut text = String::from_utf8(bytes).map_err(|_| "Document text is not UTF-8")?;
    if truncated {
        text.push_str("\n... Preview limited to the first 64 KiB");
    }
    Ok(text)
}

fn load_legacy_document_text(
    path: &Path,
    request: u64,
    serial: &AtomicU64,
) -> Result<String, &'static str> {
    let scratch = ScratchDir::new().map_err(|_| "Cannot prepare document preview")?;
    let output = scratch.join("text.txt");
    let file = File::create(&output).map_err(|_| "Cannot prepare document preview")?;
    let mut command = Command::new("catdoc");
    command
        .args(["-d", "utf-8"])
        .arg(path)
        .stdout(Stdio::from(file))
        .stderr(Stdio::null());
    let mut child = command
        .spawn()
        .map_err(|_| "Install catdoc or LibreOffice to preview DOC/RTF files")?;
    let started = Instant::now();
    loop {
        if serial.load(Ordering::Acquire) != request || started.elapsed() > DOCUMENT_TIMEOUT {
            let _ = child.kill();
            let _ = child.wait();
            return Err("Document preview cancelled or timed out");
        }
        if fs::metadata(&output)
            .is_ok_and(|info| info.len() > u64::try_from(MAX_TEXT_BYTES + 1).unwrap())
        {
            let _ = child.kill();
            let _ = child.wait();
            break;
        }
        match child.try_wait() {
            Ok(Some(status)) if status.success() => break,
            Ok(Some(_)) => return Err("Cannot read document"),
            Ok(None) => std::thread::sleep(Duration::from_millis(25)),
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("Cannot read document");
            }
        }
    }
    let mut bytes = Vec::new();
    File::open(output)
        .map_err(|_| "Cannot read document")?
        .take(u64::try_from(MAX_TEXT_BYTES + 1).unwrap())
        .read_to_end(&mut bytes)
        .map_err(|_| "Cannot read document")?;
    let truncated = bytes.len() > MAX_TEXT_BYTES;
    bytes.truncate(MAX_TEXT_BYTES);
    let mut text = String::from_utf8_lossy(&bytes).into_owned();
    if truncated {
        text.push_str("\n... Preview limited to the first 64 KiB");
    }
    Ok(text)
}

fn load_text(path: &Path) -> Result<String, &'static str> {
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(|_| "Cannot open file")?
        .take(u64::try_from(MAX_TEXT_BYTES + 1).unwrap())
        .read_to_end(&mut bytes)
        .map_err(|_| "Cannot read file")?;
    let truncated = bytes.len() > MAX_TEXT_BYTES;
    bytes.truncate(MAX_TEXT_BYTES);
    if bytes.contains(&0) {
        return Err("Binary file cannot be shown as text");
    }
    let valid = match std::str::from_utf8(&bytes) {
        Ok(_) => bytes.len(),
        Err(error) if truncated && error.error_len().is_none() => error.valid_up_to(),
        Err(_) => return Err("Text is not UTF-8"),
    };
    bytes.truncate(valid);
    let mut text = String::from_utf8(bytes).map_err(|_| "Text is not UTF-8")?;
    if truncated {
        text.push_str("\n\n... Preview limited to the first 64 KiB");
    }
    Ok(text)
}

/// Splits text into lines, cutting long lines, so the virtualized list only
/// renders a few short Text items at a time.
fn split_text(text: &str) -> Vec<String> {
    let mut chunks = Vec::new();
    for line in text.lines() {
        let mut rest = line;
        loop {
            let mut end = rest
                .char_indices()
                .nth(TEXT_CHUNK_CHARS)
                .map_or(rest.len(), |(index, _)| index);
            // Prefer a word boundary so wrapping does not split a word across items.
            if end < rest.len()
                && let Some(space) = rest[..end].rfind(' ')
            {
                end = space + 1;
            }
            chunks.push(rest[..end].to_owned());
            rest = &rest[end..];
            if rest.is_empty() {
                break;
            }
        }
    }
    chunks
}

fn text_chunks(text: &str) -> slint::ModelRc<slint::SharedString> {
    slint::ModelRc::new(slint::VecModel::from(
        split_text(text)
            .into_iter()
            .map(slint::SharedString::from)
            .collect::<Vec<_>>(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn common_formats_are_classified() {
        for name in [
            "photo.BMP",
            "scan.tiff",
            "icon.ico",
            "drawing.svg",
            "image.heic",
        ] {
            assert!(matches!(kind(name), Kind::Raster | Kind::Svg), "{name}");
        }
        for name in [
            "notes.md",
            "data.csv",
            "src/main.rs",
            ".gitignore",
            ".env",
            "Dockerfile",
        ] {
            assert_eq!(kind(name), Kind::Text, "{name}");
        }
        for name in ["report.DOC", "report.docx", "letter.odt", "memo.rtf"] {
            assert_eq!(kind(name), Kind::Document, "{name}");
        }
        assert_eq!(kind("report.PDF"), Kind::Pdf);
        for name in [
            "clip.MP4",
            "clip.mkv",
            "clip.webm",
            "clip.mov",
            "clip.avi",
            "clip.m2ts",
        ] {
            assert_eq!(kind(name), Kind::Video, "{name}");
        }
        assert_eq!(kind("archive.zip"), Kind::Unsupported);
    }

    #[test]
    fn video_probe_bounds_dimensions_and_handles_rotation_and_bad_duration() {
        let info = parse_video_info(
            "width=1920\nheight=1080\nduration=N/A\nrotation=-90\nduration=61.5\n",
        )
        .unwrap();
        assert_eq!((info.width, info.height), (360, 640));
        let positions = sample_positions(info.duration, 1234);
        assert!(
            positions
                .iter()
                .all(|position| *position >= 0.0 && *position < info.duration)
        );
        for (index, position) in positions.iter().enumerate() {
            #[allow(clippy::cast_precision_loss)]
            let start = index as f32 * info.duration * 0.98 / VIDEO_FRAMES_F32;
            #[allow(clippy::cast_precision_loss)]
            let end = (index + 1) as f32 * info.duration * 0.98 / VIDEO_FRAMES_F32;
            assert!(*position >= start && *position < end);
        }
        assert!(
            positions
                .iter()
                .zip(sample_positions(info.duration, 5678))
                .any(|(first, second)| (first - second).abs() > 0.001)
        );
        for duration in ["N/A", "NaN", "inf", "-1", "0"] {
            assert!(
                parse_video_info(&format!("width=1920\nheight=1080\nduration={duration}\n"))
                    .is_err()
            );
        }
        assert!(parse_video_info("width=10000\nheight=10000\nduration=2\n").is_err());
        assert!(parse_video_info("duration=2\n").is_err());
    }

    #[test]
    fn ffmpeg_video_samples_five_frames_loops_and_cancels() {
        if Command::new("ffmpeg").arg("-version").output().is_err()
            || Command::new("ffprobe").arg("-version").output().is_err()
        {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("two colors.mkv");
        assert!(
            Command::new("ffmpeg")
                .args([
                    "-nostdin",
                    "-v",
                    "error",
                    "-f",
                    "lavfi",
                    "-i",
                    "color=red:s=32x24:r=12:d=1",
                    "-f",
                    "lavfi",
                    "-i",
                    "color=blue:s=32x24:r=12:d=1",
                    "-filter_complex",
                    "[0:v][1:v]concat=n=2:v=1:a=0",
                    "-c:v",
                    "ffv1",
                ])
                .arg(&path)
                .status()
                .unwrap()
                .success()
        );
        let serial = AtomicU64::new(1);
        let mut request = Request {
            path: path.to_string_lossy().into_owned(),
            page: 1,
            serial: 1,
        };
        let mut seen = Vec::new();
        cycle_video_frames(&request, &serial, |preview| {
            let Preview::Video { pixels, frame } = preview else {
                panic!("video expected")
            };
            assert_eq!((pixels.width(), pixels.height()), (32, 24));
            seen.push((frame, pixels.as_bytes()[0] > pixels.as_bytes()[2]));
            seen.len() < 6
        })
        .unwrap();
        assert_eq!(
            seen.iter().map(|(frame, _)| *frame).collect::<Vec<_>>(),
            [0, 1, 2, 3, 4, 0]
        );
        assert_eq!(seen[0].1, seen[5].1);
        assert!(seen.iter().any(|(_, red)| *red));
        assert!(seen.iter().any(|(_, red)| !*red));

        let started = Instant::now();
        assert_eq!(
            cycle_video_frames(&request, &serial, |_| {
                serial.store(2, Ordering::Release);
                true
            })
            .unwrap_err(),
            "Preview cancelled"
        );
        assert!(started.elapsed() < Duration::from_secs(2));
        request.serial = 2;
        request.path = dir.path().join("broken.mp4").to_string_lossy().into_owned();
        fs::write(&request.path, b"invalid video").unwrap();
        assert!(
            cycle_video_frames(&request, &serial, |_| panic!(
                "broken video cannot produce frames"
            ))
            .is_err()
        );
    }

    #[test]
    fn image_output_parsers_enforce_shape_and_preserve_color() {
        let pam = b"P7\nWIDTH 2\nHEIGHT 1\nDEPTH 4\nMAXVAL 255\nTUPLTYPE RGB_ALPHA\nENDHDR\n\x14\x28\x3c\xff\x01\x02\x03\x80";
        let pixels = parse_pam(pam).unwrap();
        assert_eq!((pixels.width(), pixels.height()), (2, 1));
        assert_eq!(pixels.as_bytes(), &[20, 40, 60, 255, 1, 2, 3, 128]);
        let ppm = b"P6\n# comment\n2 1\n255\n\x14\x28\x3c\x01\x02\x03";
        let pixels = parse_ppm(ppm).unwrap();
        assert_eq!(pixels.as_bytes(), &[20, 40, 60, 255, 1, 2, 3, 255]);
        assert!(parse_pam(&pam[..pam.len() - 1]).is_err());
        assert!(parse_ppm(&ppm[..ppm.len() - 1]).is_err());
    }

    #[test]
    fn pdf_fit_prefilters_fine_edges() {
        let mut stripes = Vec::with_capacity(1200 * 2 * 4);
        for _ in 0..2 {
            for x in 0..1200 {
                let value = if x % 2 == 0 { 0 } else { 255 };
                stripes.extend_from_slice(&[value, value, value, 255]);
            }
        }
        let full = pixels_from_rgba(1200, 2, &stripes).unwrap();
        let fit = smooth_pdf_fit(&full);
        assert_eq!((fit.width(), fit.height()), (560, 1));
        assert!((40..215).contains(&fit.as_bytes()[280 * 4]));
    }

    #[test]
    fn pdf_page_navigation_renders_distinct_pages() {
        use std::io::Write;
        assert_eq!(parse_pdf_page_count("Title: Demo\nPages: 2\n"), Some(2));
        assert_eq!(parse_pdf_page_count("Pages: 0\n"), None);
        if Command::new("pdfinfo").arg("-v").output().is_err()
            || Command::new("pdftoppm").arg("-v").output().is_err()
        {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("two-pages.pdf");
        let red = "1 0 0 rg 0 0 100 100 re f\n";
        let blue = "0 0 1 rg 0 0 100 100 re f\n";
        let objects = [
            "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
            "<< /Type /Pages /Kids [3 0 R 5 0 R] /Count 2 >>".to_string(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Contents 4 0 R >>".to_string(),
            format!("<< /Length {} >>\nstream\n{red}endstream", red.len()),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Contents 6 0 R >>".to_string(),
            format!("<< /Length {} >>\nstream\n{blue}endstream", blue.len()),
        ];
        let mut pdf = b"%PDF-1.4\n".to_vec();
        let mut offsets = vec![0];
        for (index, object) in objects.iter().enumerate() {
            offsets.push(pdf.len());
            write!(&mut pdf, "{} 0 obj\n{object}\nendobj\n", index + 1).unwrap();
        }
        let xref = pdf.len();
        write!(&mut pdf, "xref\n0 7\n0000000000 65535 f \n").unwrap();
        for offset in offsets.iter().skip(1) {
            writeln!(&mut pdf, "{offset:010} 00000 n ").unwrap();
        }
        write!(
            &mut pdf,
            "trailer\n<< /Size 7 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n"
        )
        .unwrap();
        fs::write(&path, pdf).unwrap();
        let serial = AtomicU64::new(1);
        assert_eq!(pdf_page_count(&path, 1, &serial), Some(2));
        let first = load_pdf(&path, 1, 1, &serial).unwrap();
        let second = load_pdf(&path, 2, 1, &serial).unwrap();
        let center = |pixels: &Pixels| {
            let offset = ((pixels.height() / 2 * pixels.width() + pixels.width() / 2) * 4) as usize;
            pixels.as_bytes()[offset..offset + 3].to_vec()
        };
        assert_eq!(center(&first), [255, 0, 0]);
        assert_eq!(center(&second), [0, 0, 255]);
    }

    #[test]
    fn docx_text_uses_installed_python_without_a_rust_parser() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sample.docx");
        let script = r#"import sys, zipfile
xml = '<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>A &amp; B</w:t></w:r></w:p><w:p><w:r><w:t>Next</w:t></w:r></w:p></w:body></w:document>'
with zipfile.ZipFile(sys.argv[1], 'w') as archive: archive.writestr('word/document.xml', xml)
"#;
        let status = Command::new("python3")
            .args(["-I", "-c", script])
            .arg(&path)
            .status();
        let Ok(status) = status else { return };
        assert!(status.success());
        let serial = AtomicU64::new(1);
        assert_eq!(
            load_package_text(&path, "docx", 1, &serial).unwrap(),
            "A & B\nNext"
        );
    }

    #[test]
    fn text_preview_is_bounded_and_rejects_binary() {
        let dir = tempfile::tempdir().unwrap();
        let text = dir.path().join("large.txt");
        fs::write(&text, "é".repeat(40_000)).unwrap();
        let preview = load_text(&text).unwrap();
        assert!(preview.contains("Preview limited to the first 64 KiB"));
        assert!(preview.len() < MAX_TEXT_BYTES + 64);
        let binary = dir.path().join("bad.txt");
        fs::write(&binary, b"ab\0cd").unwrap();
        assert_eq!(
            load_text(&binary),
            Err("Binary file cannot be shown as text")
        );
    }

    fn headless_window(width: u32, height: u32) {
        use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType};
        use slint::platform::{Platform, PlatformError, WindowAdapter};
        use std::rc::Rc;
        thread_local! { static WINDOW: Rc<MinimalSoftwareWindow> = MinimalSoftwareWindow::new(RepaintBufferType::ReusedBuffer); }
        struct TestPlatform;
        impl Platform for TestPlatform {
            fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, PlatformError> {
                Ok(WINDOW.with(Rc::clone))
            }
        }
        // Tests share one process-wide platform; each test thread gets its own window.
        let _ = slint::platform::set_platform(Box::new(TestPlatform));
        WINDOW.with(|window| window.set_size(slint::PhysicalSize::new(width, height)));
    }

    fn preview_row(name: &str, folder: &str) -> slint::ModelRc<slint::StandardListViewItem> {
        let item = |text: &str| {
            let mut item = slint::StandardListViewItem::default();
            item.text = text.into();
            item
        };
        slint::ModelRc::new(slint::VecModel::from(vec![
            item(name),
            item(folder),
            item(""),
            item(""),
        ]))
    }

    #[test]
    fn long_text_preview_renders_with_the_software_renderer() {
        headless_window(900, 580);
        let ui = crate::AppWindow::new().unwrap();
        ui.set_rows(slint::ModelRc::new(slint::VecModel::from(vec![
            preview_row("notes.txt", "/tmp"),
        ])));
        ui.set_selected(0);
        ui.show().unwrap();
        let lines = (1..=5_000).fold(String::new(), |mut text, line| {
            text.push_str(&line.to_string());
            text.push('\n');
            text
        });
        ui.set_preview_text(text_chunks(&lines));
        ui.set_preview_text_ready(true);
        // Before chunking, one Text item exceeded the renderer's i16 coordinates.
        let _ = ui.window().take_snapshot().unwrap();
        for _ in 0..20 {
            ui.window()
                .dispatch_event(slint::platform::WindowEvent::PointerScrolled {
                    position: slint::LogicalPosition { x: 750.0, y: 300.0 },
                    delta_x: 0.0,
                    delta_y: -20_000.0,
                });
            let _ = ui.window().take_snapshot().unwrap();
        }
    }

    #[test]
    fn deep_pdf_pages_render_with_the_software_renderer() {
        headless_window(900, 580);
        let ui = crate::AppWindow::new().unwrap();
        ui.set_rows(slint::ModelRc::new(slint::VecModel::from(vec![
            preview_row("report.pdf", "/tmp"),
        ])));
        ui.set_selected(0);
        ui.set_preview_pdf_active(true);
        ui.set_preview_pdf_pages(1_000);
        ui.show().unwrap();
        let _ = ui.window().take_snapshot().unwrap();
        for _ in 0..40 {
            ui.window()
                .dispatch_event(slint::platform::WindowEvent::PointerScrolled {
                    position: slint::LogicalPosition { x: 750.0, y: 300.0 },
                    delta_x: 0.0,
                    delta_y: -20_000.0,
                });
            ui.set_preview_pdf_page(ui.get_preview_pdf_page() + 40);
            let _ = ui.window().take_snapshot().unwrap();
        }
    }

    #[test]
    fn text_chunks_split_lines_and_bound_long_lines() {
        let chunks = split_text("a\nb\n\n");
        assert_eq!(chunks, ["a", "b", ""]);
        let long = "é".repeat(TEXT_CHUNK_CHARS * 2 + 5);
        let chunks = split_text(&long);
        assert_eq!(chunks.len(), 3);
        assert!(
            chunks
                .iter()
                .all(|chunk| chunk.chars().count() <= TEXT_CHUNK_CHARS)
        );
        assert_eq!(chunks.concat(), long);
        let words = "word ".repeat(TEXT_CHUNK_CHARS);
        let chunks = split_text(&words);
        assert!(chunks.iter().all(|chunk| chunk.ends_with("word ")));
        assert_eq!(chunks.concat(), words);
    }

    #[test]
    fn divider_drag_resizes_and_window_shrink_clamps_the_pane() {
        use slint::LogicalPosition;
        use slint::platform::{PointerEventButton, WindowEvent};
        headless_window(900, 580);
        let ui = crate::AppWindow::new().unwrap();
        ui.show().unwrap();
        let _ = ui.window().take_snapshot().unwrap();
        let position = |x| LogicalPosition { x, y: 260.0 };
        ui.window().dispatch_event(WindowEvent::PointerPressed {
            position: position(596.0),
            button: PointerEventButton::Left,
        });
        for x in [580.0, 560.0, 540.0, 520.0, 500.0, 480.0] {
            ui.window().dispatch_event(WindowEvent::PointerMoved {
                position: position(x),
            });
        }
        ui.window().dispatch_event(WindowEvent::PointerReleased {
            position: position(480.0),
            button: PointerEventButton::Left,
        });
        assert!(ui.get_preview_width() > 300.0);
        headless_window(560, 580);
        let _ = ui.window().take_snapshot().unwrap();
        assert!(ui.get_preview_width() <= 340.0);
    }
}
