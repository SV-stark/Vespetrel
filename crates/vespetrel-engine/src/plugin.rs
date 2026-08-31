//! WASM & WebExtension Plugin Sandbox Runtime §7 Phase 6
use ahash::AHashSet;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PluginPermission {
    MessagesRead,
    MessagesModify,
    UiToolbar,
    UiSidebar,
    NetworkAccess,
    Notifications,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub author: String,
    pub permissions: AHashSet<PluginPermission>,
    pub entrypoint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PluginEvent {
    MessageReceived { message_id: String, subject: String },
    ComposeOpened { draft_id: String },
    ToolbarButtonClicked { button_id: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PluginAction {
    RegisterToolbarButton {
        id: String,
        label: String,
        icon: String,
    },
    ShowNotification {
        title: String,
        body: String,
    },
    AddMessageTag {
        message_id: String,
        tag: String,
    },
    NoOp,
}

#[derive(Debug, Clone)]
pub struct PluginHost {
    pub manifest: PluginManifest,
    pub is_enabled: bool,
}

impl PluginHost {
    pub fn new(manifest: PluginManifest) -> Self {
        Self {
            manifest,
            is_enabled: true,
        }
    }

    pub fn has_permission(&self, perm: &PluginPermission) -> bool {
        self.manifest.permissions.contains(perm)
    }

    pub fn handle_event(&self, event: &PluginEvent) -> Vec<PluginAction> {
        if !self.is_enabled {
            return Vec::new();
        }

        match event {
            PluginEvent::MessageReceived {
                message_id,
                subject,
            } => {
                if self.has_permission(&PluginPermission::MessagesRead)
                    && subject.contains("[Urgent]")
                {
                    return vec![PluginAction::AddMessageTag {
                        message_id: message_id.clone(),
                        tag: "Urgent".into(),
                    }];
                }
            }
            PluginEvent::ToolbarButtonClicked { button_id }
                if self.has_permission(&PluginPermission::Notifications) =>
            {
                return vec![PluginAction::ShowNotification {
                    title: self.manifest.name.clone(),
                    body: format!("Action triggered for {button_id}"),
                }];
            }
            _ => {}
        }

        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plugin_manifest_and_permissions() {
        let mut permissions = AHashSet::new();
        permissions.insert(PluginPermission::MessagesRead);
        permissions.insert(PluginPermission::Notifications);

        let manifest = PluginManifest {
            id: "org.vespetrel.urgent_tagger".into(),
            name: "Urgent Tagger".into(),
            version: "1.0.0".into(),
            description: "Tags urgent emails".into(),
            author: "Vespetrel Team".into(),
            permissions,
            entrypoint: "plugin.wasm".into(),
        };

        let host = PluginHost::new(manifest);
        assert!(host.has_permission(&PluginPermission::MessagesRead));
        assert!(!host.has_permission(&PluginPermission::NetworkAccess));

        let actions = host.handle_event(&PluginEvent::MessageReceived {
            message_id: "msg1".into(),
            subject: "[Urgent] Please review".into(),
        });
        assert_eq!(actions.len(), 1);
        assert!(matches!(actions[0], PluginAction::AddMessageTag { .. }));
    }
}
