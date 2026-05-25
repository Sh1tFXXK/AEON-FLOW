use crate::platform::image::{clipboard_image_png, image_dimensions};
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
        let png = clipboard_image_png()?;
        let (width, height) = image_dimensions(&png);
        Some(ClipboardImage { png, width, height })
    }
}
