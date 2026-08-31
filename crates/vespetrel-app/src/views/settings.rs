//! Settings & Preferences View Model §8
use serde::{Deserialize, Serialize};
use vespetrel_core::{
    ColorTheme, ComposerMode, KeymapPreset, NotificationMode, PaneLayout, QuoteStyle,
    RemoteImagePolicy, RowDensity, SidebarMode, UserSettings,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum SettingsTab {
    #[default]
    Appearance,
    Layout,
    ReadingAndList,
    Composer,
    Shortcuts,
    PrivacyAndSecurity,
    Notifications,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SettingsViewState {
    pub is_open: bool,
    pub active_tab: SettingsTab,
    pub settings: UserSettings,
    pub is_dirty: bool,
}

impl Default for SettingsViewState {
    fn default() -> Self {
        Self {
            is_open: false,
            active_tab: SettingsTab::Appearance,
            settings: UserSettings::default(),
            is_dirty: false,
        }
    }
}

impl SettingsViewState {
    pub fn new(settings: UserSettings) -> Self {
        Self {
            is_open: false,
            active_tab: SettingsTab::Appearance,
            settings,
            is_dirty: false,
        }
    }

    pub fn open(&mut self) {
        self.is_open = true;
    }

    pub fn close(&mut self) {
        self.is_open = false;
    }

    pub fn set_tab(&mut self, tab: SettingsTab) {
        self.active_tab = tab;
    }

    pub fn set_theme(&mut self, theme: ColorTheme) {
        self.settings.theme = theme;
        self.is_dirty = true;
    }

    pub fn set_accent_color(&mut self, hex: impl Into<String>) {
        self.settings.accent_color = hex.into();
        self.is_dirty = true;
    }

    pub fn set_layout(&mut self, layout: PaneLayout) {
        self.settings.layout = layout;
        self.is_dirty = true;
    }

    pub fn set_sidebar_mode(&mut self, mode: SidebarMode) {
        self.settings.sidebar_mode = mode;
        self.is_dirty = true;
    }

    pub fn set_row_density(&mut self, density: RowDensity) {
        self.settings.row_density = density;
        self.is_dirty = true;
    }

    pub fn set_composer_mode(&mut self, mode: ComposerMode) {
        self.settings.composer_mode = mode;
        self.is_dirty = true;
    }

    pub fn set_undo_send_seconds(&mut self, secs: u32) {
        self.settings.undo_send_seconds = secs.min(60);
        self.is_dirty = true;
    }

    pub fn set_quote_style(&mut self, style: QuoteStyle) {
        self.settings.reply_quote_style = style;
        self.is_dirty = true;
    }

    pub fn set_keymap_preset(&mut self, preset: KeymapPreset) {
        self.settings.keymap_preset = preset;
        self.is_dirty = true;
    }

    pub fn set_remote_image_policy(&mut self, policy: RemoteImagePolicy) {
        self.settings.remote_image_policy = policy;
        self.is_dirty = true;
    }

    pub fn set_notification_mode(&mut self, mode: NotificationMode) {
        self.settings.notification_mode = mode;
        self.is_dirty = true;
    }

    pub fn save_changes(&mut self) -> UserSettings {
        self.settings.sanitize();
        self.is_dirty = false;
        self.settings.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_settings_view_state_tab_and_mutation() {
        let mut view = SettingsViewState::default();
        assert!(!view.is_open);
        assert_eq!(view.active_tab, SettingsTab::Appearance);

        view.open();
        assert!(view.is_open);

        view.set_tab(SettingsTab::Composer);
        assert_eq!(view.active_tab, SettingsTab::Composer);

        view.set_theme(ColorTheme::OledBlack);
        view.set_accent_color("#10b981");
        view.set_undo_send_seconds(30);
        assert!(view.is_dirty);

        let saved = view.save_changes();
        assert!(!view.is_dirty);
        assert_eq!(saved.theme, ColorTheme::OledBlack);
        assert_eq!(saved.accent_color, "#10b981");
        assert_eq!(saved.undo_send_seconds, 30);
    }
}
