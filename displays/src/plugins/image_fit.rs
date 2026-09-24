//! Fits images for a vision model: a bounded long edge and encoded size, re-encoded as JPEG when needed.

use std::io::Cursor;

use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::{DynamicImage, ImageReader};

/// Longest edge, in pixels, of an image handed to a model.
pub const MAX_EDGE: u32 = 1568;
/// Largest encoded image handed to a model, in bytes before base64.
pub const MAX_BYTES: usize = 750_000;
const QUALITIES: [u8; 4] = [80, 65, 50, 35];
const SHRINK_STEPS: usize = 4;

/// An encoded image and its size in pixels.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fitted {
    pub bytes: Vec<u8>,
    pub mime: String,
    pub width: u32,
    pub height: u32,
}

/// Returns the image unchanged when it is within the limits, else a smaller JPEG that is.
pub fn fit(bytes: Vec<u8>, mime: &str) -> anyhow::Result<Fitted> {
    let (width, height) = ImageReader::new(Cursor::new(&bytes)).with_guessed_format()?.into_dimensions()?;
    if width.max(height) <= MAX_EDGE && bytes.len() <= MAX_BYTES {
        return Ok(Fitted { bytes, mime: mime.to_string(), width, height });
    }
    let source = image::load_from_memory(&bytes)?;
    let mut edge = width.max(height).min(MAX_EDGE);
    for _ in 0..SHRINK_STEPS {
        let scaled = (source.width().max(source.height()) > edge)
            .then(|| source.resize(edge, edge, FilterType::Triangle));
        let image = scaled.as_ref().unwrap_or(&source);
        for quality in QUALITIES {
            let jpeg = encode_jpeg(image, quality)?;
            if jpeg.len() <= MAX_BYTES {
                return Ok(Fitted {
                    bytes: jpeg,
                    mime: "image/jpeg".to_string(),
                    width: image.width(),
                    height: image.height(),
                });
            }
        }
        edge = edge * 3 / 4;
    }
    anyhow::bail!("image is still over {MAX_BYTES} bytes at {edge}px")
}

fn encode_jpeg(image: &DynamicImage, quality: u8) -> anyhow::Result<Vec<u8>> {
    let mut out = Vec::new();
    JpegEncoder::new_with_quality(&mut out, quality).encode_image(&image.to_rgb8())?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageFormat, Rgb, RgbImage};

    fn png(image: RgbImage) -> Vec<u8> {
        let mut out = Vec::new();
        DynamicImage::ImageRgb8(image)
            .write_to(&mut Cursor::new(&mut out), ImageFormat::Png)
            .expect("png encodes");
        out
    }

    #[test]
    fn a_large_capture_is_downscaled_to_the_long_edge() {
        let capture = RgbImage::from_fn(3840, 2160, |x, y| Rgb([(x / 16) as u8, (y / 9) as u8, 128]));
        let fitted = fit(png(capture), "image/png").expect("fits");
        assert_eq!((fitted.width, fitted.height), (1568, 882));
        assert_eq!(fitted.mime, "image/jpeg");
        assert!(fitted.bytes.len() <= MAX_BYTES);
        let decoded = image::load_from_memory(&fitted.bytes).expect("valid jpeg");
        assert_eq!((decoded.width(), decoded.height()), (1568, 882));
    }

    #[test]
    fn a_noisy_capture_still_ends_under_the_byte_cap() {
        let mut seed = 0x2545_f491_u32;
        let noise = RgbImage::from_fn(1500, 1000, |_, _| {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            Rgb([seed as u8, (seed >> 8) as u8, (seed >> 16) as u8])
        });
        let fitted = fit(png(noise), "image/png").expect("fits");
        assert!(fitted.bytes.len() <= MAX_BYTES, "{} bytes", fitted.bytes.len());
        assert!(fitted.width.max(fitted.height) <= MAX_EDGE);
    }

    #[test]
    fn an_image_within_the_limits_passes_through_untouched() {
        let jpeg = encode_jpeg(&DynamicImage::ImageRgb8(RgbImage::new(960, 540)), 60).expect("encodes");
        let fitted = fit(jpeg.clone(), "image/jpeg").expect("fits");
        assert_eq!(fitted.bytes, jpeg);
        assert_eq!((fitted.width, fitted.height, fitted.mime.as_str()), (960, 540, "image/jpeg"));
    }

    #[test]
    fn bytes_that_are_not_an_image_are_an_error() {
        assert!(fit(b"not an image".to_vec(), "image/png").is_err());
    }
}
