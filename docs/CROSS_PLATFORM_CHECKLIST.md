# Cross-platform capture checklist (Windows vs Linux)

| Item | Windows | Linux | Result |
|---|---|---|---|
| Clipboard text capture | `PlatformClipboard::get_text` | `PlatformClipboard::get_text` | ✅ same API |
| Clipboard image capture | `PlatformClipboard::get_image` PNG encode | `PlatformClipboard::get_image` PNG encode | ✅ same API |
| Foreground window capture | PowerShell + user32 | `hyprctl` / `xdotool` | ✅ both produce `ForegroundWindow` |
| Text commit capture | UIAutomation | `wl-paste` / `xclip` fallback | ✅ both produce `TextCommit` |
| Screenshot directory watch | `platform::paths::screenshot_dirs` | `platform::paths::screenshot_dirs` | ✅ unified path entry |
| Browser history path (Chrome/Edge) | LOCALAPPDATA path | XDG config path | ✅ unified helper |
| Firefox profiles path | Roaming data path | `~/.mozilla/firefox` | ✅ unified helper |
| Terminal process detection | WMI + process names | `pgrep -x` + process names | ✅ both supported |

## Notes
- Linux text commit currently uses clipboard-based fallback (`wl-paste` / `xclip`), so behavior is best-effort compared to Windows UIAutomation.
- Wayland environments may require desktop-specific tools for richer activity detail.
