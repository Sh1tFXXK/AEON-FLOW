use arboard::Clipboard;

pub struct PlatformClipboard {
    inner: Clipboard,
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
}
