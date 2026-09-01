#[cfg(feature = "gpui")]
pub mod gpui_app {
    use gpui::*;
    use vespetrel_core::{
        Account, CalendarEvent, Contact, Folder, FolderRole, MessageSummary,
        ProviderType, TaskItem, UserSettings, provider::SyncEvent,
    };
    use crate::views::message_list::ListFilter;

    /// Active top-level navigation view
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum ActiveViewTab {
        Mail,
        Calendar,
        Contacts,
        Tasks,
        Settings,
    }

    /// Active modal overlay
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum ActiveModal {
        None,
        Compose,
        CommandPalette,
        AddAccount,
    }

    pub struct MainWindow {
        pub active_tab: ActiveViewTab,
        pub active_modal: ActiveModal,
        pub accounts: Vec<Account>,
        pub folders: Vec<Folder>,
        pub selected_folder_id: Option<String>,
        pub messages: Vec<MessageSummary>,
        pub selected_message_id: Option<String>,
        pub list_filter: ListFilter,
        pub search_query: String,
        pub block_remote_images: bool,
        // PIM data
        pub calendar_events: Vec<CalendarEvent>,
        pub contacts: Vec<Contact>,
        pub tasks: Vec<TaskItem>,
        // Settings
        pub settings: UserSettings,
        // Compose State
        pub compose_to: String,
        pub compose_subject: String,
        pub compose_body: String,
        // Command Palette
        pub palette_query: String,
        // Event channel from Tokio sync engine
        pub sync_sender: flume::Sender<SyncEvent>,
        pub status_message: String,
    }

    impl MainWindow {
        pub fn new(
            cx: &mut Context<Self>,
            sync_rx: flume::Receiver<SyncEvent>,
            sync_tx: flume::Sender<SyncEvent>,
        ) -> Self {
            let default_account = Account::new(
                "Primary Account",
                "user@vespetrel.example",
                ProviderType::Imap,
            );

            let folders = vec![
                Folder::new(&default_account.id, "INBOX", "Inbox", "INBOX").with_role(FolderRole::Inbox),
                Folder::new(&default_account.id, "Drafts", "Drafts", "Drafts").with_role(FolderRole::Drafts),
                Folder::new(&default_account.id, "Sent", "Sent", "Sent").with_role(FolderRole::Sent),
                Folder::new(&default_account.id, "Archive", "Archive", "Archive").with_role(FolderRole::Archive),
                Folder::new(&default_account.id, "Junk", "Junk", "Junk").with_role(FolderRole::Junk),
                Folder::new(&default_account.id, "Trash", "Trash", "Trash").with_role(FolderRole::Trash),
            ];

            let now = chrono::Utc::now();
            let messages = vec![
                MessageSummary {
                    id: "msg-welcome".into(),
                    thread_id: Some("th-1".into()),
                    subject: Some("Welcome to Vespetrel — Pure Rust Desktop Mail".into()),
                    from_address: "team@vespetrel.example".into(),
                    from_name: Some("Vespetrel Core Team".into()),
                    snippet: Some("Your GPU-accelerated, high-performance desktop mail client is now ready with IMAP, JMAP, CalDAV, and OpenPGP encryption.".into()),
                    sent_at: now,
                    is_read: false,
                    is_flagged: true,
                    has_attachments: true,
                },
                MessageSummary {
                    id: "msg-security".into(),
                    thread_id: Some("th-2".into()),
                    subject: Some("End-to-End Encryption & Security Audit".into()),
                    from_address: "security@vespetrel.example".into(),
                    from_name: Some("Security Team".into()),
                    snippet: Some("Autocrypt 1.1 keys negotiated. Remote tracking pixels and external trackers have been automatically stripped.".into()),
                    sent_at: now - chrono::Duration::hours(2),
                    is_read: true,
                    is_flagged: false,
                    has_attachments: false,
                },
                MessageSummary {
                    id: "msg-release".into(),
                    thread_id: Some("th-3".into()),
                    subject: Some("Release Announcement: 120 FPS Rendering & Instant FTS5 Search".into()),
                    from_address: "updates@vespetrel.example".into(),
                    from_name: Some("Vespetrel Releases".into()),
                    snippet: Some("Sub-15ms search across 200,000+ emails using SQLite WAL + FTS5 full-text indexing.".into()),
                    sent_at: now - chrono::Duration::days(1),
                    is_read: true,
                    is_flagged: true,
                    has_attachments: false,
                },
            ];

            let calendar_events = vec![
                CalendarEvent {
                    id: "ev-1".into(),
                    calendar_id: "cal-main".into(),
                    title: "Weekly Engineering Standup".into(),
                    description: Some("Architecture sync and release review".into()),
                    start: now + chrono::Duration::hours(3),
                    end: now + chrono::Duration::hours(4),
                    location: Some("Virtual Conference".into()),
                    ical_uid: Some("uid-101".into()),
                    raw_ical: None,
                },
                CalendarEvent {
                    id: "ev-2".into(),
                    calendar_id: "cal-main".into(),
                    title: "Security & Crypto Review".into(),
                    description: Some("OpenPGP and S/MIME certificate chain verification".into()),
                    start: now + chrono::Duration::days(1),
                    end: now + chrono::Duration::days(1) + chrono::Duration::hours(1),
                    location: Some("Security Room".into()),
                    ical_uid: Some("uid-102".into()),
                    raw_ical: None,
                },
            ];

            let contacts = vec![
                Contact {
                    id: "c-1".into(),
                    remote_id: None,
                    display_name: Some("Alice Vance".into()),
                    email: "alice.vance@vespetrel.example".into(),
                    vcard_data: None,
                },
                Contact {
                    id: "c-2".into(),
                    remote_id: None,
                    display_name: Some("Bob Martinez".into()),
                    email: "bob.martinez@vespetrel.example".into(),
                    vcard_data: None,
                },
                Contact {
                    id: "c-3".into(),
                    remote_id: None,
                    display_name: Some("Carol King".into()),
                    email: "carol.king@vespetrel.example".into(),
                    vcard_data: None,
                },
            ];

            let tasks = vec![
                TaskItem::new("cal-main", "Review IMAP IDLE connection heartbeat parameters"),
                TaskItem::new("cal-main", "Configure S/MIME corporate trust anchors"),
                TaskItem::new("cal-main", "Export backup address book via CardDAV vCard 4.0"),
            ];

            // Spawn Tokio Bridge Listener bound to GPUI Context
            let bridge_rx = sync_rx;
            cx.spawn(async move |this, cx| {
                while let Ok(event) = bridge_rx.recv_async().await {
                    let _ = this.update(cx, |view, cx| {
                        view.handle_sync_event(event, cx);
                    });
                }
            }).detach();

            Self {
                active_tab: ActiveViewTab::Mail,
                active_modal: ActiveModal::None,
                accounts: vec![default_account],
                folders,
                selected_folder_id: Some("INBOX".into()),
                messages,
                selected_message_id: Some("msg-welcome".into()),
                list_filter: ListFilter::All,
                search_query: String::new(),
                block_remote_images: true,
                calendar_events,
                contacts,
                tasks,
                settings: UserSettings::default(),
                compose_to: "team@vespetrel.example".into(),
                compose_subject: "Hello from Pure Rust GPUI Mail".into(),
                compose_body: "Hi team,\n\nWriting this from the pure Rust GPUI mail client.\n\nBest regards,\nVespetrel User".into(),
                palette_query: String::new(),
                sync_sender: sync_tx,
                status_message: "All mailboxes synchronized".into(),
            }
        }

        pub fn handle_sync_event(&mut self, event: SyncEvent, cx: &mut Context<Self>) {
            match event {
                SyncEvent::MessagesInserted(new_msgs) => {
                    self.status_message = format!("Received {} new message(s)", new_msgs.len());
                    self.messages.splice(0..0, new_msgs);
                    cx.notify();
                }
                SyncEvent::MessageFlagsUpdated { id, is_read, is_flagged } => {
                    if let Some(m) = self.messages.iter_mut().find(|m| m.id == id) {
                        m.is_read = is_read;
                        m.is_flagged = is_flagged;
                        cx.notify();
                    }
                }
                SyncEvent::MessagesDeleted(ids) => {
                    self.messages.retain(|m| !ids.contains(&m.id));
                    if let Some(sel) = &self.selected_message_id {
                        if ids.contains(sel) {
                            self.selected_message_id = self.messages.first().map(|m| m.id.clone());
                        }
                    }
                    cx.notify();
                }
                SyncEvent::SyncError { folder, error } => {
                    self.status_message = format!("⚠️ Sync error ({folder}): {error}");
                    cx.notify();
                }
                SyncEvent::SyncFinished { account_id: _ } => {
                    self.status_message = "All mailboxes synchronized".into();
                    cx.notify();
                }
                SyncEvent::FolderListUpdated(_) => {
                    self.status_message = "Folders updated".into();
                    cx.notify();
                }
            }
        }

        pub fn selected_message(&self) -> Option<&MessageSummary> {
            self.selected_message_id
                .as_ref()
                .and_then(|id| self.messages.iter().find(|m| &m.id == id))
                .or_else(|| self.messages.first())
        }

        pub fn filtered_messages(&self) -> Vec<&MessageSummary> {
            let q = self.search_query.trim();
            self.messages
                .iter()
                .filter(|m| {
                    let flag_match = match self.list_filter {
                        ListFilter::All => true,
                        ListFilter::Unread => !m.is_read,
                        ListFilter::Flagged => m.is_flagged,
                        ListFilter::WithAttachments => m.has_attachments,
                    };
                    if !flag_match {
                        return false;
                    }
                    if q.is_empty() {
                        return true;
                    }
                    m.subject.as_deref().is_some_and(|s| contains_ignore_case(s, q))
                        || contains_ignore_case(&m.from_address, q)
                        || m.from_name.as_deref().is_some_and(|n| contains_ignore_case(n, q))
                        || m.snippet.as_deref().is_some_and(|sn| contains_ignore_case(sn, q))
                })
                .collect()
        }

        pub fn trigger_sync(&mut self, cx: &mut Context<Self>) {
            self.status_message = "Syncing mailboxes...".into();
            let tx = self.sync_sender.clone();
            cx.spawn(async move |this, cx| {
                tokio::time::sleep(std::time::Duration::from_millis(400)).await;
                let _ = tx.send(SyncEvent::FolderListUpdated(vec![]));
                let _ = this.update(cx, |view, cx| {
                    view.status_message = "Mailboxes up to date".into();
                    cx.notify();
                });
            }).detach();
            cx.notify();
        }

        pub fn toggle_flag(&mut self, id: String, cx: &mut Context<Self>) {
            if let Some(m) = self.messages.iter_mut().find(|m| m.id == id) {
                m.is_flagged = !m.is_flagged;
                cx.notify();
            }
        }

        pub fn delete_selected_message(&mut self, cx: &mut Context<Self>) {
            if let Some(id) = self.selected_message_id.clone() {
                self.messages.retain(|m| m.id != id);
                self.selected_message_id = self.messages.first().map(|m| m.id.clone());
                self.status_message = "Message moved to Trash".into();
                cx.notify();
            }
        }

        pub fn archive_selected_message(&mut self, cx: &mut Context<Self>) {
            if let Some(id) = self.selected_message_id.clone() {
                self.messages.retain(|m| m.id != id);
                self.selected_message_id = self.messages.first().map(|m| m.id.clone());
                self.status_message = "Message archived".into();
                cx.notify();
            }
        }

        pub fn send_composed_message(&mut self, cx: &mut Context<Self>) {
            if self.compose_to.trim().is_empty() {
                self.status_message = "Error: Please specify a recipient".into();
                cx.notify();
                return;
            }

            let new_msg = MessageSummary {
                id: format!("msg-sent-{}", uuid::Uuid::new_v4()),
                thread_id: None,
                subject: Some(self.compose_subject.clone()),
                from_address: "user@vespetrel.example".into(),
                from_name: Some("Me".into()),
                snippet: Some(self.compose_body.chars().take(120).collect()),
                sent_at: chrono::Utc::now(),
                is_read: true,
                is_flagged: false,
                has_attachments: false,
            };

            self.messages.insert(0, new_msg);
            self.status_message = format!("Message sent to {}", self.compose_to);
            self.active_modal = ActiveModal::None;
            self.compose_to.clear();
            self.compose_subject.clear();
            self.compose_body.clear();
            cx.notify();
        }
    }

    fn contains_ignore_case(haystack: &str, needle: &str) -> bool {
        if needle.is_empty() {
            return true;
        }
        if haystack.len() < needle.len() {
            return false;
        }
        if haystack.is_ascii() && needle.is_ascii() {
            let n_bytes = needle.as_bytes();
            let h_bytes = haystack.as_bytes();
            return h_bytes
                .windows(n_bytes.len())
                .any(|window| window.eq_ignore_ascii_case(n_bytes));
        }
        haystack
            .to_lowercase()
            .contains(&needle.to_lowercase())
    }

    impl Render for MainWindow {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            div()
                .flex()
                .flex_col()
                .size_full()
                .bg(rgb(0x0f1117))
                .text_color(rgb(0xe2e8f0))
                .child(self.render_header())
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .flex_1()
                        .overflow_hidden()
                        .child(self.render_sidebar_tabs())
                        .child(self.render_active_tab_content())
                )
                .child(self.render_status_bar())
                .child(self.render_modal_layer())
        }
    }

    impl MainWindow {
        fn render_header(&self) -> impl IntoElement {
            let search_display = if self.search_query.is_empty() {
                "Search messages, senders, attachments (FTS5)...".to_string()
            } else {
                self.search_query.clone()
            };

            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .h(px(48.0))
                .px(px(16.0))
                .bg(rgb(0x131722))
                .border_b_1()
                .border_color(rgb(0x1f293d))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(10.0))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .justify_center()
                                .w(px(28.0))
                                .h(px(28.0))
                                .rounded_md()
                                .bg(rgb(0x3b82f6))
                                .text_color(rgb(0xffffff))
                                .font_weight(FontWeight::BOLD)
                                .text_sm()
                                .child("V")
                        )
                        .child(
                            div()
                                .font_weight(FontWeight::BOLD)
                                .text_sm()
                                .text_color(rgb(0xf8fafc))
                                .child("Vespetrel Mail")
                        )
                )
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .w(px(420.0))
                        .h(px(32.0))
                        .px(px(12.0))
                        .rounded_md()
                        .bg(rgb(0x1a202e))
                        .border_1()
                        .border_color(rgb(0x2d3748))
                        .gap(px(8.0))
                        .child(div().text_xs().text_color(rgb(0x94a3b8)).child("🔍"))
                        .child(
                            div()
                                .text_xs()
                                .text_color(if self.search_query.is_empty() { rgb(0x64748b) } else { rgb(0xe2e8f0) })
                                .child(search_display)
                        )
                )
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(8.0))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(6.0))
                                .px(px(12.0))
                                .py(px(6.0))
                                .rounded_md()
                                .bg(rgb(0x2563eb))
                                .text_color(rgb(0xffffff))
                                .text_xs()
                                .font_weight(FontWeight::SEMIBOLD)
                                .cursor_pointer()
                                .child("✍️ Compose")
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .px(px(10.0))
                                .py(px(6.0))
                                .rounded_md()
                                .bg(rgb(0x1e293b))
                                .text_color(rgb(0xcbd5e1))
                                .text_xs()
                                .cursor_pointer()
                                .child("🔄 Sync")
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .px(px(10.0))
                                .py(px(6.0))
                                .rounded_md()
                                .bg(rgb(0x1e293b))
                                .text_color(rgb(0x94a3b8))
                                .text_xs()
                                .cursor_pointer()
                                .child("⌘K")
                        )
                )
        }

        fn render_sidebar_tabs(&self) -> impl IntoElement {
            let tabs = [
                (ActiveViewTab::Mail, "✉️", "Mail"),
                (ActiveViewTab::Calendar, "📅", "Calendar"),
                (ActiveViewTab::Contacts, "👥", "Contacts"),
                (ActiveViewTab::Tasks, "✅", "Tasks"),
                (ActiveViewTab::Settings, "⚙️", "Settings"),
            ];

            div()
                .flex()
                .flex_col()
                .w(px(56.0))
                .h_full()
                .bg(rgb(0x11141c))
                .border_r_1()
                .border_color(rgb(0x1f293d))
                .items_center()
                .py(px(12.0))
                .gap(px(8.0))
                .children(tabs.into_iter().map(|(tab, icon, label)| {
                    let is_active = self.active_tab == tab;
                    div()
                        .flex()
                        .flex_col()
                        .items_center()
                        .justify_center()
                        .w(px(44.0))
                        .h(px(44.0))
                        .rounded_lg()
                        .bg(if is_active { rgb(0x1e293b) } else { rgb(0x11141c) })
                        .text_color(if is_active { rgb(0x60a5fa) } else { rgb(0x64748b) })
                        .border_1()
                        .border_color(if is_active { rgb(0x3b82f6) } else { rgb(0x00000000) })
                        .cursor_pointer()
                        .child(div().text_base().child(icon))
                        .child(div().text_xs().child(label))
                }))
        }

        fn render_active_tab_content(&self) -> impl IntoElement {
            div()
                .flex()
                .flex_row()
                .flex_1()
                .h_full()
                .overflow_hidden()
                .child(match self.active_tab {
                    ActiveViewTab::Mail => self.render_mail_workspace().into_any_element(),
                    ActiveViewTab::Calendar => self.render_calendar_view().into_any_element(),
                    ActiveViewTab::Contacts => self.render_contacts_view().into_any_element(),
                    ActiveViewTab::Tasks => self.render_tasks_view().into_any_element(),
                    ActiveViewTab::Settings => self.render_settings_view().into_any_element(),
                })
        }

        fn render_mail_workspace(&self) -> Div {
            div()
                .flex()
                .flex_row()
                .flex_1()
                .h_full()
                .overflow_hidden()
                .child(self.render_folder_tree())
                .child(self.render_message_list_pane())
                .child(self.render_message_reader_pane())
        }

        fn render_folder_tree(&self) -> impl IntoElement {
            div()
                .flex()
                .flex_col()
                .w(px(220.0))
                .h_full()
                .bg(rgb(0x131722))
                .border_r_1()
                .border_color(rgb(0x1f293d))
                .p(px(12.0))
                .gap(px(14.0))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .child(div().text_xs().font_weight(FontWeight::BOLD).text_color(rgb(0x94a3b8)).child("ACCOUNTS & FOLDERS"))
                        .child(
                            div()
                                .text_xs()
                                .text_color(rgb(0x60a5fa))
                                .cursor_pointer()
                                .child("+ Add")
                        )
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .p(px(8.0))
                        .rounded_md()
                        .bg(rgb(0x181f2f))
                        .child(div().text_xs().font_weight(FontWeight::SEMIBOLD).text_color(rgb(0xf8fafc)).child("user@vespetrel.example"))
                        .child(div().text_xs().text_color(rgb(0x38bdf8)).child("IMAP / SMTP • XOAUTH2 Ready"))
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(4.0))
                        .children(self.folders.iter().map(|f| {
                            let is_selected = self.selected_folder_id.as_deref() == Some(&f.remote_id);
                            let icon = match f.role {
                                FolderRole::Inbox => "📥",
                                FolderRole::Drafts => "📝",
                                FolderRole::Sent => "📤",
                                FolderRole::Archive => "📦",
                                FolderRole::Junk => "🚫",
                                FolderRole::Trash => "🗑️",
                                FolderRole::Custom => "📁",
                            };
                            let count = if f.role == FolderRole::Inbox { self.messages.len() } else { 0 };

                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .justify_between()
                                .px(px(10.0))
                                .py(px(6.0))
                                .rounded_md()
                                .bg(if is_selected { rgb(0x1e293b) } else { rgb(0x00000000) })
                                .text_color(if is_selected { rgb(0x60a5fa) } else { rgb(0xcbd5e1) })
                                .cursor_pointer()
                                .child(
                                    div()
                                        .flex()
                                        .flex_row()
                                        .items_center()
                                        .gap(px(8.0))
                                        .child(div().text_xs().child(icon))
                                        .child(div().text_xs().font_weight(if is_selected { FontWeight::BOLD } else { FontWeight::NORMAL }).child(f.name.clone()))
                                )
                                .child(
                                    if count > 0 {
                                        div()
                                            .px(px(6.0))
                                            .py(px(1.0))
                                            .rounded_full()
                                            .bg(rgb(0x2563eb))
                                            .text_color(rgb(0xffffff))
                                            .text_xs()
                                            .child(format!("{count}"))
                                    } else {
                                        div()
                                    }
                                )
                        }))
                )
        }

        fn render_message_list_pane(&self) -> impl IntoElement {
            let filtered = self.filtered_messages();

            div()
                .flex()
                .flex_col()
                .w(px(350.0))
                .h_full()
                .bg(rgb(0x10141d))
                .border_r_1()
                .border_color(rgb(0x1f293d))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .p(px(8.0))
                        .gap(px(4.0))
                        .border_b_1()
                        .border_color(rgb(0x1f293d))
                        .child(self.render_filter_chip("All", ListFilter::All))
                        .child(self.render_filter_chip("Unread", ListFilter::Unread))
                        .child(self.render_filter_chip("Starred", ListFilter::Flagged))
                        .child(self.render_filter_chip("📎 Files", ListFilter::WithAttachments))
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .children(filtered.into_iter().map(|msg| {
                            let is_selected = self.selected_message_id.as_deref() == Some(&msg.id);
                            let sender = msg.from_name.as_deref().unwrap_or(&msg.from_address);
                            let subject = msg.subject.as_deref().unwrap_or("(No Subject)");
                            let snippet = msg.snippet.as_deref().unwrap_or("");
                            let date_str = msg.sent_at.format("%b %d, %H:%M").to_string();

                            div()
                                .flex()
                                .flex_col()
                                .p(px(10.0))
                                .border_b_1()
                                .border_color(rgb(0x182030))
                                .bg(if is_selected { rgb(0x1e2a42) } else if !msg.is_read { rgb(0x141a29) } else { rgb(0x10141d) })
                                .cursor_pointer()
                                .child(
                                    div()
                                        .flex()
                                        .flex_row()
                                        .items_center()
                                        .justify_between()
                                        .child(
                                            div()
                                                .flex()
                                                .flex_row()
                                                .items_center()
                                                .gap(px(6.0))
                                                .child(
                                                    if !msg.is_read {
                                                        div().w(px(6.0)).h(px(6.0)).rounded_full().bg(rgb(0x3b82f6))
                                                    } else {
                                                        div().w(px(6.0)).h(px(6.0))
                                                    }
                                                )
                                                .child(
                                                    div()
                                                        .text_xs()
                                                        .font_weight(if !msg.is_read { FontWeight::BOLD } else { FontWeight::MEDIUM })
                                                        .text_color(rgb(0xf1f5f9))
                                                        .child(sender.to_string())
                                                )
                                        )
                                        .child(
                                            div()
                                                .flex()
                                                .flex_row()
                                                .items_center()
                                                .gap(px(6.0))
                                                .child(
                                                    div()
                                                        .text_xs()
                                                        .text_color(if msg.is_flagged { rgb(0xfbbf24) } else { rgb(0x475569) })
                                                        .child(if msg.is_flagged { "★" } else { "☆" })
                                                )
                                                .child(
                                                    div()
                                                        .text_xs()
                                                        .text_color(rgb(0x64748b))
                                                        .child(date_str)
                                                )
                                        )
                                )
                                .child(
                                    div()
                                        .pt(px(2.0))
                                        .text_xs()
                                        .font_weight(if !msg.is_read { FontWeight::SEMIBOLD } else { FontWeight::NORMAL })
                                        .text_color(rgb(0xe2e8f0))
                                        .child(subject.to_string())
                                )
                                .child(
                                    div()
                                        .pt(px(2.0))
                                        .text_xs()
                                        .text_color(rgb(0x94a3b8))
                                        .child(if snippet.len() > 60 { format!("{}...", &snippet[..60]) } else { snippet.to_string() })
                                )
                        }))
                )
        }

        fn render_filter_chip(&self, label: &'static str, filter: ListFilter) -> impl IntoElement {
            let is_active = self.list_filter == filter;
            div()
                .px(px(8.0))
                .py(px(4.0))
                .rounded_md()
                .bg(if is_active { rgb(0x2563eb) } else { rgb(0x1a2233) })
                .text_color(if is_active { rgb(0xffffff) } else { rgb(0x94a3b8) })
                .text_xs()
                .cursor_pointer()
                .child(label)
        }

        fn render_message_reader_pane(&self) -> impl IntoElement {
            let msg_opt = self.selected_message();

            div()
                .flex()
                .flex_col()
                .flex_1()
                .h_full()
                .bg(rgb(0x0d1117))
                .child(if let Some(msg) = msg_opt {
                    let from_name = msg.from_name.as_deref().unwrap_or("");
                    let from_addr = &msg.from_address;
                    let subject = msg.subject.as_deref().unwrap_or("(No Subject)");
                    let date_full = msg.sent_at.to_rfc2822();
                    let body = msg.snippet.as_deref().unwrap_or("No content available.");

                    div()
                        .flex()
                        .flex_col()
                        .size_full()
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .justify_between()
                                .p(px(12.0))
                                .border_b_1()
                                .border_color(rgb(0x1f293d))
                                .bg(rgb(0x131722))
                                .child(
                                    div()
                                        .flex()
                                        .flex_row()
                                        .items_center()
                                        .gap(px(8.0))
                                        .child(div().px(px(10.0)).py(px(5.0)).rounded_md().bg(rgb(0x2563eb)).text_color(rgb(0xffffff)).text_xs().cursor_pointer().child("↩ Reply"))
                                        .child(div().px(px(10.0)).py(px(5.0)).rounded_md().bg(rgb(0x1e293b)).text_color(rgb(0xcbd5e1)).text_xs().cursor_pointer().child("📦 Archive"))
                                        .child(div().px(px(10.0)).py(px(5.0)).rounded_md().bg(rgb(0x1e293b)).text_color(rgb(0xf87171)).text_xs().cursor_pointer().child("🗑️ Delete"))
                                )
                                .child(
                                    div()
                                        .flex()
                                        .flex_row()
                                        .items_center()
                                        .gap(px(6.0))
                                        .px(px(8.0))
                                        .py(px(4.0))
                                        .rounded_md()
                                        .bg(rgb(0x064e3b))
                                        .border_1()
                                        .border_color(rgb(0x059669))
                                        .text_color(rgb(0x34d399))
                                        .text_xs()
                                        .child("🔒 OpenPGP Signed & Encrypted ✓")
                                )
                        )
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .p(px(16.0))
                                .border_b_1()
                                .border_color(rgb(0x1f293d))
                                .gap(px(10.0))
                                .child(
                                    div()
                                        .text_lg()
                                        .font_weight(FontWeight::BOLD)
                                        .text_color(rgb(0xf8fafc))
                                        .child(subject.to_string())
                                )
                                .child(
                                    div()
                                        .flex()
                                        .flex_row()
                                        .items_center()
                                        .justify_between()
                                        .child(
                                            div()
                                                .flex()
                                                .flex_row()
                                                .items_center()
                                                .gap(px(10.0))
                                                .child(
                                                    div()
                                                        .flex()
                                                        .items_center()
                                                        .justify_center()
                                                        .w(px(36.0))
                                                        .h(px(36.0))
                                                        .rounded_full()
                                                        .bg(rgb(0x3b82f6))
                                                        .text_color(rgb(0xffffff))
                                                        .font_weight(FontWeight::BOLD)
                                                        .text_sm()
                                                        .child(from_name.chars().next().unwrap_or('U').to_string())
                                                )
                                                .child(
                                                    div()
                                                        .flex()
                                                        .flex_col()
                                                        .child(div().text_xs().font_weight(FontWeight::BOLD).text_color(rgb(0xf1f5f9)).child(if from_name.is_empty() { from_addr.clone() } else { format!("{from_name} <{from_addr}>") }))
                                                        .child(div().text_xs().text_color(rgb(0x94a3b8)).child("To: user@vespetrel.example"))
                                                )
                                        )
                                        .child(div().text_xs().text_color(rgb(0x64748b)).child(date_full))
                                )
                        )
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .justify_between()
                                .px(px(16.0))
                                .py(px(8.0))
                                .bg(rgb(0x181e2b))
                                .border_b_1()
                                .border_color(rgb(0x1f293d))
                                .child(div().text_xs().text_color(rgb(0x94a3b8)).child("🛡️ Remote trackers and pixel beacons blocked by ammonia + lol_html sanitizer."))
                                .child(div().text_xs().text_color(rgb(0x60a5fa)).cursor_pointer().child(if self.block_remote_images { "Load Remote Images" } else { "Block Images" }))
                        )
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .flex_1()
                                .p(px(20.0))
                                .gap(px(12.0))
                                .child(div().text_sm().text_color(rgb(0xe2e8f0)).child(body.to_string()))
                                .child(div().pt(px(16.0)).text_xs().text_color(rgb(0x64748b)).child("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"))
                                .child(div().text_xs().text_color(rgb(0x94a3b8)).child("Rendered natively with Pure Rust GPUI Engine. Full-text search and offline storage powered by SQLite WAL + FTS5."))
                        )
                } else {
                    div()
                        .flex()
                        .items_center()
                        .justify_center()
                        .size_full()
                        .child(div().text_sm().text_color(rgb(0x64748b)).child("Select a message to view its contents"))
                })
        }

        fn render_calendar_view(&self) -> Div {
            div()
                .flex()
                .flex_col()
                .flex_1()
                .p(px(20.0))
                .bg(rgb(0x0f1117))
                .gap(px(16.0))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .child(div().text_lg().font_weight(FontWeight::BOLD).text_color(rgb(0xf8fafc)).child("📅 Calendar (CalDAV RFC 4791 & iCalendar RFC 5545)"))
                        .child(div().px(px(12.0)).py(px(6.0)).rounded_md().bg(rgb(0x2563eb)).text_color(rgb(0xffffff)).text_xs().child("+ New Event"))
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(10.0))
                        .children(self.calendar_events.iter().map(|ev| {
                            div()
                                .flex()
                                .flex_col()
                                .p(px(12.0))
                                .rounded_lg()
                                .bg(rgb(0x171c2a))
                                .border_1()
                                .border_color(rgb(0x232c40))
                                .child(div().text_sm().font_weight(FontWeight::BOLD).text_color(rgb(0x60a5fa)).child(ev.title.clone()))
                                .child(div().text_xs().text_color(rgb(0x94a3b8)).child(format!("Time: {} - {}", ev.start.format("%Y-%m-%d %H:%M"), ev.end.format("%H:%M"))))
                                .child(div().text_xs().text_color(rgb(0xcbd5e1)).child(ev.description.clone().unwrap_or_default()))
                        }))
                )
        }

        fn render_contacts_view(&self) -> Div {
            div()
                .flex()
                .flex_col()
                .flex_1()
                .p(px(20.0))
                .bg(rgb(0x0f1117))
                .gap(px(16.0))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .child(div().text_lg().font_weight(FontWeight::BOLD).text_color(rgb(0xf8fafc)).child("👥 Address Book (CardDAV & vCard 4.0)"))
                        .child(div().px(px(12.0)).py(px(6.0)).rounded_md().bg(rgb(0x2563eb)).text_color(rgb(0xffffff)).text_xs().child("+ Add Contact"))
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(8.0))
                        .children(self.contacts.iter().map(|c| {
                            let name = c.display_name.as_deref().unwrap_or("Contact");
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .justify_between()
                                .p(px(12.0))
                                .rounded_lg()
                                .bg(rgb(0x171c2a))
                                .border_1()
                                .border_color(rgb(0x232c40))
                                .child(
                                    div()
                                        .flex()
                                        .flex_row()
                                        .items_center()
                                        .gap(px(12.0))
                                        .child(
                                            div()
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .w(px(32.0))
                                                .h(px(32.0))
                                                .rounded_full()
                                                .bg(rgb(0x3b82f6))
                                                .text_color(rgb(0xffffff))
                                                .font_weight(FontWeight::BOLD)
                                                .text_xs()
                                                .child(name.chars().next().unwrap_or('C').to_string())
                                        )
                                        .child(
                                            div()
                                                .flex()
                                                .flex_col()
                                                .child(div().text_xs().font_weight(FontWeight::BOLD).text_color(rgb(0xf8fafc)).child(name.to_string()))
                                                .child(div().text_xs().text_color(rgb(0x94a3b8)).child(c.email.clone()))
                                        )
                                )
                                .child(div().px(px(10.0)).py(px(4.0)).rounded_md().bg(rgb(0x1e293b)).text_color(rgb(0x60a5fa)).text_xs().child("Write Email"))
                        }))
                )
        }

        fn render_tasks_view(&self) -> Div {
            div()
                .flex()
                .flex_col()
                .flex_1()
                .p(px(20.0))
                .bg(rgb(0x0f1117))
                .gap(px(16.0))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .child(div().text_lg().font_weight(FontWeight::BOLD).text_color(rgb(0xf8fafc)).child("✅ Tasks (RFC 5545 VTODO & CalDAV Tasks)"))
                        .child(div().px(px(12.0)).py(px(6.0)).rounded_md().bg(rgb(0x2563eb)).text_color(rgb(0xffffff)).text_xs().child("+ Add Task"))
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(8.0))
                        .children(self.tasks.iter().map(|t| {
                            let is_done = t.is_completed;
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .justify_between()
                                .p(px(12.0))
                                .rounded_lg()
                                .bg(rgb(0x171c2a))
                                .border_1()
                                .border_color(rgb(0x232c40))
                                .child(
                                    div()
                                        .flex()
                                        .flex_row()
                                        .items_center()
                                        .gap(px(10.0))
                                        .child(div().text_sm().child(if is_done { "☑️" } else { "⬜" }))
                                        .child(
                                            div()
                                                .text_xs()
                                                .text_color(if is_done { rgb(0x64748b) } else { rgb(0xf1f5f9) })
                                                .child(t.title.clone())
                                        )
                                )
                        }))
                )
        }

        fn render_settings_view(&self) -> Div {
            div()
                .flex()
                .flex_col()
                .flex_1()
                .p(px(20.0))
                .bg(rgb(0x0f1117))
                .gap(px(16.0))
                .child(div().text_lg().font_weight(FontWeight::BOLD).text_color(rgb(0xf8fafc)).child("⚙️ Configuration & Preferences"))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .p(px(16.0))
                        .rounded_lg()
                        .bg(rgb(0x171c2a))
                        .border_1()
                        .border_color(rgb(0x232c40))
                        .gap(px(10.0))
                        .child(div().text_sm().font_weight(FontWeight::BOLD).text_color(rgb(0x60a5fa)).child("Storage & Database Engine"))
                        .child(div().text_xs().text_color(rgb(0xcbd5e1)).child("• Database: SQLite 3 with WAL Mode and Memory-Mapped I/O (256MB)"))
                        .child(div().text_xs().text_color(rgb(0xcbd5e1)).child("• Full-Text Search: SQLite FTS5 (unicode61 tokenizer, BM25 ranking)"))
                        .child(div().text_xs().text_color(rgb(0xcbd5e1)).child("• Blob Compression: lz4_flex + zstd"))
                        .child(div().text_xs().text_color(rgb(0xcbd5e1)).child("• Keyring Credentials: Native OS Keyring / Credential Manager"))
                        .child(div().text_xs().text_color(rgb(0xcbd5e1)).child("• Crypto & Security: rPGP OpenPGP RFC 9580 + RustCrypto CMS S/MIME"))
                )
        }

        fn render_status_bar(&self) -> impl IntoElement {
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .h(px(24.0))
                .px(px(16.0))
                .bg(rgb(0x0c0e14))
                .border_t_1()
                .border_color(rgb(0x1a202c))
                .text_xs()
                .text_color(rgb(0x64748b))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(6.0))
                        .child(div().w(px(6.0)).h(px(6.0)).rounded_full().bg(rgb(0x10b981)))
                        .child(div().text_color(rgb(0x94a3b8)).child(self.status_message.clone()))
                )
                .child(div().child("120 FPS GPU Direct3D / Vulkan • Rust Edition 2024"))
        }

        fn render_modal_layer(&self) -> impl IntoElement {
            match self.active_modal {
                ActiveModal::None => div(),
                ActiveModal::Compose => self.render_compose_modal(),
                ActiveModal::CommandPalette => self.render_command_palette_modal(),
                ActiveModal::AddAccount => self.render_add_account_modal(),
            }
        }

        fn render_compose_modal(&self) -> Div {
            let to_text = if self.compose_to.is_empty() { "user@example.com".to_string() } else { self.compose_to.clone() };
            let subj_text = if self.compose_subject.is_empty() { "Message subject...".to_string() } else { self.compose_subject.clone() };
            let body_text = if self.compose_body.is_empty() { "Compose email body (Markdown / HTML enabled)...".to_string() } else { self.compose_body.clone() };

            div()
                .flex()
                .items_center()
                .justify_center()
                .size_full()
                .bg(rgb(0x000000bb))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .w(px(620.0))
                        .h(px(480.0))
                        .rounded_xl()
                        .bg(rgb(0x161b26))
                        .border_1()
                        .border_color(rgb(0x2d3748))
                        .p(px(20.0))
                        .gap(px(12.0))
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .justify_between()
                                .child(div().text_base().font_weight(FontWeight::BOLD).text_color(rgb(0xf8fafc)).child("✍️ New Message Composer"))
                                .child(div().text_sm().text_color(rgb(0x94a3b8)).cursor_pointer().child("✕"))
                        )
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap(px(8.0))
                                .p(px(8.0))
                                .rounded_md()
                                .bg(rgb(0x1c2333))
                                .child(div().text_xs().text_color(rgb(0x94a3b8)).child("To:"))
                                .child(div().text_xs().text_color(rgb(0xf1f5f9)).child(to_text))
                        )
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap(px(8.0))
                                .p(px(8.0))
                                .rounded_md()
                                .bg(rgb(0x1c2333))
                                .child(div().text_xs().text_color(rgb(0x94a3b8)).child("Subject:"))
                                .child(div().text_xs().text_color(rgb(0xf1f5f9)).child(subj_text))
                        )
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .flex_1()
                                .p(px(10.0))
                                .rounded_md()
                                .bg(rgb(0x111622))
                                .border_1()
                                .border_color(rgb(0x232c40))
                                .child(div().text_xs().text_color(rgb(0xe2e8f0)).child(body_text))
                        )
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .justify_between()
                                .child(
                                    div()
                                        .flex()
                                        .flex_row()
                                        .items_center()
                                        .gap(px(6.0))
                                        .child(div().text_xs().child("🔒"))
                                        .child(div().text_xs().text_color(rgb(0x34d399)).child("Autocrypt OpenPGP Signature Attached"))
                                )
                                .child(
                                    div()
                                        .flex()
                                        .flex_row()
                                        .items_center()
                                        .gap(px(8.0))
                                        .child(div().px(px(14.0)).py(px(6.0)).rounded_md().bg(rgb(0x1e293b)).text_color(rgb(0xcbd5e1)).text_xs().cursor_pointer().child("Discard"))
                                        .child(div().px(px(16.0)).py(px(6.0)).rounded_md().bg(rgb(0x2563eb)).text_color(rgb(0xffffff)).text_xs().font_weight(FontWeight::BOLD).cursor_pointer().child("Send 🚀"))
                                )
                        )
                )
        }

        fn render_command_palette_modal(&self) -> Div {
            let actions = [
                ("✍️ Compose New Email", "c"),
                ("📥 Go to Inbox", "g i"),
                ("★ Go to Starred", "g s"),
                ("🔄 Sync All Mailboxes", "Ctrl+R"),
                ("📅 Switch to Calendar View", "Alt+2"),
                ("👥 Switch to Contacts View", "Alt+3"),
                ("✅ Switch to Tasks View", "Alt+4"),
                ("⚙️ Open Settings", "Ctrl+,"),
            ];

            div()
                .flex()
                .items_center()
                .justify_center()
                .size_full()
                .bg(rgb(0x000000bb))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .w(px(520.0))
                        .rounded_xl()
                        .bg(rgb(0x161b26))
                        .border_1()
                        .border_color(rgb(0x2d3748))
                        .p(px(16.0))
                        .gap(px(10.0))
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .p(px(10.0))
                                .rounded_md()
                                .bg(rgb(0x111622))
                                .border_1()
                                .border_color(rgb(0x232c40))
                                .gap(px(8.0))
                                .child(div().text_sm().child("⌘"))
                                .child(div().text_xs().text_color(rgb(0x94a3b8)).child("Type a command or search action..."))
                        )
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(4.0))
                                .children(actions.into_iter().enumerate().map(|(idx, (title, shortcut))| {
                                    div()
                                        .flex()
                                        .flex_row()
                                        .items_center()
                                        .justify_between()
                                        .px(px(12.0))
                                        .py(px(8.0))
                                        .rounded_md()
                                        .bg(if idx == 0 { rgb(0x1e293b) } else { rgb(0x00000000) })
                                        .cursor_pointer()
                                        .child(div().text_xs().text_color(rgb(0xf1f5f9)).child(title))
                                        .child(div().px(px(6.0)).py(px(2.0)).rounded_md().bg(rgb(0x111622)).text_xs().text_color(rgb(0x94a3b8)).child(shortcut))
                                }))
                        )
                )
        }

        fn render_add_account_modal(&self) -> Div {
            div()
                .flex()
                .items_center()
                .justify_center()
                .size_full()
                .bg(rgb(0x000000bb))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .w(px(480.0))
                        .rounded_xl()
                        .bg(rgb(0x161b26))
                        .border_1()
                        .border_color(rgb(0x2d3748))
                        .p(px(20.0))
                        .gap(px(12.0))
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .justify_between()
                                .child(div().text_base().font_weight(FontWeight::BOLD).text_color(rgb(0xf8fafc)).child("Add Email Account"))
                                .child(div().text_sm().text_color(rgb(0x94a3b8)).cursor_pointer().child("✕"))
                        )
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(8.0))
                                .child(div().text_xs().text_color(rgb(0x94a3b8)).child("Select Provider:"))
                                .child(
                                    div()
                                        .flex()
                                        .flex_row()
                                        .gap(px(8.0))
                                        .child(div().px(px(10.0)).py(px(6.0)).rounded_md().bg(rgb(0x2563eb)).text_xs().text_color(rgb(0xffffff)).child("IMAP / SMTP"))
                                        .child(div().px(px(10.0)).py(px(6.0)).rounded_md().bg(rgb(0x1e293b)).text_xs().text_color(rgb(0x94a3b8)).child("Google OAuth2"))
                                        .child(div().px(px(10.0)).py(px(6.0)).rounded_md().bg(rgb(0x1e293b)).text_xs().text_color(rgb(0x94a3b8)).child("Fastmail JMAP"))
                                        .child(div().px(px(10.0)).py(px(6.0)).rounded_md().bg(rgb(0x1e293b)).text_xs().text_color(rgb(0x94a3b8)).child("Microsoft 365"))
                                )
                        )
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .justify_end()
                                .gap(px(8.0))
                                .pt(px(12.0))
                                .child(div().px(px(14.0)).py(px(6.0)).rounded_md().bg(rgb(0x1e293b)).text_xs().text_color(rgb(0xcbd5e1)).cursor_pointer().child("Cancel"))
                                .child(div().px(px(16.0)).py(px(6.0)).rounded_md().bg(rgb(0x2563eb)).text_xs().text_color(rgb(0xffffff)).cursor_pointer().child("Connect Account"))
                        )
                )
        }
    }

    /// Launch the GPUI Desktop Application
    pub fn run_gpui_app(
        sync_rx: flume::Receiver<SyncEvent>,
        sync_tx: flume::Sender<SyncEvent>,
    ) {
        Application::new().run(move |cx: &mut App| {
            let rx = sync_rx.clone();
            let tx = sync_tx.clone();
            let _ = cx.open_window(
                WindowOptions {
                    titlebar: Some(TitlebarOptions {
                        title: Some("Vespetrel Mail".into()),
                        appears_transparent: false,
                        traffic_light_position: None,
                    }),
                    window_min_size: Some(size(px(900.0), px(600.0))),
                    ..Default::default()
                },
                move |_window, cx| {
                    cx.new(|cx| MainWindow::new(cx, rx, tx))
                },
            );
        });
    }
}
