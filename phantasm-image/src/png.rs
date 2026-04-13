use crate::error::ImageError;
use image::{ColorType, DynamicImage};
use std::io::BufWriter;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PngColor {
    L8,
    La8,
    Rgb8,
    Rgba8,
}

pub struct PngImage {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
    pub color: PngColor,
}

pub fn read(path: &Path) -> Result<PngImage, ImageError> {
    let img = image::open(path).map_err(|e| ImageError::InvalidFormat(e.to_string()))?;

    let (width, height, pixels, color) = match &img {
        DynamicImage::ImageLuma8(buf) => (
            buf.width(),
            buf.height(),
            buf.as_raw().clone(),
            PngColor::L8,
        ),
        DynamicImage::ImageLumaA8(buf) => (
            buf.width(),
            buf.height(),
            buf.as_raw().clone(),
            PngColor::La8,
        ),
        DynamicImage::ImageRgb8(buf) => (
            buf.width(),
            buf.height(),
            buf.as_raw().clone(),
            PngColor::Rgb8,
        ),
        DynamicImage::ImageRgba8(buf) => (
            buf.width(),
            buf.height(),
            buf.as_raw().clone(),
            PngColor::Rgba8,
        ),
        _ => {
            // Normalize uncommon formats to Rgb8
            let rgb = img.to_rgb8();
            (rgb.width(), rgb.height(), rgb.into_raw(), PngColor::Rgb8)
        }
    };

    Ok(PngImage {
        width,
        height,
        pixels,
        color,
    })
}

pub fn write(img: &PngImage, path: &Path) -> Result<(), ImageError> {
    let file = std::fs::File::create(path).map_err(ImageError::Io)?;
    let mut writer = BufWriter::new(file);

    let color_type = match img.color {
        PngColor::L8 => ColorType::L8,
        PngColor::La8 => ColorType::La8,
        PngColor::Rgb8 => ColorType::Rgb8,
        PngColor::Rgba8 => ColorType::Rgba8,
    };

    let channels = match img.color {
        PngColor::L8 => 1u32,
        PngColor::La8 => 2,
        PngColor::Rgb8 => 3,
        PngColor::Rgba8 => 4,
    };

    // Validate buffer size.
    let expected = img.width as usize * img.height as usize * channels as usize;
    if img.pixels.len() != expected {
        return Err(ImageError::InvalidFormat(format!(
            "pixel buffer length {} does not match {}x{}x{}",
            img.pixels.len(),
            img.width,
            img.height,
            channels
        )));
    }

    use image::ImageEncoder;
    image::codecs::png::PngEncoder::new(&mut writer)
        .write_image(&img.pixels, img.width, img.height, color_type)
        .map_err(|e| ImageError::InvalidFormat(e.to_string()))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn png_roundtrip() {
        // Build a 16x16 RGB gradient
        let width = 16u32;
        let height = 16u32;
        let pixels: Vec<u8> = (0..height)
            .flat_map(|y| (0..width).flat_map(move |x| [(x * 16) as u8, (y * 16) as u8, 128u8]))
            .collect();

        let tmp = NamedTempFile::with_suffix(".png").unwrap();
        let path = tmp.path();

        let img = PngImage {
            width,
            height,
            pixels: pixels.clone(),
            color: PngColor::Rgb8,
        };
        write(&img, path).unwrap();

        let loaded = read(path).unwrap();
        assert_eq!(loaded.width, width);
        assert_eq!(loaded.height, height);
        assert_eq!(loaded.pixels, pixels);
    }
}
