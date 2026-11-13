//! Bounded PNG input for the encoder build, which intentionally omits zlib/PNG.
//! Only a temporary PPM is produced; FFmpeg retains scaling/JPEG encoding and null stdin.
use std::{
    fs::File,
    io::{Cursor, Read, Write},
    path::Path,
};
use tokio::sync::watch;

const MAX_INPUT: u64 = 16 * 1024 * 1024;
const MAX_DECODED: usize = 64 * 1024 * 1024;
const ERROR: &str = "The provider PNG thumbnail could not be decoded within the image limits.";

pub(super) fn prepare(
    path: &Path,
    cancel: &watch::Receiver<bool>,
) -> Result<Option<tempfile::NamedTempFile>, String> {
    if *cancel.borrow() {
        return Ok(None);
    }
    let mut bytes = Vec::new();
    File::open(path)
        .and_then(|file| file.take(MAX_INPUT + 1).read_to_end(&mut bytes))
        .map_err(|_| ERROR)?;
    if bytes.len() as u64 > MAX_INPUT {
        return Err(ERROR.into());
    }
    // Detect content, so images with a mismatched filename still work as before.
    if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Ok(None);
    }
    let mut decoder =
        png::Decoder::new_with_limits(Cursor::new(bytes), png::Limits { bytes: MAX_DECODED });
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().map_err(|_| ERROR)?;
    let size = reader
        .output_buffer_size()
        .filter(|&size| size <= MAX_DECODED)
        .ok_or(ERROR)?;
    let mut pixels = vec![0; size];
    let frame = reader.next_frame(&mut pixels).map_err(|_| ERROR)?;
    if frame.bit_depth != png::BitDepth::Eight || frame.width == 0 || frame.height == 0 {
        return Err(ERROR.into());
    }
    let channels = match frame.color_type {
        png::ColorType::Rgb => 3,
        png::ColorType::Rgba => 4,
        png::ColorType::Grayscale => 1,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Indexed => return Err(ERROR.into()),
    };
    let mut temporary = tempfile::Builder::new()
        .prefix("klipper-preview-")
        .suffix(".ppm")
        .tempfile()
        .map_err(|_| "Could not create temporary provider preview")?;
    write!(temporary, "P6\n{} {}\n255\n", frame.width, frame.height).map_err(|_| ERROR)?;
    for row in pixels[..frame.buffer_size()].chunks_exact(frame.width as usize * channels) {
        if *cancel.borrow() {
            return Ok(None);
        }
        let mut rgb = Vec::with_capacity(frame.width as usize * 3);
        for pixel in row.chunks_exact(channels) {
            if channels <= 2 {
                rgb.extend_from_slice(&[pixel[0]; 3]);
            } else {
                rgb.extend_from_slice(&pixel[..3]);
            }
        }
        temporary.write_all(&rgb).map_err(|_| ERROR)?;
    }
    temporary.flush().map_err(|_| ERROR)?;
    Ok(Some(temporary))
}
