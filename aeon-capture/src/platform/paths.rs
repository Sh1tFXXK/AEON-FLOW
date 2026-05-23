use std::path::PathBuf;

pub fn screenshot_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(picture_dir) = dirs::picture_dir() {
        dirs.push(picture_dir.join("Screenshots"));
        dirs.push(picture_dir.join("屏幕截图"));
        dirs.push(picture_dir);
    }
    if let Some(home) = dirs::home_dir() {
        dirs.push(home.join("Pictures"));
        dirs.push(home.join("Documents").join("WeChat Files"));
        dirs.push(home.join("Documents").join("Tencent Files"));
    }
    dirs
}
