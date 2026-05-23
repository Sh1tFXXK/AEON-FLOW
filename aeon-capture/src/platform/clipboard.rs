use arboard::Clipboard;

pub struct PlatformClipboard {
    inner: Clipboard,
}

pub struct ClipboardImage {
    pub png: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

impl PlatformClipboard {
    pub fn new() -> Result<Self, arboard::Error> {
        Ok(Self {
            inner: Clipboard::new()?,
        })
    }

    pub fn get_text(&mut self) -> Option<String> {
        self.inner.get_text().ok()
    }

    pub fn get_image(&mut self) -> Option<ClipboardImage> {
        let image = self.inner.get_image().ok()?;
        let width = image.width as u32;
        let height = image.height as u32;
        let rgba = image.bytes.into_owned();
        let img = image::RgbaImage::from_raw(width, height, rgba)?;

        let mut png = Vec::new();
        let encoder = image::codecs::png::PngEncoder::new(&mut png);
        use image::ImageEncoder;
        encoder
            .write_image(img.as_raw(), width, height, image::ExtendedColorType::Rgba8)
            .ok()?;

        Some(ClipboardImage { png, width, height })
    }
}
