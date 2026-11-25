//! Writes `assets/app.ico`. Run after changing the drawing in `icon.rs`.

use mango_app::icon;
use mango_core::settings::AppIcon;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut images = Vec::new();
    for size in [16, 32, 48, 64, 128, 256] {
        let i = icon::render(AppIcon::Sunrise, size);
        let mut png = std::io::Cursor::new(Vec::new());
        image::RgbaImage::from_raw(size, size, i.rgba)
            .ok_or("size")?
            .write_to(&mut png, image::ImageFormat::Png)?;
        images.push((size, png.into_inner()));
    }
    let mut ico = vec![0, 0, 1, 0, images.len() as u8, 0];
    let mut offset = 6 + 16 * images.len() as u32;
    for (size, png) in &images {
        let s = (*size % 256) as u8;
        ico.extend([s, s, 0, 0, 1, 0, 32, 0]);
        ico.extend((png.len() as u32).to_le_bytes());
        ico.extend(offset.to_le_bytes());
        offset += png.len() as u32;
    }
    images.iter().for_each(|(_, png)| ico.extend(png));
    std::fs::write(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/app.ico"), ico)?;
    Ok(())
}
