use super::{
    AtomicU64, BTreeMap, Command, MAX_RENDER_BYTES, Mutex, Ordering, Path, PathBuf, Pixels,
    RENDER_TIMEOUT, ScratchDir, checked_pixel_count, fs, pixels_from_rgba, read_output,
    run_command,
};
use std::ffi::OsStr;
use std::io::Cursor;
use std::os::unix::fs::PermissionsExt;
use std::sync::OnceLock;

const NAMES: &[&str] = &[
    "fc-match",
    "ffmpeg",
    "ffprobe",
    "magick",
    "convert",
    "gm",
    "resvg",
    "rsvg-convert",
    "pdftoppm",
    "pdfinfo",
    "mutool",
    "gsf-office-thumbnailer",
    "soffice",
    "libreoffice",
    "lowriter",
    "bat",
    "batcat",
    "python3",
    "unzip",
    "xmllint",
    "catdoc",
];

fn installed() -> &'static BTreeMap<String, bool> {
    static TOOLS: OnceLock<BTreeMap<String, bool>> = OnceLock::new();
    TOOLS.get_or_init(|| {
        let directories = std::env::var_os("PATH")
            .map(|path| std::env::split_paths(&path).collect::<Vec<_>>())
            .unwrap_or_default();
        NAMES
            .iter()
            .map(|name| {
                let found = directories.iter().any(|directory| {
                    fs::metadata(directory.join(name))
                        .is_ok_and(|info| info.is_file() && info.permissions().mode() & 0o111 != 0)
                });
                ((*name).to_owned(), found)
            })
            .collect()
    })
}

pub(super) fn available(name: &OsStr) -> bool {
    installed()
        .get(&name.to_string_lossy().into_owned())
        .copied()
        .unwrap_or(true)
}

fn png_reader(bytes: Vec<u8>) -> Result<png::Reader<Cursor<Vec<u8>>>, &'static str> {
    let mut decoder = png::Decoder::new_with_limits(
        Cursor::new(bytes),
        png::Limits {
            bytes: 32 * 1024 * 1024,
        },
    );
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    decoder.read_info().map_err(|_| "Cannot decode PNG")
}

fn png_pixels(mut reader: png::Reader<Cursor<Vec<u8>>>) -> Result<Pixels, &'static str> {
    checked_pixel_count(
        reader.info().width,
        reader.info().height,
        4,
        usize::try_from(u64::from(reader.info().width) * u64::from(reader.info().height) * 4)
            .unwrap_or(0),
    )?;
    let size = reader
        .output_buffer_size()
        .filter(|size| *size <= usize::try_from(MAX_RENDER_BYTES).unwrap())
        .ok_or("PNG too large")?;
    let mut bytes = vec![0; size];
    let info = reader
        .next_frame(&mut bytes)
        .map_err(|_| "Cannot decode PNG")?;
    let bytes = &bytes[..info.buffer_size()];
    let mut rgba = Vec::with_capacity((info.width * info.height * 4) as usize);
    match info.color_type {
        png::ColorType::Rgba => rgba.extend_from_slice(bytes),
        png::ColorType::Rgb => {
            for pixel in bytes.chunks_exact(3) {
                rgba.extend_from_slice(&[pixel[0], pixel[1], pixel[2], 255]);
            }
        }
        png::ColorType::Grayscale => {
            for value in bytes {
                rgba.extend_from_slice(&[*value, *value, *value, 255]);
            }
        }
        png::ColorType::GrayscaleAlpha => {
            for pixel in bytes.chunks_exact(2) {
                rgba.extend_from_slice(&[pixel[0], pixel[0], pixel[0], pixel[1]]);
            }
        }
        png::ColorType::Indexed => return Err("Cannot decode indexed PNG"),
    }
    pixels_from_rgba(info.width, info.height, &rgba)
}

fn read_png(path: &Path) -> Result<Pixels, &'static str> {
    png_pixels(png_reader(read_output(path, MAX_RENDER_BYTES)?)?)
}

pub(super) fn cached_thumbnail(path: &Path, min_width: u32) -> Option<Pixels> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().ok()?.join(path)
    };
    let uri = url::Url::from_file_path(&absolute).ok()?.to_string();
    let metadata = fs::metadata(path).ok()?;
    let cache = std::env::var_os("XDG_CACHE_HOME")
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))?;
    thumbnail_in(&cache, &uri, &metadata, min_width)
}

fn thumbnail_in(
    cache: &Path,
    uri: &str,
    metadata: &fs::Metadata,
    min_width: u32,
) -> Option<Pixels> {
    let mtime = metadata
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs()
        .to_string();
    let name = format!("{:x}.png", md5::compute(uri.as_bytes()));
    for category in ["xx-large", "x-large", "large", "normal"] {
        let result = (|| {
            let mut reader = png_reader(
                read_output(
                    &cache.join("thumbnails").join(category).join(&name),
                    MAX_RENDER_BYTES,
                )
                .ok()?,
            )
            .ok()?;
            if reader.info().width < min_width {
                return None;
            }
            // Consume the frame first: text chunks may follow IDAT.
            let pixels = {
                checked_pixel_count(
                    reader.info().width,
                    reader.info().height,
                    4,
                    usize::try_from(
                        u64::from(reader.info().width) * u64::from(reader.info().height) * 4,
                    )
                    .unwrap_or(0),
                )
                .ok()?;
                let size = reader
                    .output_buffer_size()
                    .filter(|size| *size <= usize::try_from(MAX_RENDER_BYTES).unwrap())?;
                let mut bytes = vec![0; size];
                reader.next_frame(&mut bytes).ok()?;
                reader.finish().ok()?;
                bytes
            };
            let info = reader.info();
            let mut tags = BTreeMap::new();
            for tag in &info.uncompressed_latin1_text {
                tags.insert(tag.keyword.clone(), tag.text.clone());
            }
            for tag in &info.compressed_latin1_text {
                tags.insert(tag.keyword.clone(), tag.get_text().ok()?);
            }
            for tag in &info.utf8_text {
                tags.insert(tag.keyword.clone(), tag.get_text().ok()?);
            }
            if tags.get("Thumb::URI").map(String::as_str) != Some(uri)
                || tags.get("Thumb::MTime") != Some(&mtime)
            {
                return None;
            }
            if let Some(size) = tags.get("Thumb::Size")
                && size != &metadata.len().to_string()
            {
                return None;
            }
            drop(pixels);
            read_png(&cache.join("thumbnails").join(category).join(&name)).ok()
        })();
        if result.is_some() {
            return result;
        }
    }
    None
}

pub(super) fn render_svg(
    path: &Path,
    scratch: &ScratchDir,
    request: u64,
    serial: &AtomicU64,
) -> Result<Pixels, &'static str> {
    let output = scratch.join("svg.png");
    for executable in ["resvg", "rsvg-convert"] {
        let mut command = Command::new(executable);
        if executable == "resvg" {
            command.args(["--width", "1200", "--height", "1200"]);
            command.arg(path).arg(&output);
        } else {
            command.args([
                "--keep-aspect-ratio",
                "--width",
                "1200",
                "--height",
                "1200",
                "--output",
            ]);
            command.arg(&output).arg(path);
        }
        if run_command(
            command,
            &output,
            MAX_RENDER_BYTES,
            RENDER_TIMEOUT,
            request,
            serial,
            "SVG renderer unavailable",
            "Cannot render SVG",
        )
        .is_ok()
            && let Ok(pixels) = read_png(&output)
        {
            return Ok(pixels);
        }
        if serial.load(Ordering::Acquire) != request {
            return Err("Preview cancelled");
        }
    }
    Err("Install resvg or librsvg for SVG previews")
}

pub(super) fn copy_text(text: String) -> Result<(), String> {
    use clipboard_rs::{Clipboard, ClipboardContext};
    static CLIPBOARD: Mutex<Option<ClipboardContext>> = Mutex::new(None);
    let mut clipboard = CLIPBOARD.lock().map_err(|error| error.to_string())?;
    if clipboard.is_none() {
        *clipboard = Some(ClipboardContext::new().map_err(|error| error.to_string())?);
    }
    clipboard
        .as_ref()
        .unwrap()
        .set_text(text)
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    fn thumbnail(path: &Path, uri: &str, mtime: &str) {
        let file = File::create(path).unwrap();
        let mut encoder = png::Encoder::new(file, 256, 128);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .add_text_chunk("Thumb::URI".into(), uri.into())
            .unwrap();
        encoder
            .add_text_chunk("Thumb::MTime".into(), mtime.into())
            .unwrap();
        let mut writer = encoder.write_header().unwrap();
        writer
            .write_image_data(&[10, 20, 30, 128].repeat(256 * 128))
            .unwrap();
    }
    #[test]
    fn desktop_thumbnails_validate_source_and_keep_alpha() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("a file #é.pdf");
        fs::write(&source, b"source").unwrap();
        let uri = url::Url::from_file_path(&source).unwrap().to_string();
        let metadata = fs::metadata(&source).unwrap();
        let mtime = metadata
            .modified()
            .unwrap()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
            .to_string();
        let folder = dir.path().join("thumbnails/large");
        fs::create_dir_all(&folder).unwrap();
        let thumb = folder.join(format!("{:x}.png", md5::compute(uri.as_bytes())));
        thumbnail(&thumb, &uri, &mtime);
        let pixels = thumbnail_in(dir.path(), &uri, &metadata, 128).unwrap();
        assert_eq!((pixels.width(), pixels.height()), (256, 128));
        assert_eq!(&pixels.as_bytes()[..4], &[10, 20, 30, 128]);
        assert!(thumbnail_in(dir.path(), &uri, &metadata, 300).is_none());
        thumbnail(&thumb, "file:///wrong", &mtime);
        assert!(thumbnail_in(dir.path(), &uri, &metadata, 128).is_none());
        thumbnail(&thumb, &uri, "0");
        assert!(thumbnail_in(dir.path(), &uri, &metadata, 128).is_none());
        fs::write(&thumb, b"not png").unwrap();
        assert!(thumbnail_in(dir.path(), &uri, &metadata, 128).is_none());
    }
}
