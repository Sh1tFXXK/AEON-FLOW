use serde::{Deserialize, Serialize};
use sysinfo::{Pid, ProcessStatus, System};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessInfo {
    pub pid: u32,
    pub name: String,
    pub exe: String,
    pub cpu_percent: f32,
    pub memory_mb: u64,
    pub status: String,
    pub kind: ProcessKind,
    pub capture_options: Vec<CaptureOption>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ProcessKind {
    KnownApp { app_id: String },
    AeonVM { vm_id: String },
    System,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureOption {
    pub id: String,
    pub label: String,
    pub description: String,
    pub icon: String,
    pub kind: CaptureOptionKind,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum CaptureOptionKind {
    DeepCapture,
    Screenshot,
    Migrate,
    Pause,
    Snapshot,
    Metadata,
}

pub fn list_processes() -> Vec<ProcessInfo> {
    let mut sys = System::new_all();
    sys.refresh_all();

    let mut result = sys
        .processes()
        .iter()
        .filter_map(|(pid, process)| {
            let name = process.name().to_string();
            if is_system_process(&name) {
                return None;
            }

            let pid_u32 = pid.as_u32();
            let exe = process
                .exe()
                .map(|path| path.to_string_lossy().to_string())
                .unwrap_or_default();
            let status = status_label(process.status()).to_string();
            let memory_mb = process.memory() / 1024 / 1024;

            let (kind, mut capture_options) = if let Some(vm_id) = detect_aeon_vm(&name, pid_u32) {
                (
                    ProcessKind::AeonVM {
                        vm_id: vm_id.clone(),
                    },
                    vm_options(&vm_id),
                )
            } else if let Some((app_id, options)) = known_app_options(&name) {
                (ProcessKind::KnownApp { app_id }, options)
            } else {
                (ProcessKind::Unknown, unknown_options(pid_u32))
            };

            if !matches!(kind, ProcessKind::AeonVM { .. })
                && !capture_options
                    .iter()
                    .any(|option| option.id.starts_with("app_state"))
            {
                capture_options.insert(
                    0,
                    option(
                        &format!("app_state_{pid_u32}"),
                        "捕获应用",
                        "保存这个应用的窗口截图和进程状态",
                        "▣",
                        CaptureOptionKind::DeepCapture,
                    ),
                );
            }
            if !matches!(kind, ProcessKind::AeonVM { .. })
                && !capture_options
                    .iter()
                    .any(|option| option.kind == CaptureOptionKind::Metadata)
            {
                capture_options.push(option(
                    "metadata",
                    "捕获进程信息",
                    "保存进程名称、PID、路径、资源占用等基本信息",
                    "i",
                    CaptureOptionKind::Metadata,
                ));
            }

            Some(ProcessInfo {
                pid: pid_u32,
                name,
                exe,
                cpu_percent: process.cpu_usage(),
                memory_mb,
                status,
                kind,
                capture_options,
            })
        })
        .collect::<Vec<_>>();

    result.sort_by(process_sort);
    result
}

pub fn process_metadata(pid: u32) -> Option<serde_json::Value> {
    let mut sys = System::new_all();
    sys.refresh_all();
    let process = sys.process(Pid::from_u32(pid))?;
    Some(serde_json::json!({
        "pid": pid,
        "name": process.name(),
        "exe": process.exe().map(|path| path.to_string_lossy().to_string()),
        "cpu_percent": process.cpu_usage(),
        "memory_mb": process.memory() / 1024 / 1024,
        "status": status_label(process.status()),
        "captured_at": now_ms(),
    }))
}

pub fn process_name(pid: u32) -> Option<String> {
    let mut sys = System::new_all();
    sys.refresh_all();
    sys.process(Pid::from_u32(pid))
        .map(|process| process.name().to_string())
}

fn known_app_options(name: &str) -> Option<(String, Vec<CaptureOption>)> {
    let lower = name.to_ascii_lowercase();
    if lower.contains("claude") {
        return Some((
            "claude-desktop".to_string(),
            vec![
                option(
                    "claude_conversation",
                    "捕获对话",
                    "保存当前完整对话记录到 AEON",
                    "💬",
                    CaptureOptionKind::DeepCapture,
                ),
                screenshot_option("捕获应用当前窗口截图"),
            ],
        ));
    }
    if lower == "code.exe" || lower.contains("visual studio code") {
        return Some((
            "vscode".to_string(),
            vec![
                option(
                    "vscode_workspace",
                    "捕获工作区",
                    "保存当前工作区状态到 AEON",
                    "📁",
                    CaptureOptionKind::DeepCapture,
                ),
                option(
                    "vscode_current_file",
                    "捕获当前文件",
                    "保存最近活动编辑状态",
                    "📄",
                    CaptureOptionKind::DeepCapture,
                ),
                screenshot_option("捕获编辑器窗口截图"),
            ],
        ));
    }
    if lower.contains("chrome") {
        return Some(("chrome".to_string(), chromium_options("Chrome")));
    }
    if lower.contains("msedge") || lower.contains("edge") {
        return Some(("edge".to_string(), browser_options("Edge", false)));
    }
    if lower.contains("firefox") {
        return Some(("firefox".to_string(), browser_options("Firefox", false)));
    }
    if is_terminal_process(&lower) {
        return Some((
            "terminal".to_string(),
            vec![
                option(
                    "terminal_state",
                    "捕获终端",
                    "保存运行中的终端和最近命令历史",
                    ">_",
                    CaptureOptionKind::DeepCapture,
                ),
                screenshot_option("捕获终端窗口截图"),
            ],
        ));
    }
    if lower.contains("winword") || lower.contains("excel") || lower.contains("powerpnt") {
        return Some((
            "office".to_string(),
            vec![
                screenshot_option("捕获 Office 窗口截图"),
                option(
                    "metadata",
                    "捕获进程信息",
                    "保存 PID、路径、资源占用等基本信息",
                    "i",
                    CaptureOptionKind::Metadata,
                ),
            ],
        ));
    }
    if lower.contains("obsidian") {
        return Some((
            "obsidian".to_string(),
            vec![
                option(
                    "obsidian_vault",
                    "捕获笔记库",
                    "保存 Obsidian 进程信息和当前状态线索",
                    "🗒",
                    CaptureOptionKind::DeepCapture,
                ),
                screenshot_option("捕获 Obsidian 窗口截图"),
            ],
        ));
    }
    None
}

fn chromium_options(browser: &str) -> Vec<CaptureOption> {
    let mut options = browser_options(browser, true);
    options.insert(
        2,
        option(
            "browser_bookmarks",
            "捕获书签",
            "保存 Chrome 书签文件",
            "🔖",
            CaptureOptionKind::DeepCapture,
        ),
    );
    options
}

fn browser_options(browser: &str, _chromium: bool) -> Vec<CaptureOption> {
    vec![
        option(
            "browser_tab",
            "捕获最近网页",
            &format!("抓取 {browser} 最近访问网页的真实正文"),
            "🌐",
            CaptureOptionKind::DeepCapture,
        ),
        option(
            "browser_pages",
            "捕获网页列表",
            &format!("列出 {browser} 最近访问的网页标题和 URL"),
            "☰",
            CaptureOptionKind::DeepCapture,
        ),
        screenshot_option("捕获浏览器窗口截图"),
    ]
}

fn unknown_options(pid: u32) -> Vec<CaptureOption> {
    vec![
        option(
            &format!("screenshot_{pid}"),
            "截图",
            "捕获应用当前窗口截图",
            "📸",
            CaptureOptionKind::Screenshot,
        ),
        option(
            &format!("metadata_{pid}"),
            "捕获进程信息",
            "保存进程名称、PID、资源占用等基本信息",
            "i",
            CaptureOptionKind::Metadata,
        ),
    ]
}

fn vm_options(vm_id: &str) -> Vec<CaptureOption> {
    vec![
        option(
            &format!("migrate_{vm_id}"),
            "迁移到其他设备",
            "将此 VM 状态同步到另一台设备继续运行",
            "⇄",
            CaptureOptionKind::Migrate,
        ),
        option(
            &format!("snapshot_{vm_id}"),
            "快照",
            "保存当前 VM 状态",
            "💾",
            CaptureOptionKind::Snapshot,
        ),
        option(
            &format!("pause_{vm_id}"),
            "暂停",
            "暂停 VM 记录并保留状态",
            "Ⅱ",
            CaptureOptionKind::Pause,
        ),
    ]
}

fn screenshot_option(description: &str) -> CaptureOption {
    option(
        "screenshot",
        "截图",
        description,
        "📸",
        CaptureOptionKind::Screenshot,
    )
}

fn option(
    id: &str,
    label: &str,
    description: &str,
    icon: &str,
    kind: CaptureOptionKind,
) -> CaptureOption {
    CaptureOption {
        id: id.to_string(),
        label: label.to_string(),
        description: description.to_string(),
        icon: icon.to_string(),
        kind,
    }
}

fn is_terminal_process(lower: &str) -> bool {
    [
        "windowsterminal",
        "wt.exe",
        "powershell",
        "pwsh",
        "cmd.exe",
        "bash.exe",
        "mintty",
        "wezterm",
    ]
    .iter()
    .any(|name| lower.contains(name))
}

fn is_system_process(name: &str) -> bool {
    const SYSTEM_PROCS: &[&str] = &[
        "System",
        "Registry",
        "smss.exe",
        "csrss.exe",
        "wininit.exe",
        "services.exe",
        "lsass.exe",
        "svchost.exe",
        "dwm.exe",
        "explorer.exe",
        "RuntimeBroker.exe",
        "SearchHost.exe",
        "StartMenuExperienceHost.exe",
        "fontdrvhost.exe",
        "conhost.exe",
        "taskhostw.exe",
    ];
    SYSTEM_PROCS
        .iter()
        .any(|process| name.eq_ignore_ascii_case(process))
}

fn detect_aeon_vm(name: &str, pid: u32) -> Option<String> {
    let lower = name.to_ascii_lowercase();
    if lower.contains("aeon-vm") || lower.contains("aeon-run") {
        Some(format!("pid-{pid}"))
    } else {
        None
    }
}

fn status_label(status: ProcessStatus) -> &'static str {
    match status {
        ProcessStatus::Run => "running",
        ProcessStatus::Sleep => "sleeping",
        ProcessStatus::Idle => "idle",
        ProcessStatus::Zombie => "zombie",
        ProcessStatus::Stop => "stopped",
        _ => "other",
    }
}

fn process_sort(a: &ProcessInfo, b: &ProcessInfo) -> std::cmp::Ordering {
    rank(a)
        .cmp(&rank(b))
        .then_with(|| {
            b.cpu_percent
                .partial_cmp(&a.cpu_percent)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .then_with(|| {
            a.name
                .to_ascii_lowercase()
                .cmp(&b.name.to_ascii_lowercase())
        })
        .then_with(|| a.pid.cmp(&b.pid))
}

fn rank(process: &ProcessInfo) -> u8 {
    match process.kind {
        ProcessKind::AeonVM { .. } => 0,
        ProcessKind::KnownApp { .. } => 1,
        ProcessKind::Unknown => 2,
        ProcessKind::System => 3,
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_known_apps() {
        let (app_id, options) = known_app_options("Code.exe").unwrap();

        assert_eq!(app_id, "vscode");
        assert!(options.iter().any(|option| option.id == "vscode_workspace"));
    }

    #[test]
    fn classifies_terminal_apps() {
        let (app_id, options) = known_app_options("powershell.exe").unwrap();

        assert_eq!(app_id, "terminal");
        assert!(options.iter().any(|option| option.id == "terminal_state"));
    }

    #[test]
    fn browser_apps_can_capture_page_lists() {
        let (_, options) = known_app_options("msedge.exe").unwrap();

        assert!(options.iter().any(|option| option.id == "browser_pages"));
    }

    #[test]
    fn unknown_process_has_safe_options() {
        let options = unknown_options(42);

        assert_eq!(options.len(), 2);
        assert_eq!(options[0].id, "screenshot_42");
        assert_eq!(options[1].kind, CaptureOptionKind::Metadata);
    }
}
