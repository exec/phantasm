//! ITU-R BT.601 YCbCr ↔ RGB conversions (JPEG-standard formulation).

pub fn rgb_to_ycbcr(rgb: [u8; 3]) -> [u8; 3] {
    let r = rgb[0] as f32;
    let g = rgb[1] as f32;
    let b = rgb[2] as f32;

    let y = 0.299 * r + 0.587 * g + 0.114 * b;
    let cb = -0.168_736 * r - 0.331_264 * g + 0.5 * b + 128.0;
    let cr = 0.5 * r - 0.418_688 * g - 0.081_312 * b + 128.0;

    [clamp_u8(y), clamp_u8(cb), clamp_u8(cr)]
}

pub fn ycbcr_to_rgb(ycbcr: [u8; 3]) -> [u8; 3] {
    let y = ycbcr[0] as f32;
    let cb = ycbcr[1] as f32 - 128.0;
    let cr = ycbcr[2] as f32 - 128.0;

    let r = y + 1.402 * cr;
    let g = y - 0.344_136 * cb - 0.714_136 * cr;
    let b = y + 1.772 * cb;

    [clamp_u8(r), clamp_u8(g), clamp_u8(b)]
}

pub fn rgb_to_ycbcr_image(rgb: &[u8]) -> Vec<u8> {
    assert!(
        rgb.len().is_multiple_of(3),
        "rgb buffer must be a multiple of 3 bytes"
    );
    rgb.chunks_exact(3)
        .flat_map(|px| {
            let out = rgb_to_ycbcr([px[0], px[1], px[2]]);
            [out[0], out[1], out[2]]
        })
        .collect()
}

pub fn ycbcr_to_rgb_image(ycbcr: &[u8]) -> Vec<u8> {
    assert!(
        ycbcr.len().is_multiple_of(3),
        "ycbcr buffer must be a multiple of 3 bytes"
    );
    ycbcr
        .chunks_exact(3)
        .flat_map(|px| {
            let out = ycbcr_to_rgb([px[0], px[1], px[2]]);
            [out[0], out[1], out[2]]
        })
        .collect()
}

#[inline]
fn clamp_u8(v: f32) -> u8 {
    v.round().clamp(0.0, 255.0) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gray_roundtrip() {
        let ycbcr = rgb_to_ycbcr([128, 128, 128]);
        assert_eq!(ycbcr, [128, 128, 128]);
        let rgb = ycbcr_to_rgb(ycbcr);
        assert_eq!(rgb, [128, 128, 128]);
    }

    #[test]
    fn red_pixel_bt601() {
        let ycbcr = rgb_to_ycbcr([255, 0, 0]);
        // BT.601: Y≈76, Cb≈85, Cr≈255
        let y = ycbcr[0] as i32;
        let cb = ycbcr[1] as i32;
        let cr = ycbcr[2] as i32;
        assert!((y - 76).abs() <= 2, "Y={y}");
        assert!((cb - 85).abs() <= 2, "Cb={cb}");
        assert!((cr - 255).abs() <= 2, "Cr={cr}");
    }

    #[test]
    fn image_roundtrip() {
        let rgb: Vec<u8> = (0u8..=254)
            .step_by(3)
            .flat_map(|v| [v, v / 2, 255 - v])
            .collect();
        let ycbcr = rgb_to_ycbcr_image(&rgb);
        let back = ycbcr_to_rgb_image(&ycbcr);
        // Allow ±1 rounding error per channel
        for (a, b) in rgb.iter().zip(back.iter()) {
            assert!((*a as i32 - *b as i32).abs() <= 2);
        }
    }
}
