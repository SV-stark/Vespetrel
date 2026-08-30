//! Tokio -> GPUI bridge §6.2
//! This module is only compiled with `--features gpui`

use vespetrel_core::provider::SyncEvent;
use tokio::sync::mpsc;

/// Spawn a Tokio listener bound to GPUI event loop.
/// Mirrors the spec example exactly.
pub fn spawn_sync_bridge(
    cx: &mut gpui::ViewContext<crate::views::message_list::MessageListView>,
    mut rx: mpsc::UnboundedReceiver<SyncEvent>,
) {
    cx.spawn(|this, mut cx| async move {
        while let Some(event) = rx.recv().await {
            let _ = cx.update(|cx| {
                this.update(cx, |view, cx| {
                    view.handle_sync_event(event, cx);
                });
            });
        }
    }).detach();
}
