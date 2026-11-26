//! Fit native captures to the engine's 5 MiB model-image and 16 MiB MCP message limits.
use std::io::Cursor;

use base64::Engine;

use super::{Reply, TEXT_LIMIT, clip};

const PNG_BUDGET: usize = 5 * 1024 * 1024;

/// A source limit, not the wire limit: e.g. a noisy 4K WebView2 PNG can exceed 16 MiB.
pub(super) fn check_source_size(bytes: u64) -> Result<(), String> {
    if (1..=64 * 1024 * 1024).contains(&bytes) {
        Ok(())
    } else {
        Err(format!(
            "Unexpected screenshot size: {bytes} bytes (64 MiB source limit)"
        ))
    }
}

pub(super) fn reply(text: String, png: Vec<u8>) -> Result<Reply, String> {
    Ok(Reply {
        ok: true,
        text: clip(&text, TEXT_LIMIT),
        image: Some(base64::engine::general_purpose::STANDARD.encode(fit(png)?)),
    })
}

fn fit(png: Vec<u8>) -> Result<Vec<u8>, String> {
    if png.len() <= PNG_BUDGET {
        return Ok(png);
    }
    // Leave room for filters and compression overhead even on a high-entropy capture.
    let scaled = downsample(&png, 1_000_000)?;
    if scaled.len() > PNG_BUDGET {
        return Err("Screenshot is too large to send; reduce the captured view's size".into());
    }
    Ok(scaled)
}

fn downsample(png: &[u8], max_pixels: usize) -> Result<Vec<u8>, String> {
    let mut decoder = png::Decoder::new(Cursor::new(png));
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
    let size = reader
        .output_buffer_size()
        .filter(|s| *s <= 128 * 1024 * 1024)
        .ok_or("Screenshot exceeds the decoded image limit")?;
    let mut pixels = vec![0; size];
    let info = reader.next_frame(&mut pixels).map_err(|e| e.to_string())?;
    let (w, h) = (info.width as usize, info.height as usize);
    let mut step = 2;
    while w.div_ceil(step) * h.div_ceil(step) > max_pixels {
        step += 1;
    }
    let channels = info.color_type.samples();
    let mut sampled = Vec::with_capacity(w.div_ceil(step) * h.div_ceil(step) * channels);
    for row in pixels[..info.buffer_size()]
        .chunks_exact(info.line_size)
        .step_by(step)
    {
        for pixel in row.chunks_exact(channels).step_by(step) {
            sampled.extend_from_slice(pixel);
        }
    }
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, w.div_ceil(step) as u32, h.div_ceil(step) as u32);
    encoder.set_color(info.color_type);
    encoder.set_depth(info.bit_depth);
    encoder
        .write_header()
        .and_then(|mut writer| writer.write_image_data(&sampled))
        .map_err(|e| e.to_string())?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_captures_survive_the_model_and_stdio_limits() {
        assert!(check_source_size(0).is_err());
        assert!(check_source_size(64 * 1024 * 1024 + 1).is_err());
        for (width, height, lower_bound) in [(32, 32, 0), (1920, 1080, 5), (3840, 2160, 16)] {
            let mut pixels = vec![0x40; width * height * 3];
            pixels[width * (height / 2) * 3..].fill(0xe0);
            let mut source = Vec::new();
            let mut encoder = png::Encoder::new(&mut source, width as u32, height as u32);
            encoder.set_depth(png::BitDepth::Eight);
            encoder.set_color(png::ColorType::Rgb);
            // Real, decodable PNGs just above both formerly mishandled size boundaries.
            encoder.set_compression(png::Compression::NoCompression);
            encoder
                .write_header()
                .unwrap()
                .write_image_data(&pixels)
                .unwrap();
            assert!(source.len() > lower_bound * 1024 * 1024);
            assert!(check_source_size(source.len() as u64).is_ok());
            let original = (lower_bound == 0).then(|| source.clone());
            let shot = reply("\0".repeat(TEXT_LIMIT * 2), source).unwrap();
            let result = crate::browser_mcp::content(shot);
            assert!(serde_json::to_vec(&result).unwrap().len() < 16 * 1024 * 1024 - 1024);
            let data = base64::engine::general_purpose::STANDARD
                .decode(result["content"][1]["data"].as_str().unwrap())
                .unwrap();
            assert!(data.len() <= 5 * 1024 * 1024);
            if let Some(original) = original {
                assert_eq!(data, original);
                continue;
            }
            let mut reader = png::Decoder::new(Cursor::new(data)).read_info().unwrap();
            let mut scaled = vec![0; reader.output_buffer_size().unwrap()];
            let info = reader.next_frame(&mut scaled).unwrap();
            assert!(info.width >= 960 && info.height >= 540, "not a thumbnail");
            assert_eq!(
                u64::from(info.width) * height as u64,
                u64::from(info.height) * width as u64
            );
            assert_eq!((scaled[0], scaled[info.buffer_size() - 1]), (0x40, 0xe0));
        }
    }
}
