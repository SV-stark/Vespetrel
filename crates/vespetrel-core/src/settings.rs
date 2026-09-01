//! User Preferences & GUI Customization Domain Models §8
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PaneLayout {
    /// 3-Pane Vertical (Sidebar | Message List | Message Reader)
    #[default]
    ThreePaneVertical,
    /// Classic Horizontal (Sidebar | List Top / Reader Bottom)
    ClassicHorizontal,
    /// Single Column Focus Mode (Full Width List -> Reading Overlay)
    SingleColumnFocus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SidebarMode {
    /// Expanded with folders, counts, tags
    #[default]
    Full,
    /// Compact 48px icon strip
    CompactIcons,
    /// Auto-hide on cursor leave
    AutoHide,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ColorTheme {
    #[default]
    System,
    DarkSlate,
    OledBlack,
    LightPaper,
    Nord,
    CatppuccinMocha,
    SolarizedDark,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RowDensity {
    /// 1-line tabular (Sender, Subject, Date)
    Compact,
    /// 2-line modern card (Sender + Date, Subject + Snippet)
    #[default]
    Comfortable,
    /// 3-line rich preview with badges
    Roomy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuickAction {
    #[default]
    Archive,
    Trash,
    MarkRead,
    Star,
    Snooze,
    MoveToFolder,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComposerMode {
    /// In-app bottom-right docked window
    #[default]
    BottomDock,
    /// In-pane replacement of reading view
    InPane,
    /// Detached native window
    DetachedWindow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuoteStyle {
    /// Reply above quoted previous thread (modern standard)
    #[default]
    TopPost,
    /// Traditional bottom post
    BottomPost,
    /// Inline interleaving
    Inline,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeymapPreset {
    #[default]
    Gmail,
    Thunderbird,
    Vim,
    Custom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteImagePolicy {
    #[default]
    ProxyAndSanitize,
    BlockAll,
    AllowContactsOnly,
    AllowAll,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationMode {
    #[default]
    All,
    PrimaryCategoryOnly,
    VipAndStarredOnly,
    Muted,
}

/// Comprehensive user settings profile
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UserSettings {
    // 1. Layout & Panes
    pub layout: PaneLayout,
    pub sidebar_mode: SidebarMode,
    pub unified_inbox: bool,

    // 2. Theming & Typography
    pub theme: ColorTheme,
    pub accent_color: String,
    pub font_family_ui: String,
    pub font_family_mono: String,
    pub font_size_px: f32,
    pub ui_zoom_factor: f32,

    // 3. Message List & Triaging
    pub row_density: RowDensity,
    pub show_sender_avatars: bool,
    pub show_preview_snippets: bool,
    pub visible_columns: Vec<String>,
    pub swipe_left_action: QuickAction,
    pub swipe_right_action: QuickAction,

    // 4. Composer & Writing
    pub composer_mode: ComposerMode,
    pub undo_send_seconds: u32,
    pub reply_quote_style: QuoteStyle,
    pub default_composer_font: String,
    pub default_composer_font_size: u32,

    // 5. Shortcuts & Keymap
    pub keymap_preset: KeymapPreset,
    pub custom_shortcuts: HashMap<String, String>,

    // 6. Security, Privacy & Trackers
    pub remote_image_policy: RemoteImagePolicy,
    pub auto_strip_trackers: bool,
    pub warn_on_phishing: bool,

    // 7. Snooze & Notifications
    pub snooze_later_today_hour: u8,
    pub snooze_tomorrow_hour: u8,
    pub snooze_weekend_hour: u8,
    pub snooze_next_week_hour: u8,
    pub notification_mode: NotificationMode,
}

impl Default for UserSettings {
    fn default() -> Self {
        Self {
            layout: PaneLayout::ThreePaneVertical,
            sidebar_mode: SidebarMode::Full,
            unified_inbox: true,

            theme: ColorTheme::System,
            accent_color: "#3b82f6".into(), // Modern blue
            font_family_ui: "Inter, -apple-system, BlinkMacSystemFont, Segoe UI".into(),
            font_family_mono: "JetBrains Mono, Fira Code, monospace".into(),
            font_size_px: 13.5,
            ui_zoom_factor: 1.0,

            row_density: RowDensity::Comfortable,
            show_sender_avatars: true,
            show_preview_snippets: true,
            visible_columns: vec![
                "flag".into(),
                "sender".into(),
                "subject".into(),
                "date".into(),
                "attachment".into(),
            ],
            swipe_left_action: QuickAction::Archive,
            swipe_right_action: QuickAction::Trash,

            composer_mode: ComposerMode::BottomDock,
            undo_send_seconds: 10,
            reply_quote_style: QuoteStyle::TopPost,
            default_composer_font: "sans-serif".into(),
            default_composer_font_size: 14,

            keymap_preset: KeymapPreset::Gmail,
            custom_shortcuts: HashMap::new(),

            remote_image_policy: RemoteImagePolicy::ProxyAndSanitize,
            auto_strip_trackers: true,
            warn_on_phishing: true,

            snooze_later_today_hour: 18,
            snooze_tomorrow_hour: 9,
            snooze_weekend_hour: 10,
            snooze_next_week_hour: 9,
            notification_mode: NotificationMode::All,
        }
    }
}

impl UserSettings {
    pub fn new() -> Self {
        Self::default()
    }

    /// Clamp numeric fields to valid UI bounds and sanitize CSS/font/column strings
    pub fn sanitize(&mut self) {
        self.font_size_px = self.font_size_px.clamp(10.0, 24.0);
        self.ui_zoom_factor = self.ui_zoom_factor.clamp(0.75, 2.0);
        self.undo_send_seconds = self.undo_send_seconds.min(60);
        self.snooze_later_today_hour = self.snooze_later_today_hour.min(23);
        self.snooze_tomorrow_hour = self.snooze_tomorrow_hour.min(23);
        self.snooze_weekend_hour = self.snooze_weekend_hour.min(23);
        self.snooze_next_week_hour = self.snooze_next_week_hour.min(23);

        // Validate accent_color (must match 6-digit hex #RRGGBB)
        let is_valid_hex = self.accent_color.len() == 7
            && self.accent_color.starts_with('#')
            && self.accent_color[1..].chars().all(|c| c.is_ascii_hexdigit());
        if !is_valid_hex {
            self.accent_color = "#3b82f6".into();
        }

        // Sanitize font family strings against CSS injection characters (<, >, ;, {, }, ", ')
        fn sanitize_font(s: &mut String, default_font: &str) {
            if s.chars().any(|c| matches!(c, '<' | '>' | ';' | '{' | '}' | '"' | '\''))
                || s.len() > 100
                || s.trim().is_empty()
            {
                *s = default_font.to_string();
            }
        }
        sanitize_font(&mut self.font_family_ui, "Inter, -apple-system, BlinkMacSystemFont, Segoe UI");
        sanitize_font(&mut self.font_family_mono, "JetBrains Mono, Fira Code, monospace");
        sanitize_font(&mut self.default_composer_font, "sans-serif");

        // Whitelist visible columns
        const ALLOWED_COLUMNS: &[&str] = &["flag", "sender", "subject", "date", "attachment", "size", "tags", "account"];
        self.visible_columns.retain(|col| ALLOWED_COLUMNS.contains(&col.as_str()));
        if self.visible_columns.is_empty() {
            self.visible_columns = vec!["flag".into(), "sender".into(), "subject".into(), "date".into()];
        }

        // Sanitize custom shortcuts (max 100 entries, keys & values <= 32 chars and alphanumeric/modifiers)
        self.custom_shortcuts.retain(|k, v| {
            k.len() <= 32 && v.len() <= 32
                && !k.chars().any(|c| c.is_control() || c == '<' || c == '>')
                && !v.chars().any(|c| c.is_control() || c == '<' || c == '>')
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_user_settings_defaults_and_sanitization() {
        let mut settings = UserSettings::default();
        assert_eq!(settings.layout, PaneLayout::ThreePaneVertical);
        assert_eq!(settings.theme, ColorTheme::System);
        assert_eq!(settings.undo_send_seconds, 10);
        assert!(settings.auto_strip_trackers);

        // Sanitize out-of-bounds values
        settings.font_size_px = 50.0;
        settings.ui_zoom_factor = 0.2;
        settings.undo_send_seconds = 120;
        settings.snooze_later_today_hour = 30;

        settings.sanitize();

        assert_eq!(settings.font_size_px, 24.0);
        assert_eq!(settings.ui_zoom_factor, 0.75);
        assert_eq!(settings.undo_send_seconds, 60);
        assert_eq!(settings.snooze_later_today_hour, 23);
    }

    #[test]
    fn test_user_settings_serde_roundtrip() {
        let mut custom_shortcuts = HashMap::new();
        custom_shortcuts.insert("mail.compose".into(), "Ctrl+Shift+N".into());

        let settings = UserSettings {
            theme: ColorTheme::Nord,
            accent_color: "#88c0d0".into(),
            keymap_preset: KeymapPreset::Vim,
            custom_shortcuts,
            ..Default::default()
        };

        let json = serde_json::to_string(&settings).unwrap();
        let deserialized: UserSettings = serde_json::from_str(&json).unwrap();

        assert_eq!(settings, deserialized);
    }
}
