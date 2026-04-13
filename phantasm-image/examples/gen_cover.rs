use image::{ImageBuffer, Rgb};
use std::env;

fn main() {
    let args: Vec<String> = env::args().collect();
    let out_path = args.get(1).map(|s| s.as_str()).unwrap_or("cover.jpg");
    let size: u32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(1024);

    let mut img = ImageBuffer::new(size, size);
    for (x, y, px) in img.enumerate_pixels_mut() {
        let fx = x as f32;
        let fy = y as f32;
        let r =
            ((fx * 0.017).sin() * 80.0 + (fy * 0.023).cos() * 60.0 + 128.0).clamp(0.0, 255.0) as u8;
        let g = ((fy * 0.013).cos() * 90.0 + ((fx + fy) * 0.007).sin() * 50.0 + 128.0)
            .clamp(0.0, 255.0) as u8;
        let b = (((fx - fy) * 0.019).sin() * 70.0 + ((fx * fy) * 0.0001).cos() * 60.0 + 128.0)
            .clamp(0.0, 255.0) as u8;
        *px = Rgb([r, g, b]);
    }
    img.save(out_path).expect("failed to save cover");
    println!("wrote {out_path} ({size}x{size})");
}
