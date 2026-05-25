use std::path::PathBuf;

pub fn screenshot_dirs() -> Vec<PathBuf> {
    crate::platform::image::image_watch_dirs()
}


pub fn chromium_history_path(browser: &str) -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from)?;
        return match browser {
            "Chrome" => Some(local.join("Google/Chrome/User Data/Default/History")),
            "Edge" => Some(local.join("Microsoft/Edge/User Data/Default/History")),
            _ => None,
        };
    }

    #[cfg(target_os = "linux")]
    {
        let config = dirs::config_dir()?;
        return match browser {
            "Chrome" => {
                let chrome = config.join("google-chrome/Default/History");
                let chromium = config.join("chromium/Default/History");
                if chrome.exists() { Some(chrome) } else { Some(chromium) }
            }
            "Edge" => Some(config.join("microsoft-edge/Default/History")),
            _ => None,
        };
    }

    #[cfg(target_os = "macos")]
    {
        let home = dirs::home_dir()?;
        return match browser {
            "Chrome" => Some(home.join("Library/Application Support/Google/Chrome/Default/History")),
            "Edge" => Some(home.join("Library/Application Support/Microsoft Edge/Default/History")),
            _ => None,
        };
    }

    #[allow(unreachable_code)]
    None
}

pub fn firefox_profiles_dir() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        return dirs::data_dir().map(|d| d.join("Mozilla/Firefox/Profiles"));
    }
    #[cfg(target_os = "linux")]
    {
        return dirs::home_dir().map(|d| d.join(".mozilla/firefox"));
    }
    #[cfg(target_os = "macos")]
    {
        return dirs::home_dir().map(|d| d.join("Library/Application Support/Firefox/Profiles"));
    }
    #[allow(unreachable_code)]
    None
}
