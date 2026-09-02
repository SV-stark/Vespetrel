#[cfg(windows)]
use tracing::debug;
use tracing::info;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OsTheme {
    Dark,
    Light,
}

/// Enable Per-Monitor V2 DPI awareness on Windows to prevent blurriness on 4K/HiDPI displays
pub fn enable_high_dpi() {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::HiDpi::{
            PROCESS_PER_MONITOR_DPI_AWARE, SetProcessDpiAwareness,
        };
        unsafe {
            let res = SetProcessDpiAwareness(PROCESS_PER_MONITOR_DPI_AWARE);
            if res == 0 {
                debug!("Windows Per-Monitor DPI awareness enabled successfully");
            }
        }
    }
}

/// Detect whether the operating system is currently using Dark or Light theme (cached)
pub fn detect_system_theme() -> OsTheme {
    static CACHED_THEME: std::sync::OnceLock<OsTheme> = std::sync::OnceLock::new();
    *CACHED_THEME.get_or_init(|| {
        #[cfg(windows)]
        {
            use winreg::RegKey;
            use winreg::enums::HKEY_CURRENT_USER;
            let hkcu = RegKey::predef(HKEY_CURRENT_USER);
            if let Ok(val) = hkcu
                .open_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize")
                .and_then(|subkey| subkey.get_value::<u32, _>("AppsUseLightTheme"))
            {
                return if val == 0 {
                    OsTheme::Dark
                } else {
                    OsTheme::Light
                };
            }
        }

        #[cfg(target_os = "macos")]
        {
            if let Ok(output) = std::process::Command::new("defaults")
                .args(["read", "-g", "AppleInterfaceStyle"])
                .output()
            {
                let s = String::from_utf8_lossy(&output.stdout);
                if s.trim().eq_ignore_ascii_case("Dark") {
                    return OsTheme::Dark;
                } else if output.status.success() {
                    return OsTheme::Light;
                }
            }
        }

        #[cfg(target_os = "linux")]
        {
            if let Ok(output) = std::process::Command::new("gsettings")
                .args(["get", "org.gnome.desktop.interface", "color-scheme"])
                .output()
            {
                let s = String::from_utf8_lossy(&output.stdout);
                if s.contains("prefer-dark") || s.contains("dark") {
                    return OsTheme::Dark;
                } else if s.contains("prefer-light") || s.contains("default") {
                    return OsTheme::Light;
                }
            }
        }

        // Default to dark theme for modern desktop mail client aesthetic
        OsTheme::Dark
    })
}

/// Asynchronously detect system theme off-thread via tokio spawn_blocking
pub async fn detect_system_theme_async() -> OsTheme {
    tokio::task::spawn_blocking(detect_system_theme)
        .await
        .unwrap_or(OsTheme::Dark)
}

/// IME (Input Method Editor) state tracker for international typing (CJK, accents)
#[derive(Debug, Clone, Default)]
pub struct ImeState {
    pub is_composing: bool,
    pub composition_text: String,
    pub cursor_position: usize,
}

impl ImeState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn start_composition(&mut self) {
        self.is_composing = true;
        self.composition_text.clear();
        self.cursor_position = 0;
    }

    pub fn update_composition(&mut self, text: impl Into<String>, cursor: usize) {
        self.composition_text = text.into();
        self.cursor_position = cursor;
    }

    pub fn commit_composition(&mut self) -> String {
        self.is_composing = false;
        let committed = std::mem::take(&mut self.composition_text);
        self.cursor_position = 0;
        committed
    }

    pub fn cancel_composition(&mut self) {
        self.is_composing = false;
        self.composition_text.clear();
        self.cursor_position = 0;
    }
}

pub fn init_platform() {
    enable_high_dpi();
    let theme = detect_system_theme();
    info!(?theme, "Platform initialized with system theme");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ime_state_lifecycle() {
        let mut ime = ImeState::new();
        assert!(!ime.is_composing);

        ime.start_composition();
        assert!(ime.is_composing);

        ime.update_composition("nihao", 5);
        assert_eq!(ime.composition_text, "nihao");

        let committed = ime.commit_composition();
        assert_eq!(committed, "nihao");
        assert!(!ime.is_composing);
        assert!(ime.composition_text.is_empty());
    }

    #[test]
    fn test_system_theme_detection() {
        let theme = detect_system_theme();
        assert!(theme == OsTheme::Dark || theme == OsTheme::Light);
    }
}
