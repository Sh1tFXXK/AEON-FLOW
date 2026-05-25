use crate::engine::CaptureEngine;
use crate::platform::image::screenshot_save_dirs;
use std::sync::Arc;

/// 主动屏幕轮询已禁用；截图由 `screenshot` 模块监听截图工具保存的文件实现。
pub async fn start_screen_capture(_engine: Arc<CaptureEngine>) {}

/// 截图捕获能力自检：存在可监听的截图保存目录即视为可用。
pub fn screenshot_capability() -> Option<&'static str> {
    if screenshot_save_dirs().is_empty() {
        None
    } else {
        Some("file-watch")
    }
}
