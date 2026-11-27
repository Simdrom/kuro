//! Images, file dialogs, backups and the calendar file.

use super::*;

impl Backend {
    // Images and files

    pub fn image(&self, url: String) {
        let (images, reply) = (self.images.clone(), self.reply());
        self.spawn(async move {
            let bytes = images.get(&url).await;
            let image = match bytes {
                Ok(b) => {
                    let decoded = tokio::task::spawn_blocking(move || decode_image(&b)).await.ok().flatten();
                    decoded.map(|(image, smaller)| {
                        if let Some(smaller) = smaller {
                            let (images, url) = (images.clone(), url.clone());
                            tokio::spawn(async move { images.replace(&url, &smaller).await });
                        }
                        image
                    })
                }
                Err(e) => {
                    tracing::debug!("image failed: {e}");
                    None
                }
            };
            reply.send(Event::Image { url, image });
        });
    }

    pub fn image_cache_size(&self) {
        let (images, reply) = (self.images.clone(), self.reply());
        self.spawn(async move {
            if let Ok(n) = tokio::task::spawn_blocking(move || images.size()).await {
                reply.send(Event::ImageCacheSize(n));
            }
        });
    }

    pub fn maintenance(&self) {
        let (images, store) = (self.images.clone(), self.store.clone());
        self.spawn(async move {
            let _ = tokio::task::spawn_blocking(move || {
                images.trim(400 * 1024 * 1024);
                let _ = lock(&store).prune(now(), 60);
            })
            .await;
        });
    }

    pub fn export(&self, service: Service) {
        let (store, reply) = (self.store.clone(), self.reply());
        std::thread::spawn(move || {
            let Some(path) = rfd::FileDialog::new()
                .add_filter("JSON", &["json"])
                .set_file_name("kuro-backup.json")
                .save_file()
            else {
                return;
            };
            let text = lock(&store).export(service).map_err(|e| e.to_string());
            let r = text.and_then(|t| std::fs::write(path, t).map_err(|e| e.to_string()));
            reply.send(Event::Exported(r));
        });
    }

    pub fn pick_folder(&self) {
        let reply = self.reply();
        std::thread::spawn(move || {
            if let Some(path) = rfd::FileDialog::new().pick_folder() {
                reply.send(Event::BackupFolder(path.display().to_string()));
            }
        });
    }

    pub fn pick_calendar_file(&self) {
        let reply = self.reply();
        std::thread::spawn(move || {
            let path = rfd::FileDialog::new()
                .add_filter("iCalendar", &["ics"])
                .set_file_name("kuro-episodes.ics")
                .save_file();
            if let Some(path) = path {
                reply.send(Event::CalendarFile(path.display().to_string()));
            }
        });
    }

    /// Replaces the calendar file with `text`, never leaving half a file.
    pub fn write_calendar(&self, path: PathBuf, text: String) {
        let reply = self.reply();
        std::thread::spawn(move || {
            let part = path.with_extension("ics.part");
            let r = std::fs::write(&part, text).and_then(|()| std::fs::rename(&part, &path));
            reply.send(Event::CalendarWritten(r.map_err(|e| e.to_string())));
        });
    }

    /// Saves a dated copy of the list in `folder` when one is due, then
    /// removes automatic copies beyond `keep`. Other files are left alone.
    pub fn auto_backup(&self, service: Service, folder: PathBuf, every: Every, keep: u32, today: Date) {
        let (store, reply) = (self.store.clone(), self.reply());
        std::thread::spawn(move || {
            let r = run_backup(&store, service, &folder, every, keep, today).map_err(|e| e.to_string());
            if let Err(e) = &r {
                tracing::warn!("automatic backup failed: {e}");
            }
            reply.send(Event::AutoBackup(r));
        });
    }

    pub fn import(&self) {
        const MAX: u64 = 64 * 1024 * 1024;
        let reply = self.reply();
        std::thread::spawn(move || {
            let Some(path) = rfd::FileDialog::new().add_filter("JSON", &["json"]).pick_file() else { return };
            let r = match std::fs::metadata(&path) {
                Ok(m) if m.len() > MAX => Err("file is too large".to_owned()),
                Ok(_) => std::fs::read_to_string(&path).map_err(|e| e.to_string()),
                Err(e) => Err(e.to_string()),
            };
            reply.send(Event::ImportText(r));
        });
    }
}

fn run_backup(
    store: &Mutex<Store>,
    service: Service,
    folder: &std::path::Path,
    every: Every,
    keep: u32,
    today: Date,
) -> Result<Option<String>, Box<dyn std::error::Error>> {
    let names = |folder: &std::path::Path| -> std::io::Result<Vec<String>> {
        Ok(std::fs::read_dir(folder)?
            .flatten()
            .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
            .filter_map(|e| e.file_name().into_string().ok())
            .collect())
    };
    let before = names(folder)?;
    if backup::due(before.iter().map(String::as_str), today, every) {
        let text = lock(store).export(service)?;
        let path = folder.join(backup::file_name(today));
        // Written aside first, so a failed write never leaves half a backup.
        let part = path.with_extension("json.part");
        std::fs::write(&part, text)?;
        std::fs::rename(&part, &path)?;
    }
    let after = names(folder)?;
    for old in backup::surplus(after.iter().map(String::as_str), keep) {
        std::fs::remove_file(folder.join(&old))?;
    }
    Ok(backup::latest(after.iter().map(String::as_str)).map(backup::file_name))
}

/// Decodes within limits that stop decompression bombs. When the image is scaled down,
/// also returns a re-encoded copy of what is shown, if that is smaller than the original.
fn decode_image(bytes: &[u8]) -> Option<(egui::ColorImage, Option<Vec<u8>>)> {
    const MAX_SIDE: u32 = 4096;
    const SHOWN: u32 = 640;
    let mut r = image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format().ok()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_SIDE);
    limits.max_image_height = Some(MAX_SIDE);
    limits.max_alloc = Some(96 * 1024 * 1024);
    r.limits(limits);
    let mut img = r.decode().ok()?;
    let mut smaller = None;
    if img.width() > SHOWN || img.height() > SHOWN * 2 {
        img = img.thumbnail(SHOWN, SHOWN * 2);
        smaller = encode(&img).filter(|b| b.len() < bytes.len());
    }
    let rgba = img.into_rgba8();
    let size = [rgba.width() as usize, rgba.height() as usize];
    Some((egui::ColorImage::from_rgba_unmultiplied(size, &rgba), smaller))
}

/// JPEG for covers, PNG when transparency has to be kept.
fn encode(img: &image::DynamicImage) -> Option<Vec<u8>> {
    let mut out = std::io::Cursor::new(Vec::new());
    if img.color().has_alpha() {
        img.write_to(&mut out, image::ImageFormat::Png).ok()?;
    } else {
        let enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 90);
        img.to_rgb8().write_with_encoder(enc).ok()?;
    }
    Some(out.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hostile_images_are_rejected() {
        assert!(decode_image(b"not an image").is_none());
        // PNG header announcing 60000x60000 pixels.
        let mut png = b"\x89PNG\r\n\x1a\n\x00\x00\x00\x0dIHDR".to_vec();
        png.extend([0, 0, 0xEA, 0x60, 0, 0, 0xEA, 0x60, 8, 6, 0, 0, 0, 0, 0, 0, 0]);
        assert!(decode_image(&png).is_none());
    }

    #[test]
    fn large_images_are_downscaled() {
        let mut buf = std::io::Cursor::new(Vec::new());
        // Detail, so the original PNG is larger than a JPEG of the shown size.
        let noisy = image::RgbImage::from_fn(2000, 1000, |x, y| {
            let v = (x.wrapping_mul(31) ^ y.wrapping_mul(17)) as u8;
            image::Rgb([v, v.wrapping_mul(3), v ^ 0x5a])
        });
        noisy.write_to(&mut buf, image::ImageFormat::Png).unwrap();
        let (image, smaller) = decode_image(buf.get_ref()).unwrap();
        assert!(image.size[0] <= 640);
        // The smaller copy decodes to the same size and is not scaled again.
        let (again, none) = decode_image(&smaller.unwrap()).unwrap();
        assert_eq!(again.size, image.size);
        assert!(none.is_none());
    }

    #[test]
    fn small_images_are_kept_as_they_are() {
        let mut buf = std::io::Cursor::new(Vec::new());
        image::RgbImage::new(230, 325).write_to(&mut buf, image::ImageFormat::Png).unwrap();
        assert!(decode_image(buf.get_ref()).unwrap().1.is_none());
    }
}
