//! Image decoding, orientation, metadata removal and bounded inference copies.
use super::{MAX_EDGE, MAX_IMAGE};
use crate::Result;
use ::image::{ImageDecoder as _, ImageFormat, ImageReader, Rgb, RgbImage};
use std::io::Cursor;

pub(super) fn normalize(source: &[u8]) -> Result<(Vec<u8>, u32, u32)> {
    let format = ::image::guess_format(source).map_err(|_| "Choose a JPEG, PNG, or WebP image.")?;
    if !matches!(
        format,
        ImageFormat::Jpeg | ImageFormat::Png | ImageFormat::WebP
    ) {
        return Err("Choose a JPEG, PNG, or WebP image.".into());
    }
    let mut reader = ImageReader::with_format(Cursor::new(source), format);
    let mut limits = ::image::Limits::default();
    limits.max_image_width = Some(16_384);
    limits.max_image_height = Some(16_384);
    limits.max_alloc = Some(128 * 1024 * 1024);
    reader.limits(limits);
    let mut decoder = reader
        .into_decoder()
        .map_err(|_| "This image is damaged or too large.")?;
    let (width, height) = decoder.dimensions();
    if u64::from(width) * u64::from(height) > 32_000_000 {
        return Err("Choose an image with at most 32 megapixels.".into());
    }
    let orientation = decoder
        .orientation()
        .map_err(|_| "Unreadable image orientation.")?;
    let mut decoded = ::image::DynamicImage::from_decoder(decoder)
        .map_err(|_| "This image is damaged or too large.")?;
    decoded.apply_orientation(orientation);
    let resized = if decoded.width() > MAX_EDGE || decoded.height() > MAX_EDGE {
        decoded.thumbnail(MAX_EDGE, MAX_EDGE)
    } else {
        decoded
    }
    .to_rgba8();
    // Flatten transparency onto white, and discard EXIF/GPS and other source metadata.
    let normalized = RgbImage::from_fn(resized.width(), resized.height(), |x, y| {
        let pixel = resized.get_pixel(x, y);
        let alpha = u32::from(pixel[3]);
        Rgb([0, 1, 2].map(|index| {
            u8::try_from((u32::from(pixel[index]) * alpha + 255 * (255 - alpha) + 127) / 255)
                .unwrap_or(255)
        }))
    });
    let mut bytes = Vec::new();
    ::image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, 85)
        .encode_image(&normalized)
        .map_err(|_| "Could not prepare this image.")?;
    if bytes.len() > MAX_IMAGE {
        return Err("This image is too detailed. Choose a smaller image.".into());
    }
    Ok((bytes, normalized.width(), normalized.height()))
}
