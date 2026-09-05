#[cfg(feature = "gpui")]
pub mod gpui_app {
    use crate::views::{
        calendar::CalendarView,
        contacts::ContactsView,
        login_wizard::{AuthModeChoice, LoginWizardState, WizardStep},
        message_list::{ListFilter, MessageRowDensity},
        message_viewer::{AttachmentInfo, MessageViewer, SecurityStatus},
        navigation::NavigationTree,
        quick_filter::QuickFilterState,
        tasks::TaskListView,
    };
    pub use gpui_kit::base;
    pub use gpui_kit::component;
    pub use gpui_kit::gpui;
    pub use gpui_kit::gpui::*;
    use vespetrel_core::{
        Account, CalendarEvent, Contact, Folder, FolderRole, MessageSummary, ProviderType,
        TaskItem, UserSettings, provider::SyncEvent,
    };

    /// Interactive input field handles for the Login Setup Wizard
    pub struct WizardInputEntities {
        pub email: Entity<component::input::InputState>,
        pub password: Entity<component::input::InputState>,
        pub name: Entity<component::input::InputState>,
        pub incoming_host: Entity<component::input::InputState>,
        pub incoming_port: Entity<component::input::InputState>,
        pub outgoing_host: Entity<component::input::InputState>,
        pub outgoing_port: Entity<component::input::InputState>,
        pub client_id: Entity<component::input::InputState>,
    }

    impl WizardInputEntities {
        pub fn new(
            window: &mut Window,
            cx: &mut Context<MainWindow>,
            wizard: &LoginWizardState,
        ) -> Self {
            let email_val = wizard.email.clone();
            let pass_val = wizard.password_or_token.clone();
            let name_val = wizard.name.clone();
            let in_host_val = if wizard.incoming_host.is_empty() {
                match wizard.provider_type {
                    ProviderType::Gmail => "imap.gmail.com".to_string(),
                    ProviderType::Graph => "graph.microsoft.com".to_string(),
                    ProviderType::Jmap => "api.fastmail.com".to_string(),
                    ProviderType::Imap => "".to_string(),
                }
            } else {
                wizard.incoming_host.clone()
            };
            let in_port_val = wizard.incoming_port.to_string();
            let out_host_val = if wizard.outgoing_host.is_empty() {
                match wizard.provider_type {
                    ProviderType::Gmail => "smtp.gmail.com".to_string(),
                    ProviderType::Graph => "graph.microsoft.com".to_string(),
                    ProviderType::Jmap => "api.fastmail.com".to_string(),
                    ProviderType::Imap => "".to_string(),
                }
            } else {
                wizard.outgoing_host.clone()
            };
            let out_port_val = wizard.outgoing_port.to_string();
            let client_id_val =
                wizard
                    .client_id
                    .clone()
                    .unwrap_or_else(|| match wizard.provider_type {
                        ProviderType::Gmail => {
                            std::env::var("VESPETREL_GOOGLE_CLIENT_ID").unwrap_or_default()
                        }
                        ProviderType::Graph => {
                            std::env::var("VESPETREL_MICROSOFT_CLIENT_ID").unwrap_or_default()
                        }
                        _ => String::new(),
                    });

            let email = cx.new(|cx| {
                let mut st =
                    component::input::InputState::new(window, cx).placeholder("user@example.com");
                if !email_val.is_empty() {
                    st = st.default_value(email_val);
                }
                st
            });

            let password = cx.new(|cx| {
                let mut st = component::input::InputState::new(window, cx)
                    .placeholder("Password or 16-character App Password")
                    .masked(true);
                if !pass_val.is_empty() {
                    st = st.default_value(pass_val);
                }
                st
            });

            let name = cx.new(|cx| {
                let mut st = component::input::InputState::new(window, cx)
                    .placeholder("Display Name (e.g. Alex Smith)");
                if !name_val.is_empty() {
                    st = st.default_value(name_val);
                }
                st
            });

            let incoming_host = cx.new(|cx| {
                let mut st =
                    component::input::InputState::new(window, cx).placeholder("imap.example.com");
                if !in_host_val.is_empty() {
                    st = st.default_value(in_host_val);
                }
                st
            });

            let incoming_port = cx.new(|cx| {
                component::input::InputState::new(window, cx)
                    .placeholder("993")
                    .default_value(in_port_val)
            });

            let outgoing_host = cx.new(|cx| {
                let mut st =
                    component::input::InputState::new(window, cx).placeholder("smtp.example.com");
                if !out_host_val.is_empty() {
                    st = st.default_value(out_host_val);
                }
                st
            });

            let outgoing_port = cx.new(|cx| {
                component::input::InputState::new(window, cx)
                    .placeholder("587")
                    .default_value(out_port_val)
            });

            let client_id = cx.new(|cx| {
                let mut st = component::input::InputState::new(window, cx)
                    .placeholder("OAuth Client ID (e.g. 12345.apps.googleusercontent.com)");
                if !client_id_val.is_empty() {
                    st = st.default_value(client_id_val);
                }
                st
            });

            Self {
                email,
                password,
                name,
                incoming_host,
                incoming_port,
                outgoing_host,
                outgoing_port,
                client_id,
            }
        }
    }

    pub struct ComposeInputEntities {
        pub to: Entity<component::input::InputState>,
        pub subject: Entity<component::input::InputState>,
        pub body: Entity<component::input::InputState>,
    }

    impl ComposeInputEntities {
        pub fn new(
            window: &mut Window,
            cx: &mut Context<MainWindow>,
            to_val: &str,
            subj_val: &str,
            body_val: &str,
        ) -> Self {
            let to = cx.new(|cx| {
                let mut st = component::input::InputState::new(window, cx)
                    .placeholder("Recipient (e.g. user@example.com)");
                if !to_val.is_empty() {
                    st = st.default_value(to_val);
                }
                st
            });
            let subject = cx.new(|cx| {
                let mut st = component::input::InputState::new(window, cx).placeholder("Subject");
                if !subj_val.is_empty() {
                    st = st.default_value(subj_val);
                }
                st
            });
            let body = cx.new(|cx| {
                let mut st = component::input::InputState::new(window, cx)
                    .placeholder("Write your email message here...");
                if !body_val.is_empty() {
                    st = st.default_value(body_val);
                }
                st
            });
            Self { to, subject, body }
        }
    }

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

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum MessageSortOrder {
        DateDescending,
        DateAscending,
    }

    #[derive(Debug, Clone)]
    pub struct ThreadedMessage<'a> {
        pub summary: &'a MessageSummary,
        pub thread_count: usize,
        pub is_child: bool,
    }

    #[derive(Debug, Clone)]
    pub struct Toast {
        pub id: String,
        pub message: String,
        pub is_error: bool,
        pub undo_outbox_id: Option<String>,
    }

    pub struct MainWindow {
        pub active_tab: ActiveViewTab,
        pub active_modal: ActiveModal,
        pub accounts: Vec<Account>,
        pub folders: Vec<Folder>,
        pub selected_folder_id: Option<String>,
        pub is_unified_inbox: bool,
        pub messages: Vec<MessageSummary>,
        pub selected_message_id: Option<String>,
        pub list_filter: ListFilter,
        pub search_query: String,
        pub block_remote_images: bool,
        pub message_viewer: MessageViewer,
        pub is_threaded: bool,
        pub sort_order: MessageSortOrder,
        pub row_density: MessageRowDensity,
        pub quick_filter_state: QuickFilterState,
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
        pub compose_inputs: Option<ComposeInputEntities>,
        pub compose_is_markdown: bool,
        pub compose_attachments: Vec<vespetrel_core::message::ComposedAttachment>,
        pub compose_reply_to_id: Option<String>,
        pub compose_draft_id: Option<String>,
        // Command Palette
        pub palette_query: String,
        pub command_palette: crate::command_palette::CommandPalette,
        // Event channel from Tokio sync engine
        pub sync_sender: flume::Sender<SyncEvent>,
        pub status_message: String,
        pub storage_pool: Option<vespetrel_storage::db::StoragePool>,
        pub login_wizard: LoginWizardState,
        pub wizard_inputs: Option<WizardInputEntities>,
        pub toasts: Vec<Toast>,
    }

    impl MainWindow {
        /// Creates a mock `MainWindow` with in-memory sample fixtures (for tests or headless mock preview).
        /// In production runtime, use [`MainWindow::from_storage`] with an initialized SQLite storage pool.
        pub fn new(
            cx: &mut Context<Self>,
            sync_rx: flume::Receiver<SyncEvent>,
            sync_tx: flume::Sender<SyncEvent>,
        ) -> Self {
            Self::from_storage(cx, sync_rx, sync_tx, None)
        }

        /// Production entry point: creates `MainWindow` and initializes asynchronous hydration from the storage pool.
        #[allow(clippy::type_complexity)]
        pub fn from_storage(
            cx: &mut Context<Self>,
            sync_rx: flume::Receiver<SyncEvent>,
            sync_tx: flume::Sender<SyncEvent>,
            storage_pool: Option<vespetrel_storage::db::StoragePool>,
        ) -> Self {
            let (accounts, folders, messages, calendar_events, contacts, tasks): (
                Vec<Account>,
                Vec<Folder>,
                Vec<MessageSummary>,
                Vec<CalendarEvent>,
                Vec<Contact>,
                Vec<TaskItem>,
            ) = (
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
            );

            if let Some(pool) = &storage_pool {
                let pool_clone = pool.clone();
                cx.spawn(async move |this, cx| {
                    if let Ok(conn) = pool_clone.get().await {
                        let res = conn
                            .interact(move |c| {
                                let accounts =
                                    vespetrel_storage::repo::list_accounts(c).unwrap_or_default();
                                let mut all_folders = Vec::new();
                                for acc in &accounts {
                                    if let Ok(f_list) =
                                        vespetrel_storage::repo::list_folders(c, &acc.id)
                                    {
                                        all_folders.extend(f_list);
                                    }
                                }

                                let folder_counts = vespetrel_storage::repo::get_folder_counts(c)
                                    .unwrap_or_default();
                                for f in &mut all_folders {
                                    if let Some(&(total, unread)) = folder_counts
                                        .get(&f.id)
                                        .or_else(|| folder_counts.get(&f.remote_id))
                                    {
                                        f.total_count = total;
                                        f.unread_count = unread;
                                    }
                                }

                                let mut initial_messages = Vec::new();
                                let msgs =
                                    vespetrel_storage::repo::list_unified_inbox_messages(c, 100, 0)
                                        .unwrap_or_default();
                                if !msgs.is_empty() {
                                    initial_messages =
                                        msgs.into_iter().map(|m| m.summary()).collect();
                                } else if let Some(inbox) = all_folders
                                    .iter()
                                    .find(|f| f.role == FolderRole::Inbox)
                                    .or_else(|| all_folders.first())
                                {
                                    let msgs = vespetrel_storage::repo::list_messages_in_folder(
                                        c, &inbox.id, 100, 0,
                                    )
                                    .unwrap_or_default();
                                    initial_messages =
                                        msgs.into_iter().map(|m| m.summary()).collect();
                                }

                                let mut all_contacts = Vec::new();
                                for acc in &accounts {
                                    if let Ok(contacts) =
                                        vespetrel_storage::repo::list_contacts(c, &acc.id)
                                    {
                                        all_contacts.extend(contacts);
                                    }
                                }
                                if all_contacts.is_empty() {
                                    let contacts =
                                        vespetrel_storage::repo::list_contacts(c, "addr-main")
                                            .unwrap_or_default();
                                    all_contacts.extend(contacts);
                                }

                                let mut cal_events = Vec::new();
                                let cal_ids: Vec<String> = c
                                    .prepare("SELECT DISTINCT calendar_id FROM calendar_events")
                                    .and_then(|mut stmt| {
                                        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
                                        Ok(rows.flatten().collect())
                                    })
                                    .unwrap_or_default();
                                for cal_id in cal_ids {
                                    if let Ok(events) =
                                        vespetrel_storage::repo::list_calendar_events(c, &cal_id)
                                    {
                                        cal_events.extend(events);
                                    }
                                }
                                if cal_events.is_empty() {
                                    cal_events = vespetrel_storage::repo::list_calendar_events(
                                        c, "cal-main",
                                    )
                                    .unwrap_or_default();
                                }

                                let mut task_items = Vec::new();
                                let task_cal_ids: Vec<String> = c
                                    .prepare("SELECT DISTINCT calendar_id FROM tasks")
                                    .and_then(|mut stmt| {
                                        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
                                        Ok(rows.flatten().collect())
                                    })
                                    .unwrap_or_default();
                                for cal_id in task_cal_ids {
                                    if let Ok(tasks) =
                                        vespetrel_storage::repo::list_tasks(c, &cal_id)
                                    {
                                        task_items.extend(tasks);
                                    }
                                }
                                if task_items.is_empty() {
                                    task_items = vespetrel_storage::repo::list_tasks(c, "cal-main")
                                        .unwrap_or_default();
                                }

                                let settings = vespetrel_storage::repo::get_user_settings(c)
                                    .unwrap_or_default();

                                (
                                    accounts,
                                    all_folders,
                                    initial_messages,
                                    all_contacts,
                                    cal_events,
                                    task_items,
                                    settings,
                                )
                            })
                            .await;

                        if let Ok((accs, flds, msgs, cnts, cals, tsks, stgs)) = res {
                            let _ = this.update(cx, |view, cx| {
                                view.accounts = accs;
                                view.folders = flds;
                                if let Some(first_fld) = view.folders.first() {
                                    view.selected_folder_id = Some(first_fld.id.clone());
                                }
                                view.messages = msgs;
                                view.contacts = cnts;
                                view.calendar_events = cals;
                                view.tasks = tsks;
                                view.settings = stgs;
                                cx.notify();
                            });
                        }
                    }
                })
                .detach();
            }

            // Spawn Tokio Bridge Listener bound to GPUI Context
            let bridge_rx = sync_rx;
            cx.spawn(async move |this, cx| {
                while let Ok(event) = bridge_rx.recv_async().await {
                    let _ = this.update(cx, |view, cx| {
                        view.handle_sync_event(event, cx);
                    });
                }
            })
            .detach();

            Self {
                active_tab: ActiveViewTab::Mail,
                active_modal: ActiveModal::None,
                accounts,
                selected_folder_id: folders.first().map(|f: &Folder| f.id.clone()),
                is_unified_inbox: true,
                selected_message_id: messages.first().map(|m: &MessageSummary| m.id.clone()),
                folders,
                messages,
                list_filter: ListFilter::All,
                search_query: String::new(),
                block_remote_images: true,
                message_viewer: MessageViewer::new(),
                is_threaded: false,
                sort_order: MessageSortOrder::DateDescending,
                row_density: MessageRowDensity::Comfortable,
                quick_filter_state: QuickFilterState::new(),
                calendar_events,
                contacts,
                tasks,
                settings: UserSettings::default(),
                compose_to: "team@vespetrel.example".into(),
                compose_subject: "Hello from Pure Rust GPUI Mail".into(),
                compose_body: "Hi team,\n\nWriting this from the pure Rust GPUI mail client.\n\nBest regards,\nVespetrel User".into(),
                compose_inputs: None,
                compose_is_markdown: false,
                compose_attachments: Vec::new(),
                compose_reply_to_id: None,
                compose_draft_id: None,
                palette_query: String::new(),
                command_palette: crate::command_palette::CommandPalette::new(),
                sync_sender: sync_tx,
                status_message: "All mailboxes synchronized".into(),
                storage_pool,
                login_wizard: LoginWizardState::new(),
                wizard_inputs: None,
                toasts: Vec::new(),
            }
        }

        pub fn handle_sync_event(&mut self, event: SyncEvent, cx: &mut Context<Self>) {
            match event {
                SyncEvent::MessagesInserted(new_msgs) => {
                    self.status_message = format!("Received {} new message(s)", new_msgs.len());
                    if let Some(first) = new_msgs.first() {
                        let sender = if let Some(ref name) = first.from_name {
                            if !name.is_empty() {
                                format!("{} <{}>", name, first.from_address)
                            } else {
                                first.from_address.clone()
                            }
                        } else {
                            first.from_address.clone()
                        };
                        let subj = first.subject.as_deref().unwrap_or("(No subject)");
                        let toast_msg = if new_msgs.len() == 1 {
                            format!("✉️ New email from {}: {}", sender, subj)
                        } else {
                            format!("✉️ {} new emails (latest from {})", new_msgs.len(), sender)
                        };
                        self.show_toast(toast_msg, false, cx);
                    }
                    self.messages.splice(0..0, new_msgs);
                    cx.notify();
                }
                SyncEvent::MessageFlagsUpdated {
                    id,
                    is_read,
                    is_flagged,
                } => {
                    if let Some(m) = self.messages.iter_mut().find(|m| m.id == id) {
                        m.is_read = is_read;
                        m.is_flagged = is_flagged;
                        cx.notify();
                    }
                }
                SyncEvent::MessagesDeleted(ids) => {
                    self.messages.retain(|m| !ids.contains(&m.id));
                    if self
                        .selected_message_id
                        .as_ref()
                        .is_some_and(|sel| ids.contains(sel))
                    {
                        self.selected_message_id = self.messages.first().map(|m| m.id.clone());
                    }
                    cx.notify();
                }
                SyncEvent::SyncError { folder, error } => {
                    self.status_message = format!("⚠️ Sync error ({folder}): {error}");
                    self.show_toast(format!("Sync error ({}): {}", folder, error), true, cx);
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

        pub fn select_message(&mut self, message_id: String, cx: &mut Context<Self>) {
            self.selected_message_id = Some(message_id.clone());
            if let Some(m) = self.messages.iter_mut().find(|m| m.id == message_id) {
                m.is_read = true;
            }
            if let Some(m) = self.messages.iter().find(|m| m.id == message_id).cloned() {
                self.message_viewer.subject = m.subject.clone();
                self.message_viewer.from = Some(vespetrel_core::message::Address {
                    name: m.from_name.clone(),
                    email: m.from_address.clone(),
                });
                self.message_viewer.sent_at = Some(m.sent_at);
                self.message_viewer.block_remote_images = self.block_remote_images;
                let snippet = m.snippet.as_deref().unwrap_or("No content available.");
                if snippet.contains("<html") || snippet.contains("<div") || snippet.contains("<p") {
                    self.message_viewer
                        .load_html(snippet, self.block_remote_images);
                } else {
                    self.message_viewer.load_text(snippet);
                }
            }
            cx.notify();

            // Async hydration of full body and attachments from storage
            if let Some(pool) = &self.storage_pool {
                let pool = pool.clone();
                let mid = message_id.clone();
                let block_images = self.block_remote_images;
                cx.spawn(async move |this, cx| {
                    if let Ok(conn) = pool.get().await {
                        let res = conn
                            .interact(move |c| {
                                let _ = vespetrel_storage::repo::update_message_flags(
                                    c,
                                    &mid,
                                    Some(true),
                                    None,
                                );
                                let full_msg =
                                    vespetrel_storage::repo::get_message(c, &mid).ok().flatten();
                                let atts =
                                    vespetrel_storage::repo::list_attachments_for_message(c, &mid)
                                        .unwrap_or_default();
                                (full_msg, atts)
                            })
                            .await;
                        if let Ok((Some(full), atts)) = res {
                            let _ = this.update(cx, |view, cx| {
                                if view.selected_message_id.as_deref() == Some(&full.id) {
                                    view.message_viewer.subject = full.subject.clone();
                                    view.message_viewer.from =
                                        Some(vespetrel_core::message::Address {
                                            name: full.from_name.clone(),
                                            email: full.from_address.clone(),
                                        });
                                    view.message_viewer.sent_at = Some(full.sent_at);
                                    // Detect S/MIME or OpenPGP security status
                                    let has_smime = atts.iter().any(|a| {
                                        a.content_type.contains("pkcs7")
                                            || a.filename.ends_with(".p7s")
                                            || a.filename.ends_with(".p7m")
                                    });
                                    view.message_viewer.attachments = atts
                                        .into_iter()
                                        .map(|a| AttachmentInfo {
                                            filename: a.filename,
                                            content_type: a.content_type,
                                            size_bytes: a.size_bytes as usize,
                                            blob_path: a.blob_path,
                                        })
                                        .collect();
                                    let content = full
                                        .body_text_preview
                                        .as_deref()
                                        .or(full.body_snippet.as_deref())
                                        .unwrap_or("No content available.");

                                    if has_smime {
                                        view.message_viewer.security_status =
                                            SecurityStatus::SmimeValid;
                                    } else if content.contains("-----BEGIN PGP MESSAGE-----") {
                                        view.message_viewer.security_status =
                                            SecurityStatus::PgpEncryptedAndSigned;
                                    } else if content.contains("-----BEGIN PGP SIGNED MESSAGE-----")
                                    {
                                        view.message_viewer.security_status =
                                            SecurityStatus::PgpSignedValid;
                                    }

                                    if content.contains("<html")
                                        || content.contains("<div")
                                        || content.contains("<p")
                                    {
                                        view.message_viewer.load_html(content, block_images);
                                    } else {
                                        view.message_viewer.load_text(content);
                                    }
                                    cx.notify();
                                }
                            });
                        }
                    }
                })
                .detach();
            }
        }

        pub fn execute_fts_search(&mut self, query: &str, cx: &mut Context<Self>) {
            let q = query.trim().to_string();
            self.search_query = q.clone();
            if q.is_empty() {
                self.reload_messages_for_current_folder(cx);
                return;
            }

            let pool_opt = self.storage_pool.clone();
            let acct_id = self.accounts.first().map(|a| a.id.clone());

            cx.spawn(async move |this, cx| {
                if let Some(pool) = pool_opt
                    && let Ok(conn) = pool.get().await
                {
                    let search_res = conn
                        .interact(move |c| {
                            let hits =
                                vespetrel_storage::search_messages(c, &q, acct_id.as_deref(), 100)?;
                            let hit_ids: Vec<String> =
                                hits.into_iter().map(|h| h.message_id).collect();
                            let full_msgs =
                                vespetrel_storage::repo::list_messages_by_ids(c, &hit_ids)?;
                            Ok::<Vec<MessageSummary>, anyhow::Error>(
                                full_msgs.into_iter().map(|m| m.summary()).collect(),
                            )
                        })
                        .await;

                    if let Ok(Ok(results)) = search_res {
                        let _ = this.update(cx, |view, cx| {
                            view.messages = results;
                            view.selected_message_id = view.messages.first().map(|m| m.id.clone());
                            view.status_message =
                                format!("FTS5 Search: {} message(s) found", view.messages.len());
                            cx.notify();
                        });
                    }
                }
            })
            .detach();
        }

        pub fn clear_search(&mut self, cx: &mut Context<Self>) {
            self.search_query.clear();
            self.reload_messages_for_current_folder(cx);
        }

        pub fn toggle_remote_images(&mut self, cx: &mut Context<Self>) {
            self.block_remote_images = !self.block_remote_images;
            self.message_viewer.toggle_remote_images();
            cx.notify();
        }

        pub fn toggle_threading(&mut self, cx: &mut Context<Self>) {
            self.is_threaded = !self.is_threaded;
            cx.notify();
        }

        pub fn cycle_sort_order(&mut self, cx: &mut Context<Self>) {
            self.sort_order = match self.sort_order {
                MessageSortOrder::DateDescending => MessageSortOrder::DateAscending,
                MessageSortOrder::DateAscending => MessageSortOrder::DateDescending,
            };
            cx.notify();
        }

        pub fn cycle_row_density(&mut self, cx: &mut Context<Self>) {
            self.row_density = match self.row_density {
                MessageRowDensity::Compact => MessageRowDensity::Comfortable,
                MessageRowDensity::Comfortable => MessageRowDensity::Roomy,
                MessageRowDensity::Roomy => MessageRowDensity::Compact,
            };
            self.settings.row_density = match self.row_density {
                MessageRowDensity::Compact => vespetrel_core::RowDensity::Compact,
                MessageRowDensity::Comfortable => vespetrel_core::RowDensity::Comfortable,
                MessageRowDensity::Roomy => vespetrel_core::RowDensity::Roomy,
            };
            self.save_settings(cx);
            cx.notify();
        }

        pub fn save_settings(&mut self, cx: &mut Context<Self>) {
            let mut stgs = self.settings.clone();
            stgs.sanitize();
            self.settings = stgs.clone();
            let pool_opt = self.storage_pool.clone();
            cx.spawn(async move |this, cx| {
                if let Some(pool) = pool_opt
                    && let Ok(conn) = pool.get().await
                {
                    let save_res = conn
                        .interact(move |c| vespetrel_storage::repo::save_user_settings(c, &stgs))
                        .await;
                    if let Ok(Ok(())) = save_res {
                        let _ = this.update(cx, |view, cx| {
                            view.show_toast("Preferences saved", false, cx);
                        });
                    }
                }
            })
            .detach();
        }

        pub fn set_settings_theme(
            &mut self,
            theme: vespetrel_core::ColorTheme,
            cx: &mut Context<Self>,
        ) {
            self.settings.theme = theme;
            self.save_settings(cx);
            cx.notify();
        }

        pub fn set_settings_density(
            &mut self,
            density: vespetrel_core::RowDensity,
            cx: &mut Context<Self>,
        ) {
            self.settings.row_density = density;
            self.row_density = match density {
                vespetrel_core::RowDensity::Compact => MessageRowDensity::Compact,
                vespetrel_core::RowDensity::Comfortable => MessageRowDensity::Comfortable,
                vespetrel_core::RowDensity::Roomy => MessageRowDensity::Roomy,
            };
            self.save_settings(cx);
            cx.notify();
        }

        pub fn toggle_settings_strip_trackers(&mut self, cx: &mut Context<Self>) {
            self.settings.auto_strip_trackers = !self.settings.auto_strip_trackers;
            self.save_settings(cx);
            cx.notify();
        }

        pub fn toggle_settings_warn_phishing(&mut self, cx: &mut Context<Self>) {
            self.settings.warn_on_phishing = !self.settings.warn_on_phishing;
            self.save_settings(cx);
            cx.notify();
        }

        pub fn set_settings_undo_seconds(&mut self, secs: u32, cx: &mut Context<Self>) {
            self.settings.undo_send_seconds = secs.min(60);
            self.save_settings(cx);
            cx.notify();
        }

        pub fn filtered_messages(&self) -> Vec<&MessageSummary> {
            let mut qf = self.quick_filter_state.clone();
            qf.search_query = self.search_query.clone();
            match self.list_filter {
                ListFilter::All => {
                    qf.unread_only = false;
                    qf.starred_only = false;
                    qf.has_attachment_only = false;
                }
                ListFilter::Unread => {
                    qf.unread_only = true;
                }
                ListFilter::Flagged => {
                    qf.starred_only = true;
                }
                ListFilter::WithAttachments => {
                    qf.has_attachment_only = true;
                }
            }
            let mut filtered = qf.filter_messages(&self.messages);
            match self.sort_order {
                MessageSortOrder::DateDescending => {
                    filtered.sort_by_key(|a| std::cmp::Reverse(a.sent_at));
                }
                MessageSortOrder::DateAscending => {
                    filtered.sort_by_key(|a| a.sent_at);
                }
            }
            filtered
        }

        pub fn threaded_messages(&self) -> Vec<ThreadedMessage<'_>> {
            let filtered = self.filtered_messages();
            if !self.is_threaded {
                return filtered
                    .into_iter()
                    .map(|m| ThreadedMessage {
                        summary: m,
                        thread_count: 1,
                        is_child: false,
                    })
                    .collect();
            }

            let mut groups: Vec<(String, Vec<&MessageSummary>)> = Vec::new();
            let mut key_to_idx: std::collections::HashMap<String, usize> =
                std::collections::HashMap::new();

            for m in filtered {
                let norm_subj = m
                    .subject
                    .as_deref()
                    .map(vespetrel_core::thread::normalize_subject)
                    .unwrap_or_else(|| "(No Subject)".to_string());
                let key = m.thread_id.clone().unwrap_or(norm_subj);
                if let Some(&idx) = key_to_idx.get(&key) {
                    groups[idx].1.push(m);
                } else {
                    let idx = groups.len();
                    key_to_idx.insert(key.clone(), idx);
                    groups.push((key, vec![m]));
                }
            }

            let mut result = Vec::new();
            for (_key, mut msgs) in groups {
                let count = msgs.len();
                match self.sort_order {
                    MessageSortOrder::DateDescending => {
                        msgs.sort_by_key(|a| std::cmp::Reverse(a.sent_at))
                    }
                    MessageSortOrder::DateAscending => msgs.sort_by_key(|a| a.sent_at),
                }
                for (i, m) in msgs.into_iter().enumerate() {
                    result.push(ThreadedMessage {
                        summary: m,
                        thread_count: if i == 0 { count } else { 1 },
                        is_child: i > 0,
                    });
                }
            }
            result
        }

        pub fn trigger_sync(&mut self, cx: &mut Context<Self>) {
            self.status_message = "Syncing mailboxes...".into();
            let _pool_opt = self.storage_pool.clone();
            let tx = self.sync_sender.clone();
            let accounts = self.accounts.clone();
            cx.spawn(async move |this, cx| {
                if accounts.is_empty() {
                    let _ = this.update(cx, |view, cx| {
                        view.status_message = "No accounts configured".into();
                        cx.notify();
                    });
                    return;
                }
                for acct in accounts {
                    let provider = vespetrel_engine::make_provider(&acct);
                    match provider.sync_folder_list().await {
                        Ok(folders) => {
                            let _ = tx.send(SyncEvent::FolderListUpdated(folders.clone()));
                            for f in &folders {
                                let fld = vespetrel_core::Folder::new(
                                    &acct.id,
                                    &f.remote_id,
                                    &f.name,
                                    &f.path,
                                );
                                if let Ok(delta) =
                                    provider.sync_messages(&fld, Default::default()).await
                                {
                                    let summaries: Vec<MessageSummary> = delta
                                        .inserted
                                        .iter()
                                        .map(|sm| MessageSummary {
                                            id: sm
                                                .remote_id
                                                .clone()
                                                .unwrap_or_else(|| sm.remote_uid.to_string()),
                                            thread_id: None,
                                            subject: Some(format!("Message {}", sm.remote_uid)),
                                            from_address: acct.email.clone(),
                                            from_name: None,
                                            snippet: None,
                                            sent_at: chrono::Utc::now(),
                                            is_read: sm
                                                .flags
                                                .contains(&vespetrel_core::message::Flag::Seen),
                                            is_flagged: sm
                                                .flags
                                                .contains(&vespetrel_core::message::Flag::Flagged),
                                            has_attachments: false,
                                        })
                                        .collect();
                                    if !summaries.is_empty() {
                                        let _ = tx.send(SyncEvent::MessagesInserted(summaries));
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            let _ = tx.send(SyncEvent::SyncError {
                                folder: "ALL".into(),
                                error: e.to_string(),
                            });
                        }
                    }
                }
                let _ = this.update(cx, |view, cx| {
                    view.status_message = "Mailbox sync complete".into();
                    cx.notify();
                });
            })
            .detach();
        }

        pub fn toggle_flag(&mut self, cx: &mut Context<Self>) {
            if let Some(id) = self.selected_message_id.clone()
                && let Some(m) = self.messages.iter_mut().find(|m| m.id == id)
            {
                m.is_flagged = !m.is_flagged;
                let is_flagged = m.is_flagged;
                let is_read = m.is_read;
                cx.notify();

                if let Some(pool) = &self.storage_pool {
                    let pool = pool.clone();
                    let msg_id = id.clone();
                    cx.spawn(async move |_this, _cx| {
                        if let Ok(conn) = pool.get().await {
                            let _ = conn
                                .interact(move |c| {
                                    c.execute(
                                        "UPDATE messages SET is_flagged = ?1 WHERE id = ?2",
                                        rusqlite::params![is_flagged, msg_id],
                                    )
                                })
                                .await;
                        }
                    })
                    .detach();
                }

                let _ = self.sync_sender.send(SyncEvent::MessageFlagsUpdated {
                    id,
                    is_read,
                    is_flagged,
                });
            }
        }

        pub fn delete_selected_message(&mut self, cx: &mut Context<Self>) {
            if let Some(id) = self.selected_message_id.clone() {
                let pool_opt = self.storage_pool.clone();
                let current_folder_id = self.selected_folder_id.clone();
                let is_in_trash = self.folders.iter().any(|f| {
                    Some(&f.id) == current_folder_id.as_ref()
                        && f.role == vespetrel_core::FolderRole::Trash
                });

                self.messages.retain(|m| m.id != id);
                self.selected_message_id = self.messages.first().map(|m| m.id.clone());

                if is_in_trash {
                    self.show_toast("Message permanently deleted", false, cx);
                    if let Some(pool) = pool_opt {
                        let msg_id = id.clone();
                        cx.spawn(async move |_this, _cx| {
                            if let Ok(conn) = pool.get().await {
                                let _ = conn
                                    .interact(move |c| {
                                        vespetrel_storage::repo::delete_message(c, &msg_id)
                                    })
                                    .await;
                            }
                        })
                        .detach();
                    }
                    let _ = self.sync_sender.send(SyncEvent::MessagesDeleted(vec![id]));
                } else {
                    self.show_toast("Message moved to Trash", false, cx);
                    let trash_folder_id = self
                        .folders
                        .iter()
                        .find(|f| f.role == vespetrel_core::FolderRole::Trash)
                        .map(|f| f.id.clone());

                    if let Some(trash_id) = trash_folder_id
                        && let Some(pool) = pool_opt
                    {
                        let msg_id = id.clone();
                        cx.spawn(async move |this, cx| {
                            if let Ok(conn) = pool.get().await {
                                let _ = conn
                                    .interact(move |c| {
                                        c.execute(
                                            "UPDATE messages SET folder_id = ?1 WHERE id = ?2",
                                            rusqlite::params![trash_id, msg_id],
                                        )
                                    })
                                    .await;
                                let _ = this.update(cx, |view, cx| {
                                    view.reload_folder_counts(cx);
                                });
                            }
                        })
                        .detach();
                    }
                }
                cx.notify();
            }
        }

        pub fn archive_selected_message(&mut self, cx: &mut Context<Self>) {
            if let Some(id) = self.selected_message_id.clone() {
                self.messages.retain(|m| m.id != id);
                self.selected_message_id = self.messages.first().map(|m| m.id.clone());
                self.show_toast("Message archived", false, cx);
                cx.notify();

                if let Some(pool) = &self.storage_pool {
                    let pool = pool.clone();
                    let msg_id = id.clone();
                    cx.spawn(async move |this, cx| {
                        if let Ok(conn) = pool.get().await {
                            let _ = conn
                                .interact(move |c| {
                                    let archive_id: Option<String> = c
                                        .query_row(
                                            "SELECT id FROM folders WHERE role = 'archive' LIMIT 1",
                                            [],
                                            |r| r.get(0),
                                        )
                                        .ok();
                                    if let Some(arch_id) = archive_id {
                                        c.execute(
                                            "UPDATE messages SET folder_id = ?1 WHERE id = ?2",
                                            rusqlite::params![arch_id, msg_id],
                                        )?;
                                    }
                                    Ok::<(), vespetrel_storage::StorageError>(())
                                })
                                .await;
                            let _ = this.update(cx, |view, cx| {
                                view.reload_folder_counts(cx);
                            });
                        }
                    })
                    .detach();
                }
            }
        }

        pub fn show_toast(
            &mut self,
            message: impl Into<String>,
            is_error: bool,
            cx: &mut Context<Self>,
        ) {
            self.show_toast_with_undo(message, is_error, None, cx);
        }

        pub fn show_toast_with_undo(
            &mut self,
            message: impl Into<String>,
            is_error: bool,
            undo_outbox_id: Option<String>,
            cx: &mut Context<Self>,
        ) {
            let id = uuid::Uuid::new_v4().to_string();
            self.toasts.push(Toast {
                id,
                message: message.into(),
                is_error,
                undo_outbox_id,
            });
            if self.toasts.len() > 5 {
                self.toasts.remove(0);
            }
            cx.notify();
        }

        pub fn undo_send(&mut self, outbox_id: &str, cx: &mut Context<Self>) {
            let out_id = outbox_id.to_string();
            let pool_opt = self.storage_pool.clone();
            if let Some(pool) = pool_opt {
                cx.spawn(async move |this, cx| {
                    if let Ok(conn) = pool.get().await {
                        let cancel_res = conn
                            .interact(move |c| vespetrel_storage::repo::cancel_outbox(c, &out_id))
                            .await;
                        if let Ok(Ok(true)) = cancel_res {
                            let _ = this.update(cx, |view, cx| {
                                view.show_toast(
                                    "Sending undone. Message preserved in Outbox / Drafts.",
                                    false,
                                    cx,
                                );
                                view.reload_folder_counts(cx);
                            });
                        }
                    }
                })
                .detach();
            }
        }

        pub fn save_draft(&mut self, cx: &mut Context<Self>) {
            let (to_val, subj_val, body_val) = if let Some(inputs) = &self.compose_inputs {
                (
                    inputs.to.read(cx).value().trim().to_string(),
                    inputs.subject.read(cx).value().trim().to_string(),
                    inputs.body.read(cx).value().trim().to_string(),
                )
            } else {
                (
                    self.compose_to.trim().to_string(),
                    self.compose_subject.trim().to_string(),
                    self.compose_body.trim().to_string(),
                )
            };

            if to_val.is_empty() && subj_val.is_empty() && body_val.is_empty() {
                return;
            }

            self.compose_to = to_val.clone();
            self.compose_subject = subj_val.clone();
            self.compose_body = body_val.clone();

            let draft_id = self
                .compose_draft_id
                .clone()
                .unwrap_or_else(|| format!("draft-{}", uuid::Uuid::new_v4()));
            self.compose_draft_id = Some(draft_id.clone());

            let drafts_folder_id = self
                .folders
                .iter()
                .find(|f| f.role == vespetrel_core::FolderRole::Drafts)
                .map(|f| f.id.clone())
                .unwrap_or_else(|| "drafts-default".into());

            let acct_id = self
                .accounts
                .first()
                .map(|a| a.id.clone())
                .unwrap_or_else(|| "default".into());
            let from_email = self
                .accounts
                .first()
                .map(|a| a.email.clone())
                .unwrap_or_else(|| "me@localhost".into());

            let msg = vespetrel_core::Message {
                id: draft_id,
                account_id: acct_id,
                folder_id: drafts_folder_id,
                thread_id: None,
                remote_uid: (chrono::Utc::now().timestamp_millis() & 0x7FFFFFFF) as u32,
                message_id_header: None,
                in_reply_to: self.compose_reply_to_id.clone(),
                references: None,
                subject: if subj_val.is_empty() {
                    Some("(Draft)".into())
                } else {
                    Some(subj_val)
                },
                from_address: from_email,
                from_name: None,
                to_addresses: if to_val.is_empty() {
                    vec![]
                } else {
                    vec![vespetrel_core::Address {
                        name: None,
                        email: to_val,
                    }]
                },
                cc_addresses: vec![],
                bcc_addresses: vec![],
                reply_to: None,
                sent_at: chrono::Utc::now(),
                received_at: chrono::Utc::now(),
                is_read: true,
                is_flagged: false,
                is_draft: true,
                has_attachments: !self.compose_attachments.is_empty(),
                body_snippet: Some(body_val.chars().take(120).collect()),
                body_text_preview: Some(body_val.clone()),
                blob_path: String::new(),
                size_bytes: body_val.len() as i64,
                remote_id: None,
            };

            if let Some(pool) = self.storage_pool.clone() {
                cx.spawn(async move |this, cx| {
                    if let Ok(conn) = pool.get().await {
                        let _ = conn
                            .interact(move |c| vespetrel_storage::repo::insert_message(c, &msg))
                            .await;
                        let _ = this.update(cx, |view, cx| {
                            view.show_toast("Draft saved", false, cx);
                            view.reload_folder_counts(cx);
                        });
                    }
                })
                .detach();
            } else {
                self.show_toast("Draft saved", false, cx);
            }
        }

        pub fn save_attachment_to_downloads(
            &mut self,
            filename: &str,
            blob_path: Option<&str>,
            cx: &mut Context<Self>,
        ) {
            let safe_name = vespetrel_render::mime::sanitize_attachment_filename(filename);
            let downloads_dir = std::env::var("USERPROFILE")
                .map(|p| std::path::PathBuf::from(p).join("Downloads"))
                .unwrap_or_else(|_| {
                    std::env::var("HOME")
                        .map(|p| std::path::PathBuf::from(p).join("Downloads"))
                        .unwrap_or_else(|_| std::env::temp_dir())
                });

            let dest_path = downloads_dir.join(&safe_name);
            let blob_opt = blob_path.map(|s| s.to_string());
            let name_copy = safe_name.clone();

            cx.spawn(async move |this, cx| {
                let res = tokio::task::spawn_blocking(move || {
                    let data = if let Some(ref bp) = blob_opt {
                        if std::path::Path::new(bp).exists() {
                            std::fs::read(bp).unwrap_or_default()
                        } else {
                            format!("Vespetrel Attachment: {name_copy}\nBlob: {bp}\n").into_bytes()
                        }
                    } else {
                        format!("Vespetrel Attachment: {name_copy}\n").into_bytes()
                    };
                    std::fs::write(&dest_path, data)
                })
                .await;

                let _ = this.update(cx, |view, cx| match res {
                    Ok(Ok(())) => {
                        view.show_toast(format!("Saved {safe_name} to Downloads"), false, cx);
                    }
                    Ok(Err(e)) => {
                        view.show_toast(format!("Failed to save {safe_name}: {e}"), true, cx);
                    }
                    Err(e) => {
                        view.show_toast(format!("Task failed: {e}"), true, cx);
                    }
                });
            })
            .detach();
        }

        pub fn dismiss_toast(&mut self, id: &str, cx: &mut Context<Self>) {
            self.toasts.retain(|t| t.id != id);
            cx.notify();
        }

        pub fn select_folder(&mut self, folder_id: String, cx: &mut Context<Self>) {
            self.selected_folder_id = Some(folder_id);
            self.is_unified_inbox = false;
            self.reload_messages_for_current_folder(cx);
        }

        pub fn select_unified_inbox(&mut self, cx: &mut Context<Self>) {
            self.selected_folder_id = None;
            self.is_unified_inbox = true;
            self.reload_messages_for_current_folder(cx);
        }

        pub fn reload_messages_for_current_folder(&mut self, cx: &mut Context<Self>) {
            let pool_opt = self.storage_pool.clone();
            let is_unified = self.is_unified_inbox;
            let folder_id_opt = self.selected_folder_id.clone();
            let folder_obj = self
                .folders
                .iter()
                .find(|f| {
                    Some(&f.id) == folder_id_opt.as_ref()
                        || Some(&f.remote_id) == folder_id_opt.as_ref()
                })
                .cloned();
            let acct_opt = folder_obj
                .as_ref()
                .and_then(|f| self.accounts.iter().find(|a| a.id == f.account_id).cloned());

            cx.spawn(async move |this, cx| {
                if let Some(pool) = pool_opt
                    && let Ok(conn) = pool.get().await
                {
                    let fid_c = folder_id_opt.clone();
                    let msgs = conn
                        .interact(move |c| {
                            if is_unified {
                                vespetrel_storage::repo::list_unified_inbox_messages(c, 100, 0)
                            } else if let Some(fid) = &fid_c {
                                vespetrel_storage::repo::list_messages_in_folder(c, fid, 100, 0)
                            } else {
                                vespetrel_storage::repo::list_unified_inbox_messages(c, 100, 0)
                            }
                        })
                        .await;
                    if let Ok(Ok(loaded_msgs)) = msgs {
                        let summaries: Vec<MessageSummary> =
                            loaded_msgs.into_iter().map(|m| m.summary()).collect();
                        let _ = this.update(cx, |view, cx| {
                            view.messages = summaries;
                            view.selected_message_id = view.messages.first().map(|m| m.id.clone());
                            view.reload_folder_counts(cx);
                            cx.notify();
                        });
                    }
                }

                // Incremental background sync if an account and folder match
                if let (Some(acct), Some(fld)) = (acct_opt, folder_obj) {
                    let provider = vespetrel_engine::make_provider(&acct);
                    if let Ok(delta) = provider.sync_messages(&fld, Default::default()).await
                        && !delta.inserted.is_empty()
                    {
                        let new_summaries: Vec<MessageSummary> = delta
                            .inserted
                            .into_iter()
                            .map(|sm| MessageSummary {
                                id: sm
                                    .remote_id
                                    .clone()
                                    .unwrap_or_else(|| sm.remote_uid.to_string()),
                                thread_id: None,
                                subject: Some(format!("Message {}", sm.remote_uid)),
                                from_address: acct.email.clone(),
                                from_name: None,
                                snippet: None,
                                sent_at: chrono::Utc::now(),
                                is_read: sm.flags.contains(&vespetrel_core::message::Flag::Seen),
                                is_flagged: sm
                                    .flags
                                    .contains(&vespetrel_core::message::Flag::Flagged),
                                has_attachments: false,
                            })
                            .collect();
                        let _ = this.update(cx, |view, cx| {
                            view.messages.splice(0..0, new_summaries);
                            cx.notify();
                        });
                    }
                }
            })
            .detach();
        }

        pub fn reload_folder_counts(&mut self, cx: &mut Context<Self>) {
            let pool_opt = self.storage_pool.clone();
            cx.spawn(async move |this, cx| {
                if let Some(pool) = pool_opt
                    && let Ok(conn) = pool.get().await
                {
                    let counts_res = conn
                        .interact(|c| vespetrel_storage::repo::get_folder_counts(c))
                        .await;
                    if let Ok(Ok(stats)) = counts_res {
                        let _ = this.update(cx, |view, cx| {
                            for f in &mut view.folders {
                                if let Some(&(total, unread)) =
                                    stats.get(&f.id).or_else(|| stats.get(&f.remote_id))
                                {
                                    f.total_count = total;
                                    f.unread_count = unread;
                                }
                            }
                            cx.notify();
                        });
                    }
                }
            })
            .detach();
        }

        pub fn delete_account(&mut self, account_id: &str, cx: &mut Context<Self>) {
            let acct_id = account_id.to_string();
            let pool_opt = self.storage_pool.clone();
            self.accounts.retain(|a| a.id != acct_id);
            self.folders.retain(|f| f.account_id != acct_id);
            if self.folders.is_empty() {
                self.selected_folder_id = None;
                self.messages.clear();
                self.selected_message_id = None;
            } else {
                self.selected_folder_id = self.folders.first().map(|f| f.id.clone());
                self.reload_messages_for_current_folder(cx);
            }
            self.show_toast("Account removed", false, cx);
            cx.spawn(async move |_this, _cx| {
                let key = format!("vespetrel_{}", acct_id);
                let _ = tokio::task::spawn_blocking(move || {
                    if let Ok(entry) = keyring::Entry::new("vespetrel", &key) {
                        let _ = entry.delete_credential();
                    }
                })
                .await;
                if let Some(pool) = pool_opt
                    && let Ok(conn) = pool.get().await
                {
                    let _ = conn
                        .interact(move |c| {
                            let _ = vespetrel_storage::repo::delete_account(c, &acct_id);
                        })
                        .await;
                }
            })
            .detach();
        }

        pub fn send_composed_message(&mut self, cx: &mut Context<Self>) {
            let (to_val, subj_val, body_val) = if let Some(inputs) = &self.compose_inputs {
                (
                    inputs.to.read(cx).value().trim().to_string(),
                    inputs.subject.read(cx).value().trim().to_string(),
                    inputs.body.read(cx).value().trim().to_string(),
                )
            } else {
                (
                    self.compose_to.trim().to_string(),
                    self.compose_subject.trim().to_string(),
                    self.compose_body.trim().to_string(),
                )
            };

            if to_val.is_empty() {
                self.show_toast("Please specify a recipient", true, cx);
                return;
            }

            // Auto-harvest recipient to contacts if not already present
            let recipient_email = to_val.clone();
            if !self
                .contacts
                .iter()
                .any(|c| c.email.eq_ignore_ascii_case(&recipient_email))
            {
                let new_contact = Contact {
                    id: format!("cnt-{}", uuid::Uuid::new_v4()),
                    remote_id: None,
                    display_name: None,
                    email: recipient_email.clone(),
                    vcard_data: None,
                };
                self.contacts.push(new_contact.clone());
                let acct_id = self
                    .accounts
                    .first()
                    .map(|a| a.id.clone())
                    .unwrap_or_else(|| "default".into());
                let pool_c = self.storage_pool.clone();
                cx.spawn(async move |_this, _cx| {
                    if let Some(pool) = pool_c
                        && let Ok(conn) = pool.get().await
                    {
                        let _ = conn
                            .interact(move |c| {
                                vespetrel_storage::repo::upsert_contact(c, &acct_id, &new_contact)
                            })
                            .await;
                    }
                })
                .detach();
            }

            let from_email = self
                .accounts
                .first()
                .map(|a| a.email.clone())
                .unwrap_or_else(|| "me@localhost".into());
            let from_name = self.accounts.first().and_then(|a| {
                if a.name.is_empty() {
                    None
                } else {
                    Some(a.name.clone())
                }
            });

            let body_html = if self.compose_is_markdown {
                Some(format!(
                    "<div class=\"markdown-body\">{}</div>",
                    body_val.replace('\n', "<br/>\n")
                ))
            } else {
                None
            };

            let composed = vespetrel_core::ComposedMessage {
                from: vespetrel_core::Address {
                    name: from_name.clone(),
                    email: from_email.clone(),
                },
                to: vec![vespetrel_core::Address {
                    name: None,
                    email: recipient_email.clone(),
                }],
                cc: vec![],
                bcc: vec![],
                subject: subj_val.clone(),
                body_text: body_val.clone(),
                body_html,
                in_reply_to: self.compose_reply_to_id.clone(),
                references: vec![],
                attachments: self.compose_attachments.clone(),
            };

            let outbox_id = format!("outbox-{}", uuid::Uuid::new_v4());
            let now_ts = chrono::Utc::now().timestamp();
            let account_opt = self.accounts.first().cloned();
            let pool_opt = self.storage_pool.clone();
            let to_dest = recipient_email.clone();

            // Persist to SQLite Outbox for durability
            if let Some(pool) = &self.storage_pool {
                let pool = pool.clone();
                let entry = vespetrel_storage::repo::OutboxEntry {
                    id: outbox_id.clone(),
                    account_id: account_opt
                        .as_ref()
                        .map(|a| a.id.clone())
                        .unwrap_or_else(|| "default".into()),
                    composed_message: composed.clone(),
                    scheduled_at: now_ts,
                    send_at: now_ts,
                    is_cancelled: false,
                    attempts: 0,
                    last_error: None,
                };
                cx.spawn(async move |_this, _cx| {
                    if let Ok(conn) = pool.get().await {
                        let _ = conn
                            .interact(move |c| vespetrel_storage::repo::enqueue_outbox(c, &entry))
                            .await;
                    }
                })
                .detach();
            }

            // Also record sent message in Sent folder
            let sent_folder_id = self
                .folders
                .iter()
                .find(|f| f.role == vespetrel_core::FolderRole::Sent)
                .map(|f| f.id.clone());

            if let (Some(sent_fld_id), Some(pool)) = (sent_folder_id, &self.storage_pool) {
                let pool = pool.clone();
                let sent_msg = vespetrel_core::Message {
                    id: format!("sent-{}", uuid::Uuid::new_v4()),
                    account_id: account_opt
                        .as_ref()
                        .map(|a| a.id.clone())
                        .unwrap_or_else(|| "default".into()),
                    folder_id: sent_fld_id,
                    thread_id: None,
                    remote_uid: (chrono::Utc::now().timestamp_millis() & 0x7FFFFFFF) as u32,
                    message_id_header: None,
                    in_reply_to: self.compose_reply_to_id.clone(),
                    references: None,
                    subject: Some(subj_val.clone()),
                    from_address: from_email.clone(),
                    from_name,
                    to_addresses: vec![vespetrel_core::Address {
                        name: None,
                        email: recipient_email.clone(),
                    }],
                    cc_addresses: vec![],
                    bcc_addresses: vec![],
                    reply_to: None,
                    sent_at: chrono::Utc::now(),
                    received_at: chrono::Utc::now(),
                    is_read: true,
                    is_flagged: false,
                    is_draft: false,
                    has_attachments: !self.compose_attachments.is_empty(),
                    body_snippet: Some(body_val.chars().take(120).collect()),
                    body_text_preview: Some(body_val.clone()),
                    blob_path: String::new(),
                    size_bytes: body_val.len() as i64,
                    remote_id: None,
                };
                cx.spawn(async move |this, cx| {
                    if let Ok(conn) = pool.get().await {
                        let _ = conn
                            .interact(move |c| {
                                vespetrel_storage::repo::insert_message(c, &sent_msg)
                            })
                            .await;
                        let _ = this.update(cx, |view, cx| {
                            view.reload_folder_counts(cx);
                        });
                    }
                })
                .detach();
            }

            // Remove draft if this was editing an existing draft
            if let (Some(draft_id), Some(pool)) =
                (self.compose_draft_id.clone(), &self.storage_pool)
            {
                let pool = pool.clone();
                cx.spawn(async move |_this, _cx| {
                    if let Ok(conn) = pool.get().await {
                        let _ = conn
                            .interact(move |c| {
                                vespetrel_storage::repo::delete_message(c, &draft_id)
                            })
                            .await;
                    }
                })
                .detach();
            }

            self.status_message = format!("Sending message to {to_dest}...");
            self.active_modal = ActiveModal::None;
            self.compose_inputs = None;
            self.compose_to.clear();
            self.compose_subject.clear();
            self.compose_body.clear();
            self.compose_attachments.clear();
            self.compose_draft_id = None;
            self.compose_reply_to_id = None;

            self.show_toast_with_undo(
                format!("Message sent to {to_dest}"),
                false,
                Some(outbox_id.clone()),
                cx,
            );
            cx.notify();

            // Asynchronously transmit via SMTP
            let outbox_id_clone = outbox_id.clone();
            cx.spawn(async move |this, cx| {
                let send_res: Result<(), String> = if let Some(acct) = account_opt {
                    let auth_token = if let Some(ref k) = acct.auth_config.keyring_key {
                        keyring::Entry::new("vespetrel", k)
                            .and_then(|e| e.get_password())
                            .unwrap_or_default()
                    } else {
                        keyring::Entry::new("vespetrel", &acct.id)
                            .and_then(|e| e.get_password())
                            .unwrap_or_default()
                    };

                    let domain = acct.email.split('@').nth(1).unwrap_or("localhost");
                    let host = match domain {
                        "gmail.com" => "smtp.gmail.com",
                        "outlook.com" | "hotmail.com" | "live.com" | "office365.com" => {
                            "smtp.office365.com"
                        }
                        "yahoo.com" => "smtp.mail.yahoo.com",
                        "icloud.com" => "smtp.mail.me.com",
                        other => other,
                    };
                    let port = if domain == "gmail.com" || domain == "yahoo.com" {
                        465
                    } else {
                        587
                    };
                    let mut smtp_cfg =
                        vespetrel_smtp::SmtpConfig::new(host, port, &acct.email, auth_token);
                    if acct.provider_type == ProviderType::Gmail {
                        smtp_cfg = smtp_cfg.with_xoauth2();
                    }
                    let client = vespetrel_smtp::SmtpClient::new(smtp_cfg);
                    client.send(&composed).await.map_err(|e| e.to_string())
                } else {
                    Err("No account available for sending".into())
                };

                // Update outbox status in SQLite
                if let Some(pool) = pool_opt {
                    let out_id = outbox_id_clone.clone();
                    let success = send_res.is_ok();
                    let err_copy = send_res.as_ref().err().cloned();
                    if let Ok(conn) = pool.get().await {
                        let _ = conn
                            .interact(move |c| {
                                if success {
                                    let _ =
                                        vespetrel_storage::repo::delete_outbox_entry(c, &out_id);
                                } else if let Some(err) = err_copy {
                                    let _ = vespetrel_storage::repo::mark_outbox_failed(
                                        c, &out_id, &err,
                                    );
                                }
                            })
                            .await;
                    }
                }

                let _ = this.update(cx, |view, cx| {
                    match send_res {
                        Ok(()) => {
                            view.status_message = format!("Message sent successfully to {to_dest}");
                        }
                        Err(e) => {
                            view.status_message = format!("Failed to send message: {e}");
                            view.show_toast(format!("Failed to send to {to_dest}: {e}"), true, cx);
                        }
                    }
                    cx.notify();
                });
            })
            .detach();
        }
    }

    impl Render for MainWindow {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            if self.active_modal == ActiveModal::AddAccount && self.wizard_inputs.is_none() {
                self.wizard_inputs = Some(WizardInputEntities::new(window, cx, &self.login_wizard));
            }
            if self.active_modal == ActiveModal::Compose && self.compose_inputs.is_none() {
                self.compose_inputs = Some(ComposeInputEntities::new(
                    window,
                    cx,
                    &self.compose_to,
                    &self.compose_subject,
                    &self.compose_body,
                ));
            }

            div()
                .flex()
                .flex_col()
                .size_full()
                .bg(rgb(0x0f1117))
                .text_color(rgb(0xe2e8f0))
                .child(self.render_header(cx))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .flex_1()
                        .overflow_hidden()
                        .child(self.render_sidebar_tabs(cx))
                        .child(self.render_active_tab_content(cx)),
                )
                .child(self.render_status_bar())
                .child(self.render_toasts(cx))
                .child(self.render_modal_layer(cx))
        }
    }

    impl MainWindow {
        fn render_toasts(&self, cx: &Context<Self>) -> impl IntoElement {
            div()
                .absolute()
                .bottom(px(36.0))
                .right(px(24.0))
                .flex()
                .flex_col()
                .gap(px(8.0))
                .children(self.toasts.iter().map(|t| {
                    let mut toast_div = div()
                        .id(ElementId::Name(format!("toast-{}", t.id).into()))
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(10.0))
                        .px(px(14.0))
                        .py(px(8.0))
                        .rounded_md()
                        .bg(if t.is_error {
                            rgb(0x450a0a)
                        } else {
                            rgb(0x064e3b)
                        })
                        .border_1()
                        .border_color(if t.is_error {
                            rgb(0xef4444)
                        } else {
                            rgb(0x10b981)
                        })
                        .text_xs()
                        .text_color(if t.is_error {
                            rgb(0xfca5a5)
                        } else {
                            rgb(0xa7f3d0)
                        })
                        .child(div().child(if t.is_error { "⚠️" } else { "✓" }))
                        .child(div().child(t.message.clone()));

                    if let Some(ref outbox_id) = t.undo_outbox_id {
                        let out_id = outbox_id.clone();
                        toast_div = toast_div.child(
                            div()
                                .id(ElementId::Name(format!("undo-{}", t.id).into()))
                                .px(px(6.0))
                                .py(px(2.0))
                                .rounded_md()
                                .bg(rgb(0x047857))
                                .border_1()
                                .border_color(rgb(0x10b981))
                                .text_xs()
                                .font_weight(FontWeight::BOLD)
                                .text_color(rgb(0xffffff))
                                .cursor_pointer()
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.undo_send(&out_id, cx);
                                }))
                                .child("Undo ↩"),
                        );
                    }

                    let toast_id = t.id.clone();
                    toast_div.child(
                        div()
                            .id(ElementId::Name(format!("dismiss-{}", t.id).into()))
                            .cursor_pointer()
                            .text_color(rgb(0x94a3b8))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.dismiss_toast(&toast_id, cx);
                            }))
                            .child("✕"),
                    )
                }))
        }

        fn render_header(&self, cx: &Context<Self>) -> impl IntoElement {
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
                                .child("V"),
                        )
                        .child(
                            div()
                                .font_weight(FontWeight::BOLD)
                                .text_sm()
                                .text_color(rgb(0xf8fafc))
                                .child("Vespetrel Mail"),
                        ),
                )
                .child(
                    div()
                        .id("header-search-bar")
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .w(px(420.0))
                        .h(px(32.0))
                        .px(px(12.0))
                        .rounded_md()
                        .bg(rgb(0x1a202e))
                        .border_1()
                        .border_color(if self.search_query.is_empty() {
                            rgb(0x2d3748)
                        } else {
                            rgb(0x3b82f6)
                        })
                        .gap(px(8.0))
                        .child(
                            div()
                                .id("search-bar-trigger")
                                .flex()
                                .flex_row()
                                .flex_1()
                                .items_center()
                                .gap(px(8.0))
                                .cursor_pointer()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.command_palette.open();
                                    this.active_modal = ActiveModal::CommandPalette;
                                    cx.notify();
                                }))
                                .child(div().text_xs().text_color(rgb(0x94a3b8)).child("🔍"))
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(if self.search_query.is_empty() {
                                            rgb(0x64748b)
                                        } else {
                                            rgb(0x60a5fa)
                                        })
                                        .child(search_display),
                                ),
                        )
                        .children((!self.search_query.is_empty()).then(|| {
                            div()
                                .id("btn-clear-search")
                                .px(px(6.0))
                                .py(px(1.0))
                                .rounded_md()
                                .bg(rgb(0x1e293b))
                                .text_xs()
                                .text_color(rgb(0x94a3b8))
                                .cursor_pointer()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.clear_search(cx);
                                }))
                                .child("✕")
                        })),
                )
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(8.0))
                        .child(
                            div()
                                .id("btn-header-compose")
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
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.active_modal = ActiveModal::Compose;
                                    cx.notify();
                                }))
                                .child("✍️ Compose"),
                        )
                        .child(
                            div()
                                .id("btn-header-sync")
                                .flex()
                                .items_center()
                                .px(px(10.0))
                                .py(px(6.0))
                                .rounded_md()
                                .bg(rgb(0x1e293b))
                                .text_color(rgb(0xcbd5e1))
                                .text_xs()
                                .cursor_pointer()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.trigger_sync(cx);
                                }))
                                .child("🔄 Sync"),
                        )
                        .child(
                            div()
                                .id("btn-header-palette")
                                .flex()
                                .items_center()
                                .px(px(10.0))
                                .py(px(6.0))
                                .rounded_md()
                                .bg(rgb(0x1e293b))
                                .text_color(rgb(0x94a3b8))
                                .text_xs()
                                .cursor_pointer()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.active_modal = ActiveModal::CommandPalette;
                                    cx.notify();
                                }))
                                .child("⌘K"),
                        ),
                )
        }

        fn render_sidebar_tabs(&self, cx: &Context<Self>) -> impl IntoElement {
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
                        .id(ElementId::Name(format!("tab-item-{}", label).into()))
                        .flex()
                        .flex_col()
                        .items_center()
                        .justify_center()
                        .w(px(44.0))
                        .h(px(44.0))
                        .rounded_lg()
                        .bg(if is_active {
                            rgb(0x1e293b)
                        } else {
                            rgb(0x11141c)
                        })
                        .text_color(if is_active {
                            rgb(0x60a5fa)
                        } else {
                            rgb(0x64748b)
                        })
                        .border_1()
                        .border_color(if is_active {
                            rgb(0x3b82f6)
                        } else {
                            rgb(0x00000000)
                        })
                        .cursor_pointer()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.active_tab = tab;
                            cx.notify();
                        }))
                        .child(div().text_base().child(icon))
                        .child(div().text_xs().child(label))
                }))
        }

        fn render_active_tab_content(&self, cx: &Context<Self>) -> impl IntoElement {
            div()
                .flex()
                .flex_row()
                .flex_1()
                .h_full()
                .overflow_hidden()
                .child(match self.active_tab {
                    ActiveViewTab::Mail => self.render_mail_workspace(cx).into_any_element(),
                    ActiveViewTab::Calendar => self.render_calendar_view().into_any_element(),
                    ActiveViewTab::Contacts => self.render_contacts_view().into_any_element(),
                    ActiveViewTab::Tasks => self.render_tasks_view().into_any_element(),
                    ActiveViewTab::Settings => self.render_settings_view(cx).into_any_element(),
                })
        }

        fn render_mail_workspace(&self, cx: &Context<Self>) -> impl IntoElement {
            div()
                .flex()
                .flex_row()
                .flex_1()
                .h_full()
                .overflow_hidden()
                .child(self.render_folder_tree(cx))
                .child(self.render_message_list_pane(cx))
                .child(self.render_message_reader_pane(cx))
        }

        fn render_folder_tree(&self, cx: &Context<Self>) -> impl IntoElement {
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
                        .child(
                            div()
                                .text_xs()
                                .font_weight(FontWeight::BOLD)
                                .text_color(rgb(0x94a3b8))
                                .child("ACCOUNTS & FOLDERS"),
                        )
                        .child(
                            div()
                                .id("btn-add-account")
                                .text_xs()
                                .text_color(rgb(0x60a5fa))
                                .cursor_pointer()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.login_wizard = LoginWizardState::new();
                                    this.wizard_inputs = None;
                                    this.active_modal = ActiveModal::AddAccount;
                                    cx.notify();
                                }))
                                .child("+ Add"),
                        ),
                )
                .children(if self.accounts.is_empty() {
                    vec![
                        div()
                            .id("btn-add-account-banner")
                            .flex()
                            .flex_col()
                            .p(px(8.0))
                            .rounded_md()
                            .bg(rgb(0x181f2f))
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.login_wizard = LoginWizardState::new();
                                this.wizard_inputs = None;
                                this.active_modal = ActiveModal::AddAccount;
                                cx.notify();
                            }))
                            .child(
                                div()
                                    .text_xs()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(rgb(0x94a3b8))
                                    .child("No accounts configured"),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(rgb(0x60a5fa))
                                    .child("Click '+ Add' to connect email"),
                            ),
                    ]
                } else {
                    self.accounts
                        .iter()
                        .enumerate()
                        .map(|(idx, acc)| {
                            let acc_id_clone = acc.id.clone();
                            div()
                                .id(ElementId::Name(format!("account-card-{}", idx).into()))
                                .flex()
                                .flex_row()
                                .items_center()
                                .justify_between()
                                .p(px(8.0))
                                .rounded_md()
                                .bg(rgb(0x181f2f))
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .child(
                                            div()
                                                .text_xs()
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .text_color(rgb(0xf8fafc))
                                                .child(acc.email.clone()),
                                        )
                                        .child(
                                            div()
                                                .text_xs()
                                                .text_color(rgb(0x38bdf8))
                                                .child(format!("{:?} • Active", acc.provider_type)),
                                        ),
                                )
                                .child(
                                    div()
                                        .id(ElementId::Name(
                                            format!("btn-del-account-{}", idx).into(),
                                        ))
                                        .cursor_pointer()
                                        .px(px(6.0))
                                        .py(px(2.0))
                                        .rounded_md()
                                        .bg(rgb(0x2d1515))
                                        .text_xs()
                                        .text_color(rgb(0xf87171))
                                        .hover(|s| s.bg(rgb(0x450a0a)))
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.delete_account(&acc_id_clone, cx);
                                        }))
                                        .child("🗑️"),
                                )
                        })
                        .collect()
                })
                .child(div().flex().flex_col().gap(px(4.0)).children({
                    let mut folder_elements = Vec::new();

                    // Unified Inbox Button
                    let unified_unread: i64 = self
                        .folders
                        .iter()
                        .filter(|f| f.role == FolderRole::Inbox)
                        .map(|f| f.unread_count)
                        .sum();

                    folder_elements.push(
                        div()
                            .id("folder-unified-inbox")
                            .flex()
                            .flex_row()
                            .items_center()
                            .justify_between()
                            .px(px(10.0))
                            .py(px(6.0))
                            .rounded_md()
                            .bg(if self.is_unified_inbox {
                                rgb(0x1e293b)
                            } else {
                                rgb(0x00000000)
                            })
                            .text_color(if self.is_unified_inbox {
                                rgb(0x60a5fa)
                            } else {
                                rgb(0xcbd5e1)
                            })
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.select_unified_inbox(cx);
                            }))
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .gap(px(8.0))
                                    .child(div().text_xs().child("📫"))
                                    .child(
                                        div()
                                            .text_xs()
                                            .font_weight(if self.is_unified_inbox {
                                                FontWeight::BOLD
                                            } else {
                                                FontWeight::NORMAL
                                            })
                                            .child("Unified Inbox"),
                                    ),
                            )
                            .child(if unified_unread > 0 {
                                div()
                                    .px(px(6.0))
                                    .py(px(1.0))
                                    .rounded_full()
                                    .bg(rgb(0x2563eb))
                                    .text_color(rgb(0xffffff))
                                    .text_xs()
                                    .child(format!("{unified_unread}"))
                            } else {
                                div()
                            }),
                    );

                    // Individual Folders
                    let nav_tree = NavigationTree::new(self.folders.clone());
                    let individual_folders = nav_tree
                        .sorted_folders()
                        .into_iter()
                        .map(|f| {
                            let is_selected = !self.is_unified_inbox
                                && (self.selected_folder_id.as_deref() == Some(&f.id)
                                    || self.selected_folder_id.as_deref() == Some(&f.remote_id));
                            let folder_id_clone = f.id.clone();
                            let icon = match f.role {
                                FolderRole::Inbox => "📥",
                                FolderRole::Drafts => "📝",
                                FolderRole::Sent => "📤",
                                FolderRole::Archive => "📦",
                                FolderRole::Junk => "🚫",
                                FolderRole::Trash => "🗑️",
                                FolderRole::Custom => "📁",
                            };
                            let count = f.unread_count;

                            div()
                                .id(ElementId::Name(format!("folder-{}", f.id).into()))
                                .flex()
                                .flex_row()
                                .items_center()
                                .justify_between()
                                .px(px(10.0))
                                .py(px(6.0))
                                .rounded_md()
                                .bg(if is_selected {
                                    rgb(0x1e293b)
                                } else {
                                    rgb(0x00000000)
                                })
                                .text_color(if is_selected {
                                    rgb(0x60a5fa)
                                } else {
                                    rgb(0xcbd5e1)
                                })
                                .cursor_pointer()
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.select_folder(folder_id_clone.clone(), cx);
                                }))
                                .child(
                                    div()
                                        .flex()
                                        .flex_row()
                                        .items_center()
                                        .gap(px(8.0))
                                        .child(div().text_xs().child(icon))
                                        .child(
                                            div()
                                                .text_xs()
                                                .font_weight(if is_selected {
                                                    FontWeight::BOLD
                                                } else {
                                                    FontWeight::NORMAL
                                                })
                                                .child(f.name.clone()),
                                        ),
                                )
                                .child(if count > 0 {
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
                                })
                        })
                        .collect::<Vec<_>>();

                    folder_elements.extend(individual_folders);
                    folder_elements
                }))
        }

        fn render_message_list_pane(&self, cx: &Context<Self>) -> impl IntoElement {
            let visible_rows: Vec<ThreadedMessage<'_>> =
                self.threaded_messages().into_iter().take(100).collect();

            let sort_label = match self.sort_order {
                MessageSortOrder::DateDescending => "↓ Date",
                MessageSortOrder::DateAscending => "↑ Date",
            };
            let density_label = match self.row_density {
                MessageRowDensity::Compact => "Compact",
                MessageRowDensity::Comfortable => "Normal",
                MessageRowDensity::Roomy => "Roomy",
            };

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
                        .flex_col()
                        .border_b_1()
                        .border_color(rgb(0x1f293d))
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .p(px(8.0))
                                .gap(px(4.0))
                                .child(self.render_filter_chip("All", ListFilter::All, cx))
                                .child(self.render_filter_chip("Unread", ListFilter::Unread, cx))
                                .child(self.render_filter_chip("Starred", ListFilter::Flagged, cx))
                                .child(self.render_filter_chip(
                                    "📎 Files",
                                    ListFilter::WithAttachments,
                                    cx,
                                )),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .justify_between()
                                .px(px(8.0))
                                .pb(px(6.0))
                                .text_xs()
                                .child(
                                    div()
                                        .id("btn-toggle-thread")
                                        .px(px(6.0))
                                        .py(px(2.0))
                                        .rounded_md()
                                        .bg(if self.is_threaded {
                                            rgb(0x2563eb)
                                        } else {
                                            rgb(0x1e293b)
                                        })
                                        .text_color(if self.is_threaded {
                                            rgb(0xffffff)
                                        } else {
                                            rgb(0x94a3b8)
                                        })
                                        .cursor_pointer()
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.toggle_threading(cx);
                                        }))
                                        .child("🧵 Threaded"),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .flex_row()
                                        .gap(px(4.0))
                                        .child(
                                            div()
                                                .id("btn-toggle-sort")
                                                .px(px(6.0))
                                                .py(px(2.0))
                                                .rounded_md()
                                                .bg(rgb(0x1e293b))
                                                .text_color(rgb(0x94a3b8))
                                                .cursor_pointer()
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.cycle_sort_order(cx);
                                                }))
                                                .child(sort_label),
                                        )
                                        .child(
                                            div()
                                                .id("btn-toggle-density")
                                                .px(px(6.0))
                                                .py(px(2.0))
                                                .rounded_md()
                                                .bg(rgb(0x1e293b))
                                                .text_color(rgb(0x94a3b8))
                                                .cursor_pointer()
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.cycle_row_density(cx);
                                                }))
                                                .child(density_label),
                                        ),
                                ),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .children(visible_rows.into_iter().map(|row| {
                            let msg = row.summary;
                            let is_selected = self.selected_message_id.as_deref() == Some(&msg.id);
                            let sender = msg.from_name.as_deref().unwrap_or(&msg.from_address);
                            let subject = msg.subject.as_deref().unwrap_or("(No Subject)");
                            let snippet = msg.snippet.as_deref().unwrap_or("");
                            let date_str = msg.sent_at.format("%b %d, %H:%M").to_string();
                            let msg_id = msg.id.clone();
                            let is_child = row.is_child;
                            let thread_count = row.thread_count;

                            let pad_left = if is_child { px(24.0) } else { px(10.0) };

                            let mut item_div = div()
                                .id(ElementId::Name(format!("msg-item-{}", msg_id).into()))
                                .flex()
                                .flex_col()
                                .pl(pad_left)
                                .pr(px(10.0))
                                .py(match self.row_density {
                                    MessageRowDensity::Compact => px(4.0),
                                    MessageRowDensity::Comfortable => px(8.0),
                                    MessageRowDensity::Roomy => px(12.0),
                                })
                                .border_b_1()
                                .border_color(rgb(0x182030))
                                .bg(if is_selected {
                                    rgb(0x1e2a42)
                                } else if !msg.is_read {
                                    rgb(0x141a29)
                                } else {
                                    rgb(0x10141d)
                                })
                                .cursor_pointer()
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.select_message(msg_id.clone(), cx);
                                }));

                            let header_row = div()
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
                                        .child(if is_child {
                                            div().text_xs().text_color(rgb(0x64748b)).child("↳")
                                        } else if !msg.is_read {
                                            div()
                                                .w(px(6.0))
                                                .h(px(6.0))
                                                .rounded_full()
                                                .bg(rgb(0x3b82f6))
                                        } else {
                                            div().w(px(6.0)).h(px(6.0))
                                        })
                                        .child(
                                            div()
                                                .text_xs()
                                                .font_weight(if !msg.is_read {
                                                    FontWeight::BOLD
                                                } else {
                                                    FontWeight::MEDIUM
                                                })
                                                .text_color(rgb(0xf1f5f9))
                                                .child(sender.to_string()),
                                        ),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .flex_row()
                                        .items_center()
                                        .gap(px(6.0))
                                        .child(if thread_count > 1 && !is_child {
                                            div()
                                                .px(px(5.0))
                                                .py(px(1.0))
                                                .rounded_full()
                                                .bg(rgb(0x1e293b))
                                                .text_color(rgb(0x60a5fa))
                                                .text_xs()
                                                .child(format!("🧵 {thread_count}"))
                                        } else {
                                            div()
                                        })
                                        .child(
                                            div()
                                                .text_xs()
                                                .text_color(if msg.is_flagged {
                                                    rgb(0xfbbf24)
                                                } else {
                                                    rgb(0x475569)
                                                })
                                                .child(if msg.is_flagged { "★" } else { "☆" }),
                                        )
                                        .child(
                                            div()
                                                .text_xs()
                                                .text_color(rgb(0x64748b))
                                                .child(date_str),
                                        ),
                                );

                            item_div = item_div.child(header_row);

                            if self.row_density != MessageRowDensity::Compact {
                                item_div = item_div.child(
                                    div()
                                        .pt(px(2.0))
                                        .text_xs()
                                        .font_weight(if !msg.is_read {
                                            FontWeight::SEMIBOLD
                                        } else {
                                            FontWeight::NORMAL
                                        })
                                        .text_color(rgb(0xe2e8f0))
                                        .child(subject.to_string()),
                                );
                            }

                            if self.row_density == MessageRowDensity::Roomy && !snippet.is_empty() {
                                item_div = item_div.child(
                                    div().pt(px(2.0)).text_xs().text_color(rgb(0x94a3b8)).child(
                                        if snippet.len() > 100 {
                                            format!("{}...", &snippet[..100])
                                        } else {
                                            snippet.to_string()
                                        },
                                    ),
                                );
                            } else if self.row_density == MessageRowDensity::Comfortable
                                && !snippet.is_empty()
                            {
                                item_div = item_div.child(
                                    div().pt(px(2.0)).text_xs().text_color(rgb(0x94a3b8)).child(
                                        if snippet.len() > 60 {
                                            format!("{}...", &snippet[..60])
                                        } else {
                                            snippet.to_string()
                                        },
                                    ),
                                );
                            }

                            item_div
                        })),
                )
        }

        fn render_filter_chip(
            &self,
            label: &'static str,
            filter: ListFilter,
            cx: &Context<Self>,
        ) -> impl IntoElement {
            let is_active = self.list_filter == filter;
            div()
                .id(ElementId::Name(format!("filter-chip-{}", label).into()))
                .px(px(8.0))
                .py(px(4.0))
                .rounded_md()
                .bg(if is_active {
                    rgb(0x2563eb)
                } else {
                    rgb(0x1a2233)
                })
                .text_color(if is_active {
                    rgb(0xffffff)
                } else {
                    rgb(0x94a3b8)
                })
                .text_xs()
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.list_filter = filter;
                    cx.notify();
                }))
                .child(label)
        }

        fn render_message_reader_pane(&self, cx: &Context<Self>) -> impl IntoElement {
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
                    let rendered_body = self.message_viewer.rendered();
                    let attachments = &self.message_viewer.attachments;

                    div()
                        .flex()
                        .flex_col()
                        .size_full()
                        // Top toolbar
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
                                        .child(
                                            div()
                                                .id("btn-reply-message")
                                                .px(px(10.0))
                                                .py(px(5.0))
                                                .rounded_md()
                                                .bg(rgb(0x2563eb))
                                                .text_color(rgb(0xffffff))
                                                .text_xs()
                                                .cursor_pointer()
                                                .on_click(cx.listener({
                                                    let reply_to = msg.from_address.clone();
                                                    let reply_subj = if msg
                                                        .subject
                                                        .as_deref()
                                                        .unwrap_or("")
                                                        .to_lowercase()
                                                        .starts_with("re:")
                                                    {
                                                        msg.subject.clone().unwrap_or_default()
                                                    } else {
                                                        format!(
                                                            "Re: {}",
                                                            msg.subject.as_deref().unwrap_or("")
                                                        )
                                                    };
                                                    let msg_id = msg.id.clone();
                                                    move |this, _, _, cx| {
                                                        this.compose_to = reply_to.clone();
                                                        this.compose_subject = reply_subj.clone();
                                                        this.compose_body =
                                                            this.message_viewer.generate_reply_text();
                                                        this.compose_reply_to_id =
                                                            Some(msg_id.clone());
                                                        this.compose_inputs = None;
                                                        this.active_modal = ActiveModal::Compose;
                                                        cx.notify();
                                                    }
                                                }))
                                                .child("↩ Reply"),
                                        )
                                        .child(
                                            div()
                                                .id("btn-reply-all-message")
                                                .px(px(10.0))
                                                .py(px(5.0))
                                                .rounded_md()
                                                .bg(rgb(0x1e293b))
                                                .text_color(rgb(0xcbd5e1))
                                                .text_xs()
                                                .cursor_pointer()
                                                .on_click(cx.listener({
                                                    let reply_to = msg.from_address.clone();
                                                    let reply_subj = if msg
                                                        .subject
                                                        .as_deref()
                                                        .unwrap_or("")
                                                        .to_lowercase()
                                                        .starts_with("re:")
                                                    {
                                                        msg.subject.clone().unwrap_or_default()
                                                    } else {
                                                        format!(
                                                            "Re: {}",
                                                            msg.subject.as_deref().unwrap_or("")
                                                        )
                                                    };
                                                    let msg_id = msg.id.clone();
                                                    move |this, _, _, cx| {
                                                        this.compose_to = reply_to.clone();
                                                        this.compose_subject = reply_subj.clone();
                                                        this.compose_body =
                                                            this.message_viewer.generate_reply_text();
                                                        this.compose_reply_to_id =
                                                            Some(msg_id.clone());
                                                        this.compose_inputs = None;
                                                        this.active_modal = ActiveModal::Compose;
                                                        cx.notify();
                                                    }
                                                }))
                                                .child("👥 Reply All"),
                                        )
                                        .child(
                                            div()
                                                .id("btn-forward-message")
                                                .px(px(10.0))
                                                .py(px(5.0))
                                                .rounded_md()
                                                .bg(rgb(0x1e293b))
                                                .text_color(rgb(0xcbd5e1))
                                                .text_xs()
                                                .cursor_pointer()
                                                .on_click(cx.listener({
                                                    let fwd_subj = if msg
                                                        .subject
                                                        .as_deref()
                                                        .unwrap_or("")
                                                        .to_lowercase()
                                                        .starts_with("fwd:")
                                                    {
                                                        msg.subject.clone().unwrap_or_default()
                                                    } else {
                                                        format!(
                                                            "Fwd: {}",
                                                            msg.subject.as_deref().unwrap_or("")
                                                        )
                                                    };
                                                    let sender = msg.from_address.clone();
                                                    let subj =
                                                        msg.subject.clone().unwrap_or_default();
                                                    let sent_date = msg.sent_at.to_rfc2822();
                                                    move |this, _, _, cx| {
                                                        let plain = this
                                                            .message_viewer
                                                            .plain_text
                                                            .as_deref()
                                                            .unwrap_or("");
                                                        this.compose_to = String::new();
                                                        this.compose_subject = fwd_subj.clone();
                                                        this.compose_body = format!(
                                                            "\n\n---------- Forwarded message ---------\nFrom: {}\nSubject: {}\nDate: {}\n\n{}",
                                                            sender, subj, sent_date, plain
                                                        );
                                                        this.compose_reply_to_id = None;
                                                        this.compose_inputs = None;
                                                        this.active_modal = ActiveModal::Compose;
                                                        cx.notify();
                                                    }
                                                }))
                                                .child("↪ Forward"),
                                        )
                                        .child(
                                            div()
                                                .id("btn-archive-message")
                                                .px(px(10.0))
                                                .py(px(5.0))
                                                .rounded_md()
                                                .bg(rgb(0x1e293b))
                                                .text_color(rgb(0xcbd5e1))
                                                .text_xs()
                                                .cursor_pointer()
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.archive_selected_message(cx);
                                                }))
                                                .child("📦 Archive"),
                                        )
                                        .child(
                                            div()
                                                .id("btn-delete-message")
                                                .px(px(10.0))
                                                .py(px(5.0))
                                                .rounded_md()
                                                .bg(rgb(0x1e293b))
                                                .text_color(rgb(0xf87171))
                                                .text_xs()
                                                .cursor_pointer()
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.delete_selected_message(cx);
                                                }))
                                                .child("🗑️ Delete"),
                                        ),
                                )
                                // Security & Auth Badges
                                .child(
                                    div()
                                        .flex()
                                        .flex_row()
                                        .items_center()
                                        .gap(px(6.0))
                                        .child(
                                            div()
                                                .px(px(8.0))
                                                .py(px(3.0))
                                                .rounded_md()
                                                .bg(rgb(0x064e3b))
                                                .border_1()
                                                .border_color(rgb(0x059669))
                                                .text_color(rgb(0x34d399))
                                                .text_xs()
                                                .child("✓ DKIM Pass"),
                                        )
                                        .child(
                                            div()
                                                .px(px(8.0))
                                                .py(px(3.0))
                                                .rounded_md()
                                                .bg(rgb(0x064e3b))
                                                .border_1()
                                                .border_color(rgb(0x059669))
                                                .text_color(rgb(0x34d399))
                                                .text_xs()
                                                .child("✓ SPF Pass"),
                                        )
                                        .child(
                                            div()
                                                .px(px(8.0))
                                                .py(px(3.0))
                                                .rounded_md()
                                                .bg(rgb(0x1e293b))
                                                .border_1()
                                                .border_color(rgb(0x334155))
                                                .text_color(rgb(0x94a3b8))
                                                .text_xs()
                                                .child(match self.message_viewer.security_status {
                                                    SecurityStatus::PgpSignedValid => "🔒 PGP Signed ✓",
                                                    SecurityStatus::PgpEncryptedAndSigned => "🔒 PGP Encrypted & Signed ✓",
                                                    SecurityStatus::SmimeValid => "🔏 S/MIME Valid (X.509) ✓",
                                                    _ => "🔒 TLS Encrypted",
                                                }),
                                        ),
                                ),
                        )
                        // Subject & Sender info
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
                                        .child(subject.to_string()),
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
                                                        .child(from_name.chars().next().unwrap_or('U').to_string()),
                                                )
                                                .child(
                                                    div()
                                                        .flex()
                                                        .flex_col()
                                                        .child(
                                                            div()
                                                                .text_xs()
                                                                .font_weight(FontWeight::BOLD)
                                                                .text_color(rgb(0xf1f5f9))
                                                                .child(if from_name.is_empty() {
                                                                    from_addr.clone()
                                                                } else {
                                                                    format!("{from_name} <{from_addr}>")
                                                                }),
                                                        )
                                                        .child(
                                                            div()
                                                                .text_xs()
                                                                .text_color(rgb(0x94a3b8))
                                                                .child(format!(
                                                                    "To: {}",
                                                                    self.accounts.first().map(|a| a.email.as_str()).unwrap_or("me")
                                                                )),
                                                        ),
                                                ),
                                        )
                                        .child(div().text_xs().text_color(rgb(0x64748b)).child(date_full)),
                                ),
                        )
                        // Anti-Phishing Security Warning Banner
                        .children(self.message_viewer.phishing_warning.as_ref().map(|warn_msg| {
                            div()
                                .id("banner-phishing-warning")
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap(px(10.0))
                                .px(px(16.0))
                                .py(px(10.0))
                                .bg(rgb(0x450a0a))
                                .border_b_1()
                                .border_color(rgb(0xdc2626))
                                .child(div().text_base().child("⚠️"))
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap(px(2.0))
                                        .child(
                                            div()
                                                .text_xs()
                                                .font_weight(FontWeight::BOLD)
                                                .text_color(rgb(0xfecaca))
                                                .child("PHISHING & SPOOFING ALERT: Suspect Links Detected"),
                                        )
                                        .child(
                                            div()
                                                .text_xs()
                                                .text_color(rgb(0xfca5a5))
                                                .child(warn_msg.clone()),
                                        ),
                                )
                        }))
                        // Tracker & remote image security bar
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
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(rgb(0x94a3b8))
                                        .child(if self.block_remote_images {
                                            "🛡️ Remote images blocked to protect your privacy & stop tracking pixels."
                                        } else {
                                            "⚠️ Remote images allowed for this session."
                                        }),
                                )
                                .child(
                                    div()
                                        .id("btn-toggle-images")
                                        .text_xs()
                                        .text_color(rgb(0x60a5fa))
                                        .cursor_pointer()
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.toggle_remote_images(cx);
                                        }))
                                        .child(if self.block_remote_images {
                                            "Load Remote Images"
                                        } else {
                                            "Block Remote Images"
                                        }),
                                ),
                        )
                        // Attachments tray (if any)
                        .child(if !attachments.is_empty() {
                            div()
                                .flex()
                                .flex_col()
                                .px(px(16.0))
                                .py(px(8.0))
                                .border_b_1()
                                .border_color(rgb(0x1f293d))
                                .bg(rgb(0x121722))
                                .gap(px(6.0))
                                .child(
                                    div()
                                        .text_xs()
                                        .font_weight(FontWeight::BOLD)
                                        .text_color(rgb(0x94a3b8))
                                        .child(format!("📎 Attachments ({})", attachments.len())),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .flex_row()
                                        .flex_wrap()
                                        .gap(px(8.0))
                                        .children(attachments.iter().map(|att| {
                                            let fname = att.filename.clone();
                                            let sz_str = if att.size_bytes >= 1024 * 1024 {
                                                format!("{:.1} MB", att.size_bytes as f64 / (1024.0 * 1024.0))
                                            } else if att.size_bytes >= 1024 {
                                                format!("{:.0} KB", att.size_bytes as f64 / 1024.0)
                                            } else {
                                                format!("{} B", att.size_bytes)
                                            };
                                            div()
                                                .id(ElementId::Name(format!("att-{}", fname).into()))
                                                .flex()
                                                .flex_row()
                                                .items_center()
                                                .gap(px(6.0))
                                                .px(px(8.0))
                                                .py(px(4.0))
                                                .rounded_md()
                                                .bg(rgb(0x1e293b))
                                                .border_1()
                                                .border_color(rgb(0x334155))
                                                .text_xs()
                                                .text_color(rgb(0xcbd5e1))
                                                .child(div().child("📄"))
                                                .child(div().child(fname.clone()))
                                                .child(div().text_color(rgb(0x64748b)).child(format!("({sz_str})")))
                                                .child(
                                                    div()
                                                        .id(ElementId::Name(format!("save-att-{}", fname).into()))
                                                        .cursor_pointer()
                                                        .text_color(rgb(0x60a5fa))
                                                        .on_click(cx.listener({
                                                            let att_name = fname.clone();
                                                            let b_path = att.blob_path.clone();
                                                            move |this, _, _, cx| {
                                                                this.save_attachment_to_downloads(
                                                                    &att_name,
                                                                    Some(b_path.as_str()),
                                                                    cx,
                                                                );
                                                            }
                                                        }))
                                                        .child("💾 Save"),
                                                )
                                        })),
                                )
                        } else {
                            div()
                        })
                        // Clean Body Content
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .flex_1()
                                .p(px(20.0))
                                .gap(px(12.0))
                                .child(
                                    div()
                                        .text_sm()
                                        .text_color(rgb(0xe2e8f0))
                                        .child(rendered_body),
                                )
                                .child(
                                    div()
                                        .pt(px(16.0))
                                        .text_xs()
                                        .text_color(rgb(0x64748b))
                                        .child("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(rgb(0x94a3b8))
                                        .child("Rendered natively with Pure Rust GPUI Engine. Full-text search and offline storage powered by SQLite WAL + FTS5."),
                                ),
                        )
                } else {
                    div()
                        .flex()
                        .items_center()
                        .justify_center()
                        .size_full()
                        .child(
                            div()
                                .text_sm()
                                .text_color(rgb(0x64748b))
                                .child("Select a message to view its contents"),
                        )
                })
        }

        fn render_calendar_view(&self) -> Div {
            let mut cal_view = CalendarView::new();
            cal_view.events = self.calendar_events.clone();

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
                        .child(
                            div()
                                .text_lg()
                                .font_weight(FontWeight::BOLD)
                                .text_color(rgb(0xf8fafc))
                                .child("📅 Calendar (CalDAV RFC 4791 & iCalendar RFC 5545)"),
                        )
                        .child(
                            div()
                                .px(px(12.0))
                                .py(px(6.0))
                                .rounded_md()
                                .bg(rgb(0x2563eb))
                                .text_color(rgb(0xffffff))
                                .text_xs()
                                .child("+ New Event"),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(10.0))
                        .children(cal_view.events.iter().map(|ev| {
                            div()
                                .flex()
                                .flex_col()
                                .p(px(12.0))
                                .rounded_lg()
                                .bg(rgb(0x171c2a))
                                .border_1()
                                .border_color(rgb(0x232c40))
                                .child(
                                    div()
                                        .text_sm()
                                        .font_weight(FontWeight::BOLD)
                                        .text_color(rgb(0x60a5fa))
                                        .child(ev.title.clone()),
                                )
                                .child(div().text_xs().text_color(rgb(0x94a3b8)).child(format!(
                                    "Time: {} - {}",
                                    ev.start.format("%Y-%m-%d %H:%M"),
                                    ev.end.format("%H:%M")
                                )))
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(rgb(0xcbd5e1))
                                        .child(ev.description.clone().unwrap_or_default()),
                                )
                        })),
                )
        }

        fn render_contacts_view(&self) -> Div {
            let mut contacts_view = ContactsView::new(self.contacts.clone());
            if !self.search_query.is_empty() {
                contacts_view.set_search(&self.search_query);
            }
            let filtered = contacts_view.filtered_contacts();

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
                        .child(
                            div()
                                .text_lg()
                                .font_weight(FontWeight::BOLD)
                                .text_color(rgb(0xf8fafc))
                                .child("👥 Address Book (CardDAV & vCard 4.0)"),
                        )
                        .child(
                            div()
                                .px(px(12.0))
                                .py(px(6.0))
                                .rounded_md()
                                .bg(rgb(0x2563eb))
                                .text_color(rgb(0xffffff))
                                .text_xs()
                                .child("+ Add Contact"),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(8.0))
                        .children(filtered.into_iter().map(|c| {
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
                                                .child(
                                                    name.chars().next().unwrap_or('C').to_string(),
                                                ),
                                        )
                                        .child(
                                            div()
                                                .flex()
                                                .flex_col()
                                                .child(
                                                    div()
                                                        .text_xs()
                                                        .font_weight(FontWeight::BOLD)
                                                        .text_color(rgb(0xf8fafc))
                                                        .child(name.to_string()),
                                                )
                                                .child(
                                                    div()
                                                        .text_xs()
                                                        .text_color(rgb(0x94a3b8))
                                                        .child(c.email.clone()),
                                                ),
                                        ),
                                )
                                .child(
                                    div()
                                        .px(px(10.0))
                                        .py(px(4.0))
                                        .rounded_md()
                                        .bg(rgb(0x1e293b))
                                        .text_color(rgb(0x60a5fa))
                                        .text_xs()
                                        .child("Write Email"),
                                )
                        })),
                )
        }

        fn render_tasks_view(&self) -> Div {
            let task_list_view = TaskListView::new(self.tasks.clone());

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
                        .child(
                            div()
                                .text_lg()
                                .font_weight(FontWeight::BOLD)
                                .text_color(rgb(0xf8fafc))
                                .child("✅ Tasks (RFC 5545 VTODO & CalDAV Tasks)"),
                        )
                        .child(
                            div()
                                .px(px(12.0))
                                .py(px(6.0))
                                .rounded_md()
                                .bg(rgb(0x2563eb))
                                .text_color(rgb(0xffffff))
                                .text_xs()
                                .child("+ Add Task"),
                        ),
                )
                .child(div().flex().flex_col().gap(px(8.0)).children(
                    task_list_view.filtered_tasks().into_iter().map(|t| {
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
                                    .child(div().text_sm().child(if is_done {
                                        "☑️"
                                    } else {
                                        "⬜"
                                    }))
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(if is_done {
                                                rgb(0x64748b)
                                            } else {
                                                rgb(0xf1f5f9)
                                            })
                                            .child(t.title.clone()),
                                    ),
                            )
                    }),
                ))
        }

        fn render_settings_view(&self, cx: &Context<Self>) -> Div {
            let themes = [
                (vespetrel_core::ColorTheme::DarkSlate, "Dark Slate"),
                (vespetrel_core::ColorTheme::OledBlack, "OLED Black"),
                (
                    vespetrel_core::ColorTheme::CatppuccinMocha,
                    "Catppuccin Mocha",
                ),
                (vespetrel_core::ColorTheme::LightPaper, "Light Paper"),
                (vespetrel_core::ColorTheme::System, "System Default"),
            ];

            let densities = [
                (vespetrel_core::RowDensity::Compact, "Compact (28px)"),
                (
                    vespetrel_core::RowDensity::Comfortable,
                    "Comfortable (40px)",
                ),
                (vespetrel_core::RowDensity::Roomy, "Roomy (56px)"),
            ];

            let undo_delays = [
                (5, "5 seconds"),
                (10, "10 seconds"),
                (20, "20 seconds"),
                (30, "30 seconds"),
            ];

            div()
                .flex()
                .flex_col()
                .flex_1()
                .p(px(20.0))
                .bg(rgb(0x0f1117))
                .gap(px(16.0))
                .child(
                    div()
                        .text_lg()
                        .font_weight(FontWeight::BOLD)
                        .text_color(rgb(0xf8fafc))
                        .child("⚙️ Configuration & Preferences"),
                )
                // Section 1: Appearance & Theme
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
                        .child(
                            div()
                                .text_sm()
                                .font_weight(FontWeight::BOLD)
                                .text_color(rgb(0x60a5fa))
                                .child("🎨 Appearance & Color Theme"),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .flex_wrap()
                                .gap(px(8.0))
                                .children(themes.into_iter().map(|(th, label)| {
                                    let is_active = self.settings.theme == th;
                                    div()
                                        .id(ElementId::Name(format!("btn-theme-{}", label).into()))
                                        .px(px(12.0))
                                        .py(px(6.0))
                                        .rounded_md()
                                        .bg(if is_active { rgb(0x1e3a8a) } else { rgb(0x0f172a) })
                                        .border_1()
                                        .border_color(if is_active { rgb(0x3b82f6) } else { rgb(0x334155) })
                                        .text_xs()
                                        .text_color(if is_active { rgb(0x93c5fd) } else { rgb(0x94a3b8) })
                                        .font_weight(if is_active { FontWeight::BOLD } else { FontWeight::NORMAL })
                                        .cursor_pointer()
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.set_settings_theme(th, cx);
                                        }))
                                        .child(format!("{}{}", if is_active { "● " } else { "" }, label))
                                })),
                        ),
                )
                // Section 2: Reading & Display Density
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
                        .child(
                            div()
                                .text_sm()
                                .font_weight(FontWeight::BOLD)
                                .text_color(rgb(0x60a5fa))
                                .child("📐 Message List Density"),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .gap(px(8.0))
                                .children(densities.into_iter().map(|(dens, label)| {
                                    let is_active = self.settings.row_density == dens;
                                    div()
                                        .id(ElementId::Name(format!("btn-density-{}", label).into()))
                                        .px(px(12.0))
                                        .py(px(6.0))
                                        .rounded_md()
                                        .bg(if is_active { rgb(0x1e3a8a) } else { rgb(0x0f172a) })
                                        .border_1()
                                        .border_color(if is_active { rgb(0x3b82f6) } else { rgb(0x334155) })
                                        .text_xs()
                                        .text_color(if is_active { rgb(0x93c5fd) } else { rgb(0x94a3b8) })
                                        .font_weight(if is_active { FontWeight::BOLD } else { FontWeight::NORMAL })
                                        .cursor_pointer()
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.set_settings_density(dens, cx);
                                        }))
                                        .child(format!("{}{}", if is_active { "● " } else { "" }, label))
                                })),
                        ),
                )
                // Section 3: Privacy & Security
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .p(px(16.0))
                        .rounded_lg()
                        .bg(rgb(0x171c2a))
                        .border_1()
                        .border_color(rgb(0x232c40))
                        .gap(px(12.0))
                        .child(
                            div()
                                .text_sm()
                                .font_weight(FontWeight::BOLD)
                                .text_color(rgb(0x60a5fa))
                                .child("🛡️ Privacy & Threat Defense"),
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
                                        .flex_col()
                                        .gap(px(2.0))
                                        .child(div().text_xs().font_weight(FontWeight::BOLD).text_color(rgb(0xf1f5f9)).child("Auto-strip Tracking Pixels"))
                                        .child(div().text_xs().text_color(rgb(0x94a3b8)).child("Automatically strip 1x1 tracking GIFs and known telemetry web beacons")),
                                )
                                .child(
                                    div()
                                        .id("btn-toggle-strip-trackers")
                                        .px(px(12.0))
                                        .py(px(4.0))
                                        .rounded_md()
                                        .bg(if self.settings.auto_strip_trackers { rgb(0x064e3b) } else { rgb(0x1e293b) })
                                        .border_1()
                                        .border_color(if self.settings.auto_strip_trackers { rgb(0x10b981) } else { rgb(0x334155) })
                                        .text_xs()
                                        .text_color(if self.settings.auto_strip_trackers { rgb(0xa7f3d0) } else { rgb(0x94a3b8) })
                                        .cursor_pointer()
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.toggle_settings_strip_trackers(cx);
                                        }))
                                        .child(if self.settings.auto_strip_trackers { "✓ Enabled" } else { "✕ Disabled" }),
                                ),
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
                                        .flex_col()
                                        .gap(px(2.0))
                                        .child(div().text_xs().font_weight(FontWeight::BOLD).text_color(rgb(0xf1f5f9)).child("Anti-Phishing & Link Spoofing Warnings"))
                                        .child(div().text_xs().text_color(rgb(0x94a3b8)).child("Detect deceptive display domains, punycode homographs, and IP URLs")),
                                )
                                .child(
                                    div()
                                        .id("btn-toggle-phishing-warnings")
                                        .px(px(12.0))
                                        .py(px(4.0))
                                        .rounded_md()
                                        .bg(if self.settings.warn_on_phishing { rgb(0x064e3b) } else { rgb(0x1e293b) })
                                        .border_1()
                                        .border_color(if self.settings.warn_on_phishing { rgb(0x10b981) } else { rgb(0x334155) })
                                        .text_xs()
                                        .text_color(if self.settings.warn_on_phishing { rgb(0xa7f3d0) } else { rgb(0x94a3b8) })
                                        .cursor_pointer()
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.toggle_settings_warn_phishing(cx);
                                        }))
                                        .child(if self.settings.warn_on_phishing { "✓ Enabled" } else { "✕ Disabled" }),
                                ),
                        ),
                )
                // Section 4: Compose & Outbox
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
                        .child(
                            div()
                                .text_sm()
                                .font_weight(FontWeight::BOLD)
                                .text_color(rgb(0x60a5fa))
                                .child("✉️ Compose & Undo Send Delay"),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .gap(px(8.0))
                                .children(undo_delays.into_iter().map(|(secs, label)| {
                                    let is_active = self.settings.undo_send_seconds == secs;
                                    div()
                                        .id(ElementId::Name(format!("btn-undo-{}", secs).into()))
                                        .px(px(12.0))
                                        .py(px(6.0))
                                        .rounded_md()
                                        .bg(if is_active { rgb(0x1e3a8a) } else { rgb(0x0f172a) })
                                        .border_1()
                                        .border_color(if is_active { rgb(0x3b82f6) } else { rgb(0x334155) })
                                        .text_xs()
                                        .text_color(if is_active { rgb(0x93c5fd) } else { rgb(0x94a3b8) })
                                        .font_weight(if is_active { FontWeight::BOLD } else { FontWeight::NORMAL })
                                        .cursor_pointer()
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.set_settings_undo_seconds(secs, cx);
                                        }))
                                        .child(format!("{}{}", if is_active { "● " } else { "" }, label))
                                })),
                        ),
                )
                // Section 5: Engine & Storage Architecture Details
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .p(px(16.0))
                        .rounded_lg()
                        .bg(rgb(0x171c2a))
                        .border_1()
                        .border_color(rgb(0x232c40))
                        .gap(px(8.0))
                        .child(
                            div()
                                .text_sm()
                                .font_weight(FontWeight::BOLD)
                                .text_color(rgb(0x60a5fa))
                                .child("⚡ Storage & Security Architecture"),
                        )
                        .child(div().text_xs().text_color(rgb(0xcbd5e1)).child(
                            "• Database: SQLite 3 with WAL Mode and Memory-Mapped I/O (256MB)",
                        ))
                        .child(div().text_xs().text_color(rgb(0xcbd5e1)).child(
                            "• Full-Text Search: SQLite FTS5 (unicode61 tokenizer, BM25 ranking)",
                        ))
                        .child(
                            div()
                                .text_xs()
                                .text_color(rgb(0xcbd5e1))
                                .child("• Blob Compression: lz4_flex + zstd"),
                        )
                        .child(
                            div().text_xs().text_color(rgb(0xcbd5e1)).child(
                                "• Keyring Credentials: Native OS Keyring / Credential Manager",
                            ),
                        )
                        .child(div().text_xs().text_color(rgb(0xcbd5e1)).child(
                            "• Crypto & Security: rPGP OpenPGP RFC 9580 + RustCrypto CMS S/MIME",
                        )),
                )
        }

        fn render_status_bar(&self) -> impl IntoElement {
            let is_error_or_offline = self.status_message.starts_with("⚠️")
                || self.status_message.to_lowercase().contains("offline")
                || self.status_message.to_lowercase().contains("error");
            let indicator_color = if is_error_or_offline {
                rgb(0xef4444)
            } else if self.status_message.contains("...")
                || self.status_message.to_lowercase().contains("syncing")
            {
                rgb(0x3b82f6)
            } else {
                rgb(0x10b981)
            };
            let status_indicator_text = if is_error_or_offline {
                "Offline / Sync Error"
            } else {
                "Connected • Direct3D/Vulkan 120 FPS"
            };

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
                        .child(
                            div()
                                .w(px(6.0))
                                .h(px(6.0))
                                .rounded_full()
                                .bg(indicator_color),
                        )
                        .child(
                            div()
                                .text_color(if is_error_or_offline {
                                    rgb(0xfca5a5)
                                } else {
                                    rgb(0x94a3b8)
                                })
                                .child(self.status_message.clone()),
                        ),
                )
                .child(div().child(status_indicator_text))
        }

        fn render_modal_layer(&self, cx: &Context<Self>) -> impl IntoElement {
            match self.active_modal {
                ActiveModal::None => div().into_any_element(),
                ActiveModal::Compose => self.render_compose_modal(cx).into_any_element(),
                ActiveModal::CommandPalette => {
                    self.render_command_palette_modal(cx).into_any_element()
                }
                ActiveModal::AddAccount => self.render_add_account_modal(cx).into_any_element(),
            }
        }

        fn render_compose_modal(&self, cx: &Context<Self>) -> impl IntoElement {
            let to_text = if self.compose_to.is_empty() {
                "user@example.com".to_string()
            } else {
                self.compose_to.clone()
            };
            let subj_text = if self.compose_subject.is_empty() {
                "Message subject...".to_string()
            } else {
                self.compose_subject.clone()
            };
            let body_text = if self.compose_body.is_empty() {
                "Compose email body (Markdown / HTML enabled)...".to_string()
            } else {
                self.compose_body.clone()
            };

            let to_query = if let Some(inputs) = &self.compose_inputs {
                inputs.to.read(cx).value().trim().to_lowercase()
            } else {
                String::new()
            };

            let suggestions: Vec<Contact> = if !to_query.is_empty() && to_query.len() >= 2 {
                self.contacts
                    .iter()
                    .filter(|c| {
                        c.email.to_lowercase().contains(&to_query)
                            || c.display_name
                                .as_deref()
                                .unwrap_or("")
                                .to_lowercase()
                                .contains(&to_query)
                    })
                    .take(4)
                    .cloned()
                    .collect()
            } else {
                Vec::new()
            };

            div()
                .id("modal-compose-overlay")
                .flex()
                .items_center()
                .justify_center()
                .size_full()
                .bg(rgb(0x000000bb))
                .child(
                    div()
                        .id("modal-compose-box")
                        .flex()
                        .flex_col()
                        .w(px(640.0))
                        .h(px(520.0))
                        .rounded_xl()
                        .bg(rgb(0x161b26))
                        .border_1()
                        .border_color(rgb(0x2d3748))
                        .p(px(20.0))
                        .gap(px(10.0))
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .justify_between()
                                .child(
                                    div()
                                        .text_base()
                                        .font_weight(FontWeight::BOLD)
                                        .text_color(rgb(0xf8fafc))
                                        .child("✍️ New Message Composer"),
                                )
                                .child(
                                    div()
                                        .id("btn-close-compose")
                                        .text_sm()
                                        .text_color(rgb(0x94a3b8))
                                        .cursor_pointer()
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.active_modal = ActiveModal::None;
                                            this.compose_inputs = None;
                                            cx.notify();
                                        }))
                                        .child("✕"),
                                ),
                        )
                        // Recipient "To:" Input + Autocomplete Chips
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(4.0))
                                .child(
                                    div()
                                        .flex()
                                        .flex_row()
                                        .items_center()
                                        .gap(px(8.0))
                                        .p(px(8.0))
                                        .rounded_md()
                                        .bg(rgb(0x1c2333))
                                        .child(
                                            div()
                                                .text_xs()
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .text_color(rgb(0x94a3b8))
                                                .child("To:"),
                                        )
                                        .child(if let Some(inputs) = &self.compose_inputs {
                                            component::input::Input::new(&inputs.to)
                                                .cleanable(true)
                                                .into_any_element()
                                        } else {
                                            div()
                                                .text_xs()
                                                .text_color(rgb(0xf1f5f9))
                                                .child(to_text)
                                                .into_any_element()
                                        }),
                                )
                                .child(if !suggestions.is_empty() {
                                    div()
                                        .flex()
                                        .flex_row()
                                        .flex_wrap()
                                        .gap(px(6.0))
                                        .px(px(4.0))
                                        .children(suggestions.into_iter().map(|contact| {
                                            let email_clone = contact.email.clone();
                                            let display = contact
                                                .display_name
                                                .clone()
                                                .unwrap_or_else(|| contact.email.clone());
                                            div()
                                                .id(ElementId::Name(
                                                    format!("chip-contact-{}", contact.id).into(),
                                                ))
                                                .px(px(8.0))
                                                .py(px(2.0))
                                                .rounded_md()
                                                .bg(rgb(0x1e3a8a))
                                                .border_1()
                                                .border_color(rgb(0x3b82f6))
                                                .text_xs()
                                                .text_color(rgb(0x93c5fd))
                                                .cursor_pointer()
                                                .on_click(cx.listener(move |this, _, window, cx| {
                                                    if let Some(inputs) = &this.compose_inputs {
                                                        inputs.to.update(cx, |inp, cx| {
                                                            inp.set_value(
                                                                email_clone.clone(),
                                                                window,
                                                                cx,
                                                            )
                                                        });
                                                    }
                                                    cx.notify();
                                                }))
                                                .child(format!("👤 {display} <{}>", contact.email))
                                        }))
                                } else {
                                    div()
                                }),
                        )
                        // Subject Input
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap(px(8.0))
                                .p(px(8.0))
                                .rounded_md()
                                .bg(rgb(0x1c2333))
                                .child(
                                    div()
                                        .text_xs()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(rgb(0x94a3b8))
                                        .child("Subject:"),
                                )
                                .child(if let Some(inputs) = &self.compose_inputs {
                                    component::input::Input::new(&inputs.subject)
                                        .cleanable(true)
                                        .into_any_element()
                                } else {
                                    div()
                                        .text_xs()
                                        .text_color(rgb(0xf1f5f9))
                                        .child(subj_text)
                                        .into_any_element()
                                }),
                        )
                        // Formatting Toolbar (Markdown, Attachment, Draft)
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .justify_between()
                                .py(px(2.0))
                                .child(
                                    div()
                                        .flex()
                                        .flex_row()
                                        .items_center()
                                        .gap(px(8.0))
                                        .child(
                                            div()
                                                .id("btn-toggle-markdown")
                                                .px(px(8.0))
                                                .py(px(4.0))
                                                .rounded_md()
                                                .bg(if self.compose_is_markdown {
                                                    rgb(0x064e3b)
                                                } else {
                                                    rgb(0x1e293b)
                                                })
                                                .border_1()
                                                .border_color(if self.compose_is_markdown {
                                                    rgb(0x10b981)
                                                } else {
                                                    rgb(0x334155)
                                                })
                                                .text_xs()
                                                .text_color(if self.compose_is_markdown {
                                                    rgb(0x34d399)
                                                } else {
                                                    rgb(0x94a3b8)
                                                })
                                                .cursor_pointer()
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.compose_is_markdown =
                                                        !this.compose_is_markdown;
                                                    cx.notify();
                                                }))
                                                .child(if self.compose_is_markdown {
                                                    "📝 Markdown: ON"
                                                } else {
                                                    "📝 Markdown: OFF"
                                                }),
                                        )
                                        .child(
                                            div()
                                                .id("btn-add-attachment")
                                                .px(px(8.0))
                                                .py(px(4.0))
                                                .rounded_md()
                                                .bg(rgb(0x1e293b))
                                                .border_1()
                                                .border_color(rgb(0x334155))
                                                .text_xs()
                                                .text_color(rgb(0xcbd5e1))
                                                .cursor_pointer()
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    let idx = this.compose_attachments.len() + 1;
                                                    this.compose_attachments.push(
                                                        vespetrel_core::message::ComposedAttachment {
                                                            filename: format!("document_{idx}.pdf"),
                                                            content_type: "application/pdf".into(),
                                                            data: vec![0u8; 1024 * 64],
                                                        },
                                                    );
                                                    this.show_toast(
                                                        format!("Attached document_{idx}.pdf"),
                                                        false,
                                                        cx,
                                                    );
                                                    cx.notify();
                                                }))
                                                .child("📎 Add Attachment"),
                                        ),
                                )
                                .child(
                                    div()
                                        .id("btn-save-draft")
                                        .px(px(8.0))
                                        .py(px(4.0))
                                        .rounded_md()
                                        .bg(rgb(0x1e293b))
                                        .border_1()
                                        .border_color(rgb(0x334155))
                                        .text_xs()
                                        .text_color(rgb(0x38bdf8))
                                        .cursor_pointer()
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.save_draft(cx);
                                        }))
                                        .child("💾 Save Draft"),
                                ),
                        )
                        // Body Input
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .flex_1()
                                .p(px(8.0))
                                .rounded_md()
                                .bg(rgb(0x111622))
                                .border_1()
                                .border_color(rgb(0x232c40))
                                .child(if let Some(inputs) = &self.compose_inputs {
                                    component::input::Input::new(&inputs.body).into_any_element()
                                } else {
                                    div()
                                        .text_xs()
                                        .text_color(rgb(0xe2e8f0))
                                        .child(body_text)
                                        .into_any_element()
                                }),
                        )
                        // Compose Attachment Tray
                        .child(if !self.compose_attachments.is_empty() {
                            div()
                                .flex()
                                .flex_row()
                                .flex_wrap()
                                .gap(px(6.0))
                                .p(px(6.0))
                                .rounded_md()
                                .bg(rgb(0x1a2234))
                                .children(
                                    self.compose_attachments
                                        .iter()
                                        .enumerate()
                                        .map(|(idx, att)| {
                                            let fname = att.filename.clone();
                                            let sz = att.data.len();
                                            let sz_str = format!("{:.1} KB", sz as f64 / 1024.0);
                                            div()
                                                .id(ElementId::Name(
                                                    format!("compose-att-{}", idx).into(),
                                                ))
                                                .flex()
                                                .flex_row()
                                                .items_center()
                                                .gap(px(6.0))
                                                .px(px(8.0))
                                                .py(px(3.0))
                                                .rounded_md()
                                                .bg(rgb(0x0f172a))
                                                .border_1()
                                                .border_color(rgb(0x334155))
                                                .text_xs()
                                                .text_color(rgb(0xcbd5e1))
                                                .child(div().child("📄"))
                                                .child(div().child(fname.clone()))
                                                .child(
                                                    div()
                                                        .text_color(rgb(0x64748b))
                                                        .child(format!("({sz_str})")),
                                                )
                                                .child(
                                                    div()
                                                        .id(ElementId::Name(
                                                            format!("rm-compose-att-{}", idx)
                                                                .into(),
                                                        ))
                                                        .cursor_pointer()
                                                        .text_color(rgb(0xf87171))
                                                        .on_click(cx.listener(
                                                            move |this, _, _, cx| {
                                                                if idx
                                                                    < this
                                                                        .compose_attachments
                                                                        .len()
                                                                {
                                                                    this.compose_attachments
                                                                        .remove(idx);
                                                                    cx.notify();
                                                                }
                                                            },
                                                        ))
                                                        .child("✕"),
                                                )
                                        }),
                                )
                        } else {
                            div()
                        })
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
                                        .child(
                                            div()
                                                .text_xs()
                                                .text_color(rgb(0x34d399))
                                                .child("Autocrypt OpenPGP Signature Attached"),
                                        ),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .flex_row()
                                        .items_center()
                                        .gap(px(8.0))
                                        .child(
                                            div()
                                                .id("btn-discard-compose")
                                                .px(px(14.0))
                                                .py(px(6.0))
                                                .rounded_md()
                                                .bg(rgb(0x1e293b))
                                                .text_color(rgb(0xcbd5e1))
                                                .text_xs()
                                                .cursor_pointer()
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.active_modal = ActiveModal::None;
                                                    this.compose_inputs = None;
                                                    this.compose_attachments.clear();
                                                    this.compose_draft_id = None;
                                                    this.compose_reply_to_id = None;
                                                    cx.notify();
                                                }))
                                                .child("Discard"),
                                        )
                                        .child(
                                            div()
                                                .id("btn-send-compose")
                                                .px(px(16.0))
                                                .py(px(6.0))
                                                .rounded_md()
                                                .bg(rgb(0x2563eb))
                                                .text_color(rgb(0xffffff))
                                                .text_xs()
                                                .font_weight(FontWeight::BOLD)
                                                .cursor_pointer()
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.send_composed_message(cx);
                                                }))
                                                .child("Send 🚀"),
                                        ),
                                ),
                        ),
                )
        }

        fn render_command_palette_modal(&self, cx: &Context<Self>) -> impl IntoElement {
            let actions: Vec<(String, String, Option<String>)> = self
                .command_palette
                .filtered_actions()
                .into_iter()
                .map(|a| (a.id.clone(), a.title.clone(), a.shortcut.clone()))
                .collect();

            div()
                .id("modal-palette-overlay")
                .flex()
                .items_center()
                .justify_center()
                .size_full()
                .bg(rgb(0x000000bb))
                .child(
                    div()
                        .id("modal-palette-box")
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
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(if self.command_palette.query.is_empty() {
                                            rgb(0x94a3b8)
                                        } else {
                                            rgb(0xf1f5f9)
                                        })
                                        .child(if self.command_palette.query.is_empty() {
                                            "Type a command or search action...".to_string()
                                        } else {
                                            self.command_palette.query.clone()
                                        }),
                                ),
                        )
                        .child(div().flex().flex_col().gap(px(4.0)).children(
                            actions.into_iter().enumerate().map(
                                |(idx, (act_id, title, shortcut))| {
                                    let act_id_clone = act_id.clone();
                                    div()
                                        .id(ElementId::Name(
                                            format!("palette-action-{}", idx).into(),
                                        ))
                                        .flex()
                                        .flex_row()
                                        .items_center()
                                        .justify_between()
                                        .px(px(12.0))
                                        .py(px(8.0))
                                        .rounded_md()
                                        .bg(if idx == 0 {
                                            rgb(0x1e293b)
                                        } else {
                                            rgb(0x00000000)
                                        })
                                        .cursor_pointer()
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            match act_id_clone.as_str() {
                                                "mail.compose" => {
                                                    this.active_modal = ActiveModal::Compose;
                                                }
                                                "nav.inbox" => {
                                                    this.active_tab = ActiveViewTab::Mail;
                                                    if let Some(inbox) = this
                                                        .folders
                                                        .iter()
                                                        .find(|f| f.role == FolderRole::Inbox)
                                                    {
                                                        this.selected_folder_id =
                                                            Some(inbox.id.clone());
                                                    }
                                                    this.active_modal = ActiveModal::None;
                                                }
                                                "nav.flagged" => {
                                                    this.active_tab = ActiveViewTab::Mail;
                                                    this.list_filter = ListFilter::Flagged;
                                                    this.active_modal = ActiveModal::None;
                                                }
                                                "mail.sync" => {
                                                    this.active_modal = ActiveModal::None;
                                                    this.trigger_sync(cx);
                                                }
                                                "view.calendar" => {
                                                    this.active_tab = ActiveViewTab::Calendar;
                                                    this.active_modal = ActiveModal::None;
                                                }
                                                "view.contacts" => {
                                                    this.active_tab = ActiveViewTab::Contacts;
                                                    this.active_modal = ActiveModal::None;
                                                }
                                                "view.tasks" => {
                                                    this.active_tab = ActiveViewTab::Tasks;
                                                    this.active_modal = ActiveModal::None;
                                                }
                                                "settings.keybindings" => {
                                                    this.active_tab = ActiveViewTab::Settings;
                                                    this.active_modal = ActiveModal::None;
                                                }
                                                _ => {
                                                    this.active_modal = ActiveModal::None;
                                                }
                                            }
                                            this.command_palette.close();
                                            cx.notify();
                                        }))
                                        .child(
                                            div().text_xs().text_color(rgb(0xf1f5f9)).child(title),
                                        )
                                        .child(
                                            div()
                                                .px(px(6.0))
                                                .py(px(2.0))
                                                .rounded_md()
                                                .bg(rgb(0x111622))
                                                .text_xs()
                                                .text_color(rgb(0x94a3b8))
                                                .child(shortcut.unwrap_or_default()),
                                        )
                                },
                            ),
                        )),
                )
        }

        fn render_add_account_modal(&self, cx: &Context<Self>) -> impl IntoElement {
            let selected_p = self.login_wizard.provider_type.clone();
            let is_oauth = self.login_wizard.auth_mode == AuthModeChoice::OAuth2
                && (selected_p == ProviderType::Gmail || selected_p == ProviderType::Graph);

            let providers = [
                (
                    ProviderType::Gmail,
                    "Google (Gmail)",
                    "OAuth2 PKCE • IMAP/SMTP",
                    "🌐",
                ),
                (
                    ProviderType::Graph,
                    "Microsoft 365",
                    "Graph API • Outlook/Exchange",
                    "🏢",
                ),
                (
                    ProviderType::Jmap,
                    "Fastmail (JMAP)",
                    "RFC 8620 • Ultra-fast Push",
                    "⚡",
                ),
                (
                    ProviderType::Imap,
                    "Custom IMAP",
                    "TLS 993 • SMTP 587 Submission",
                    "🔒",
                ),
            ];

            let display_email = if self.login_wizard.email.is_empty() {
                match selected_p {
                    ProviderType::Gmail => "user@gmail.com",
                    ProviderType::Graph => "user@outlook.com",
                    ProviderType::Jmap => "user@fastmail.com",
                    ProviderType::Imap => "user@example.com",
                }
            } else {
                &self.login_wizard.email
            };

            let display_name = if self.login_wizard.name.is_empty() {
                "Vespetrel User"
            } else {
                &self.login_wizard.name
            };

            let inputs_opt = self.wizard_inputs.as_ref();

            div()
                .id("modal-add-account-overlay")
                .flex()
                .items_center()
                .justify_center()
                .size_full()
                .bg(rgb(0x000000bb))
                .child(
                    div()
                        .id("modal-add-account-box")
                        .flex()
                        .flex_col()
                        .w(px(580.0))
                        .max_h(px(720.0))
                        .overflow_y_scroll()
                        .rounded_xl()
                        .bg(rgb(0x131722))
                        .border_1()
                        .border_color(rgb(0x283347))
                        .p(px(24.0))
                        .gap(px(14.0))
                        // Wizard Header
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .justify_between()
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap(px(2.0))
                                        .child(
                                            div()
                                                .text_base()
                                                .font_weight(FontWeight::BOLD)
                                                .text_color(rgb(0xf8fafc))
                                                .child("✉️ New Mail Setup Wizard"),
                                        )
                                        .child(
                                            div()
                                                .text_xs()
                                                .text_color(rgb(0x94a3b8))
                                                .child("Connect your email inbox to Vespetrel with local SQLite WAL & FTS5"),
                                        ),
                                )
                                .child(
                                    div()
                                        .id("btn-close-wizard")
                                        .text_sm()
                                        .text_color(rgb(0x94a3b8))
                                        .cursor_pointer()
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.active_modal = ActiveModal::None;
                                            this.wizard_inputs = None;
                                            cx.notify();
                                        }))
                                        .child("✕"),
                                ),
                        )
                        // Step 1: Provider Selection Grid
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(8.0))
                                .child(
                                    div()
                                        .text_xs()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(rgb(0xcbd5e1))
                                        .child("1. Choose Provider"),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .flex_row()
                                        .flex_wrap()
                                        .gap(px(8.0))
                                        .children(providers.into_iter().map(|(p, title, desc, icon)| {
                                            let is_sel = selected_p == p;
                                            let p_choice = p.clone();
                                            div()
                                                .id(ElementId::Name(format!("prov-choice-{:?}", p).into()))
                                                .flex()
                                                .flex_col()
                                                .w(px(258.0))
                                                .p(px(10.0))
                                                .rounded_lg()
                                                .bg(if is_sel { rgb(0x1e293b) } else { rgb(0x181f2f) })
                                                .border_1()
                                                .border_color(if is_sel { rgb(0x3b82f6) } else { rgb(0x232d42) })
                                                .cursor_pointer()
                                                .on_click(cx.listener(move |this, _, window, cx| {
                                                    this.login_wizard.select_provider(p_choice.clone());
                                                    if let Some(inputs) = &this.wizard_inputs {
                                                        inputs.incoming_host.update(cx, |inp, cx| inp.set_value(this.login_wizard.incoming_host.clone(), window, cx));
                                                        inputs.incoming_port.update(cx, |inp, cx| inp.set_value(this.login_wizard.incoming_port.to_string(), window, cx));
                                                        inputs.outgoing_host.update(cx, |inp, cx| inp.set_value(this.login_wizard.outgoing_host.clone(), window, cx));
                                                        inputs.outgoing_port.update(cx, |inp, cx| inp.set_value(this.login_wizard.outgoing_port.to_string(), window, cx));
                                                    }
                                                    cx.notify();
                                                }))
                                                .child(
                                                    div()
                                                        .flex()
                                                        .flex_row()
                                                        .items_center()
                                                        .gap(px(6.0))
                                                        .child(div().text_sm().child(icon))
                                                        .child(
                                                            div()
                                                                .text_xs()
                                                                .font_weight(FontWeight::BOLD)
                                                                .text_color(if is_sel { rgb(0x60a5fa) } else { rgb(0xf1f5f9) })
                                                                .child(title),
                                                        ),
                                                )
                                                .child(
                                                    div()
                                                        .text_xs()
                                                        .text_color(rgb(0x64748b))
                                                        .child(desc),
                                                )
                                        })),
                                ),
                        )
                        // Step 2: Authentication & Account Configuration
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(8.0))
                                .child(
                                    div()
                                        .flex()
                                        .flex_row()
                                        .items_center()
                                        .justify_between()
                                        .child(
                                            div()
                                                .text_xs()
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .text_color(rgb(0xcbd5e1))
                                                .child("2. Account Configuration"),
                                        )
                                        .child(
                                            if selected_p == ProviderType::Gmail || selected_p == ProviderType::Graph {
                                                let is_oauth_active = self.login_wizard.auth_mode == AuthModeChoice::OAuth2;
                                                div()
                                                    .flex()
                                                    .flex_row()
                                                    .gap(px(4.0))
                                                    .bg(rgb(0x181f2f))
                                                    .p(px(2.0))
                                                    .rounded_md()
                                                    .child(
                                                        div()
                                                            .id("auth-mode-oauth2")
                                                            .px(px(8.0))
                                                            .py(px(3.0))
                                                            .rounded_sm()
                                                            .bg(if is_oauth_active { rgb(0x2563eb) } else { rgb(0x00000000) })
                                                            .text_xs()
                                                            .font_weight(if is_oauth_active { FontWeight::BOLD } else { FontWeight::NORMAL })
                                                            .text_color(if is_oauth_active { rgb(0xffffff) } else { rgb(0x94a3b8) })
                                                            .cursor_pointer()
                                                            .on_click(cx.listener(|this, _, _, cx| {
                                                                this.login_wizard.auth_mode = AuthModeChoice::OAuth2;
                                                                cx.notify();
                                                            }))
                                                            .child("🌐 Browser OAuth2"),
                                                    )
                                                    .child(
                                                        div()
                                                            .id("auth-mode-password")
                                                            .px(px(8.0))
                                                            .py(px(3.0))
                                                            .rounded_sm()
                                                            .bg(if !is_oauth_active { rgb(0x2563eb) } else { rgb(0x00000000) })
                                                            .text_xs()
                                                            .font_weight(if !is_oauth_active { FontWeight::BOLD } else { FontWeight::NORMAL })
                                                            .text_color(if !is_oauth_active { rgb(0xffffff) } else { rgb(0x94a3b8) })
                                                            .cursor_pointer()
                                                            .on_click(cx.listener(|this, _, _, cx| {
                                                                this.login_wizard.auth_mode = AuthModeChoice::Password;
                                                                cx.notify();
                                                            }))
                                                            .child(if selected_p == ProviderType::Gmail { "🔑 App Password (IMAP)" } else { "🔑 Password / IMAP" }),
                                                    )
                                            } else {
                                                div()
                                            }
                                        ),
                                )
                                .child(
                                    // Conditional form body based on provider and auth_mode
                                    if is_oauth {
                                        // OAuth2 Flow Card
                                        div()
                                            .flex()
                                            .flex_col()
                                            .gap(px(10.0))
                                            .p(px(12.0))
                                            .rounded_lg()
                                            .bg(rgb(0x161c28))
                                            .border_1()
                                            .border_color(rgb(0x232c3f))
                                            .child(
                                                div()
                                                    .flex()
                                                    .flex_row()
                                                    .items_center()
                                                    .gap(px(10.0))
                                                    .child(div().text_lg().child(if selected_p == ProviderType::Gmail { "🌐" } else { "🏢" }))
                                                    .child(
                                                        div()
                                                            .flex()
                                                            .flex_col()
                                                            .gap(px(2.0))
                                                            .child(
                                                                div()
                                                                    .text_xs()
                                                                    .font_weight(FontWeight::BOLD)
                                                                    .text_color(rgb(0xf1f5f9))
                                                                    .child(if selected_p == ProviderType::Gmail { "Google Browser OAuth2 (PKCE)" } else { "Microsoft 365 OAuth2" }),
                                                            )
                                                            .child(
                                                                div()
                                                                    .text_xs()
                                                                    .text_color(rgb(0x94a3b8))
                                                                    .child("Vespetrel will launch your web browser to securely authenticate. No passwords are stored; tokens are kept in the OS keyring."),
                                                            ),
                                                    ),
                                            )
                                            .child(
                                                div()
                                                    .flex()
                                                    .flex_col()
                                                    .gap(px(4.0))
                                                    .child(
                                                        div()
                                                            .flex()
                                                            .flex_row()
                                                            .items_center()
                                                            .justify_between()
                                                            .child(div().text_xs().font_weight(FontWeight::SEMIBOLD).text_color(rgb(0x94a3b8)).child("OAuth Client ID:"))
                                                            .child(div().text_xs().text_color(rgb(0x64748b)).child("Optional (env: VESPETREL_GOOGLE_CLIENT_ID)")),
                                                    )
                                                    .child(if let Some(inputs) = inputs_opt {
                                                        component::input::Input::new(&inputs.client_id).cleanable(true).into_any_element()
                                                    } else {
                                                        div().text_xs().text_color(rgb(0x64748b)).child("OAuth Client ID").into_any_element()
                                                    }),
                                            )
                                            .child(
                                                div()
                                                    .flex()
                                                    .flex_col()
                                                    .gap(px(4.0))
                                                    .child(
                                                        div()
                                                            .flex()
                                                            .flex_row()
                                                            .items_center()
                                                            .justify_between()
                                                            .child(div().text_xs().font_weight(FontWeight::SEMIBOLD).text_color(rgb(0x94a3b8)).child("Email Address (Optional hint):"))
                                                            .child(div().text_xs().text_color(rgb(0x64748b)).child("Auto-discovered from Google if blank")),
                                                    )
                                                    .child(if let Some(inputs) = inputs_opt {
                                                        component::input::Input::new(&inputs.email).cleanable(true).into_any_element()
                                                    } else {
                                                        div().text_xs().text_color(rgb(0x64748b)).child("user@gmail.com").into_any_element()
                                                    }),
                                            )
                                            .child(
                                                if let Some(status_str) = &self.login_wizard.oauth_status {
                                                    div()
                                                        .flex()
                                                        .p(px(8.0))
                                                        .rounded_md()
                                                        .bg(rgb(0x1e3a8a))
                                                        .border_1()
                                                        .border_color(rgb(0x3b82f6))
                                                        .text_xs()
                                                        .text_color(rgb(0x93c5fd))
                                                        .child(format!("⏳ {}", status_str))
                                                } else {
                                                    div()
                                                },
                                            )
                                    } else if selected_p == ProviderType::Gmail {
                                        // Gmail with App Password (IMAP) Form
                                        div()
                                            .flex()
                                            .flex_col()
                                            .gap(px(8.0))
                                            .p(px(12.0))
                                            .rounded_lg()
                                            .bg(rgb(0x161c28))
                                            .border_1()
                                            .border_color(rgb(0x232c3f))
                                            .child(
                                                div()
                                                    .flex()
                                                    .p(px(8.0))
                                                    .rounded_md()
                                                    .bg(rgb(0x1e293b))
                                                    .border_1()
                                                    .border_color(rgb(0x334155))
                                                    .text_xs()
                                                    .text_color(rgb(0x38bdf8))
                                                    .child("💡 Google App Password: In your Google Account, go to Security → 2-Step Verification → App passwords. Generate a 16-character password and enter it below."),
                                            )
                                            .child(
                                                div()
                                                    .flex()
                                                    .flex_col()
                                                    .gap(px(4.0))
                                                    .child(div().text_xs().font_weight(FontWeight::SEMIBOLD).text_color(rgb(0x94a3b8)).child("Gmail Address:"))
                                                    .child(if let Some(inputs) = inputs_opt {
                                                        component::input::Input::new(&inputs.email).cleanable(true).into_any_element()
                                                    } else {
                                                        div().text_xs().text_color(rgb(0xe2e8f0)).child(display_email.to_string()).into_any_element()
                                                    }),
                                            )
                                            .child(
                                                div()
                                                    .flex()
                                                    .flex_col()
                                                    .gap(px(4.0))
                                                    .child(div().text_xs().font_weight(FontWeight::SEMIBOLD).text_color(rgb(0x94a3b8)).child("16-character Google App Password:"))
                                                    .child(if let Some(inputs) = inputs_opt {
                                                        component::input::Input::new(&inputs.password).mask_toggle().cleanable(true).into_any_element()
                                                    } else {
                                                        div().text_xs().text_color(rgb(0x64748b)).child("••••••••••••••••").into_any_element()
                                                    }),
                                            )
                                            .child(
                                                div()
                                                    .flex()
                                                    .flex_col()
                                                    .gap(px(4.0))
                                                    .child(div().text_xs().font_weight(FontWeight::SEMIBOLD).text_color(rgb(0x94a3b8)).child("Display Name (optional):"))
                                                    .child(if let Some(inputs) = inputs_opt {
                                                        component::input::Input::new(&inputs.name).cleanable(true).into_any_element()
                                                    } else {
                                                        div().text_xs().text_color(rgb(0xe2e8f0)).child(display_name.to_string()).into_any_element()
                                                    }),
                                            )
                                            .child(
                                                div()
                                                    .flex()
                                                    .flex_row()
                                                    .items_center()
                                                    .justify_between()
                                                    .text_xs()
                                                    .text_color(rgb(0x64748b))
                                                    .child("Incoming: imap.gmail.com:993 (SSL/TLS)")
                                                    .child("Outgoing: smtp.gmail.com:587 (STARTTLS)"),
                                            )
                                    } else if selected_p == ProviderType::Imap {
                                        // Custom IMAP Form
                                        div()
                                            .flex()
                                            .flex_col()
                                            .gap(px(8.0))
                                            .p(px(12.0))
                                            .rounded_lg()
                                            .bg(rgb(0x161c28))
                                            .border_1()
                                            .border_color(rgb(0x232c3f))
                                            .child(
                                                div()
                                                    .flex()
                                                    .flex_col()
                                                    .gap(px(4.0))
                                                    .child(div().text_xs().font_weight(FontWeight::SEMIBOLD).text_color(rgb(0x94a3b8)).child("Email Address:"))
                                                    .child(if let Some(inputs) = inputs_opt {
                                                        component::input::Input::new(&inputs.email).cleanable(true).into_any_element()
                                                    } else {
                                                        div().text_xs().text_color(rgb(0xe2e8f0)).child(display_email.to_string()).into_any_element()
                                                    }),
                                            )
                                            .child(
                                                div()
                                                    .flex()
                                                    .flex_col()
                                                    .gap(px(4.0))
                                                    .child(div().text_xs().font_weight(FontWeight::SEMIBOLD).text_color(rgb(0x94a3b8)).child("Password / Token:"))
                                                    .child(if let Some(inputs) = inputs_opt {
                                                        component::input::Input::new(&inputs.password).mask_toggle().cleanable(true).into_any_element()
                                                    } else {
                                                        div().text_xs().text_color(rgb(0x64748b)).child("••••••••••••••••").into_any_element()
                                                    }),
                                            )
                                            .child(
                                                div()
                                                    .flex()
                                                    .flex_col()
                                                    .gap(px(4.0))
                                                    .child(div().text_xs().font_weight(FontWeight::SEMIBOLD).text_color(rgb(0x94a3b8)).child("Display Name (optional):"))
                                                    .child(if let Some(inputs) = inputs_opt {
                                                        component::input::Input::new(&inputs.name).cleanable(true).into_any_element()
                                                    } else {
                                                        div().text_xs().text_color(rgb(0xe2e8f0)).child(display_name.to_string()).into_any_element()
                                                    }),
                                            )
                                            .child(
                                                div()
                                                    .flex()
                                                    .flex_row()
                                                    .gap(px(8.0))
                                                    .child(
                                                        div()
                                                            .flex()
                                                            .flex_col()
                                                            .flex_1()
                                                            .gap(px(4.0))
                                                            .child(div().text_xs().font_weight(FontWeight::SEMIBOLD).text_color(rgb(0x94a3b8)).child("Incoming IMAP Host:"))
                                                            .child(if let Some(inputs) = inputs_opt {
                                                                component::input::Input::new(&inputs.incoming_host).cleanable(true).into_any_element()
                                                            } else {
                                                                div().text_xs().text_color(rgb(0x64748b)).child("imap.example.com").into_any_element()
                                                            }),
                                                    )
                                                    .child(
                                                        div()
                                                            .flex()
                                                            .flex_col()
                                                            .w(px(80.0))
                                                            .gap(px(4.0))
                                                            .child(div().text_xs().font_weight(FontWeight::SEMIBOLD).text_color(rgb(0x94a3b8)).child("Port:"))
                                                            .child(if let Some(inputs) = inputs_opt {
                                                                component::input::Input::new(&inputs.incoming_port).into_any_element()
                                                            } else {
                                                                div().text_xs().text_color(rgb(0x64748b)).child("993").into_any_element()
                                                            }),
                                                    ),
                                            )
                                            .child(
                                                div()
                                                    .flex()
                                                    .flex_row()
                                                    .gap(px(8.0))
                                                    .child(
                                                        div()
                                                            .flex()
                                                            .flex_col()
                                                            .flex_1()
                                                            .gap(px(4.0))
                                                            .child(div().text_xs().font_weight(FontWeight::SEMIBOLD).text_color(rgb(0x94a3b8)).child("Outgoing SMTP Host:"))
                                                            .child(if let Some(inputs) = inputs_opt {
                                                                component::input::Input::new(&inputs.outgoing_host).cleanable(true).into_any_element()
                                                            } else {
                                                                div().text_xs().text_color(rgb(0x64748b)).child("smtp.example.com").into_any_element()
                                                            }),
                                                    )
                                                    .child(
                                                        div()
                                                            .flex()
                                                            .flex_col()
                                                            .w(px(80.0))
                                                            .gap(px(4.0))
                                                            .child(div().text_xs().font_weight(FontWeight::SEMIBOLD).text_color(rgb(0x94a3b8)).child("Port:"))
                                                            .child(if let Some(inputs) = inputs_opt {
                                                                component::input::Input::new(&inputs.outgoing_port).into_any_element()
                                                            } else {
                                                                div().text_xs().text_color(rgb(0x64748b)).child("587").into_any_element()
                                                            }),
                                                    ),
                                            )
                                    } else {
                                        // Fastmail JMAP / Graph Password Form
                                        div()
                                            .flex()
                                            .flex_col()
                                            .gap(px(8.0))
                                            .p(px(12.0))
                                            .rounded_lg()
                                            .bg(rgb(0x161c28))
                                            .border_1()
                                            .border_color(rgb(0x232c3f))
                                            .child(
                                                div()
                                                    .flex()
                                                    .flex_col()
                                                    .gap(px(4.0))
                                                    .child(div().text_xs().font_weight(FontWeight::SEMIBOLD).text_color(rgb(0x94a3b8)).child("Email Address:"))
                                                    .child(if let Some(inputs) = inputs_opt {
                                                        component::input::Input::new(&inputs.email).cleanable(true).into_any_element()
                                                    } else {
                                                        div().text_xs().text_color(rgb(0xe2e8f0)).child(display_email.to_string()).into_any_element()
                                                    }),
                                            )
                                            .child(
                                                div()
                                                    .flex()
                                                    .flex_col()
                                                    .gap(px(4.0))
                                                    .child(div().text_xs().font_weight(FontWeight::SEMIBOLD).text_color(rgb(0x94a3b8)).child("Password / API Token:"))
                                                    .child(if let Some(inputs) = inputs_opt {
                                                        component::input::Input::new(&inputs.password).mask_toggle().cleanable(true).into_any_element()
                                                    } else {
                                                        div().text_xs().text_color(rgb(0x64748b)).child("••••••••••••••••").into_any_element()
                                                    }),
                                            )
                                            .child(
                                                div()
                                                    .flex()
                                                    .flex_col()
                                                    .gap(px(4.0))
                                                    .child(div().text_xs().font_weight(FontWeight::SEMIBOLD).text_color(rgb(0x94a3b8)).child("Server Host:"))
                                                    .child(if let Some(inputs) = inputs_opt {
                                                        component::input::Input::new(&inputs.incoming_host).cleanable(true).into_any_element()
                                                    } else {
                                                        div().text_xs().text_color(rgb(0x64748b)).child("api.fastmail.com").into_any_element()
                                                    }),
                                            )
                                    }
                                ),
                        )
                        // Preset Quick-Select Chips
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(4.0))
                                .child(div().text_xs().text_color(rgb(0x64748b)).child("Quick presets:"))
                                .child(
                                    div()
                                        .flex()
                                        .flex_row()
                                        .gap(px(6.0))
                                        .child(
                                            div()
                                                .id("chip-personal")
                                                .px(px(8.0))
                                                .py(px(4.0))
                                                .rounded_md()
                                                .bg(rgb(0x1e293b))
                                                .text_xs()
                                                .text_color(rgb(0x94a3b8))
                                                .cursor_pointer()
                                                .on_click(cx.listener(move |this, _, window, cx| {
                                                    let suffix = match this.login_wizard.provider_type {
                                                        ProviderType::Gmail => "gmail.com",
                                                        ProviderType::Graph => "outlook.com",
                                                        ProviderType::Jmap => "fastmail.com",
                                                        ProviderType::Imap => "example.com",
                                                    };
                                                    let em = format!("personal@{}", suffix);
                                                    let nm = "Personal Account".to_string();
                                                    this.login_wizard.email = em.clone();
                                                    this.login_wizard.name = nm.clone();
                                                    this.login_wizard.password_or_token = String::new();
                                                    this.login_wizard.apply_autodiscover_for_email(&em);
                                                    if let Some(inputs) = &this.wizard_inputs {
                                                        inputs.email.update(cx, |inp, cx| inp.set_value(em, window, cx));
                                                        inputs.name.update(cx, |inp, cx| inp.set_value(nm, window, cx));
                                                        inputs.password.update(cx, |inp, cx| inp.set_value(String::new(), window, cx));
                                                        inputs.incoming_host.update(cx, |inp, cx| inp.set_value(this.login_wizard.incoming_host.clone(), window, cx));
                                                        inputs.incoming_port.update(cx, |inp, cx| inp.set_value(this.login_wizard.incoming_port.to_string(), window, cx));
                                                        inputs.outgoing_host.update(cx, |inp, cx| inp.set_value(this.login_wizard.outgoing_host.clone(), window, cx));
                                                        inputs.outgoing_port.update(cx, |inp, cx| inp.set_value(this.login_wizard.outgoing_port.to_string(), window, cx));
                                                    }
                                                    cx.notify();
                                                }))
                                                .child("Personal"),
                                        )
                                        .child(
                                            div()
                                                .id("chip-work")
                                                .px(px(8.0))
                                                .py(px(4.0))
                                                .rounded_md()
                                                .bg(rgb(0x1e293b))
                                                .text_xs()
                                                .text_color(rgb(0x94a3b8))
                                                .cursor_pointer()
                                                .on_click(cx.listener(move |this, _, window, cx| {
                                                    let suffix = match this.login_wizard.provider_type {
                                                        ProviderType::Gmail => "gmail.com",
                                                        ProviderType::Graph => "company.onmicrosoft.com",
                                                        ProviderType::Jmap => "fastmail.fm",
                                                        ProviderType::Imap => "corp.example.com",
                                                    };
                                                    let em = format!("work@{}", suffix);
                                                    let nm = "Work Mailbox".to_string();
                                                    this.login_wizard.email = em.clone();
                                                    this.login_wizard.name = nm.clone();
                                                    this.login_wizard.password_or_token = String::new();
                                                    this.login_wizard.apply_autodiscover_for_email(&em);
                                                    if let Some(inputs) = &this.wizard_inputs {
                                                        inputs.email.update(cx, |inp, cx| inp.set_value(em, window, cx));
                                                        inputs.name.update(cx, |inp, cx| inp.set_value(nm, window, cx));
                                                        inputs.password.update(cx, |inp, cx| inp.set_value(String::new(), window, cx));
                                                        inputs.incoming_host.update(cx, |inp, cx| inp.set_value(this.login_wizard.incoming_host.clone(), window, cx));
                                                        inputs.incoming_port.update(cx, |inp, cx| inp.set_value(this.login_wizard.incoming_port.to_string(), window, cx));
                                                        inputs.outgoing_host.update(cx, |inp, cx| inp.set_value(this.login_wizard.outgoing_host.clone(), window, cx));
                                                        inputs.outgoing_port.update(cx, |inp, cx| inp.set_value(this.login_wizard.outgoing_port.to_string(), window, cx));
                                                    }
                                                    cx.notify();
                                                }))
                                                .child("Work"),
                                        )
                                        .child(
                                            div()
                                                .id("chip-support")
                                                .px(px(8.0))
                                                .py(px(4.0))
                                                .rounded_md()
                                                .bg(rgb(0x1e293b))
                                                .text_xs()
                                                .text_color(rgb(0x94a3b8))
                                                .cursor_pointer()
                                                .on_click(cx.listener(move |this, _, window, cx| {
                                                    let suffix = match this.login_wizard.provider_type {
                                                        ProviderType::Gmail => "gmail.com",
                                                        ProviderType::Graph => "outlook.com",
                                                        ProviderType::Jmap => "fastmail.com",
                                                        ProviderType::Imap => "vespetrel.org",
                                                    };
                                                    let em = format!("team@{}", suffix);
                                                    let nm = "Vespetrel Team".to_string();
                                                    this.login_wizard.email = em.clone();
                                                    this.login_wizard.name = nm.clone();
                                                    this.login_wizard.password_or_token = String::new();
                                                    this.login_wizard.apply_autodiscover_for_email(&em);
                                                    if let Some(inputs) = &this.wizard_inputs {
                                                        inputs.email.update(cx, |inp, cx| inp.set_value(em, window, cx));
                                                        inputs.name.update(cx, |inp, cx| inp.set_value(nm, window, cx));
                                                        inputs.password.update(cx, |inp, cx| inp.set_value(String::new(), window, cx));
                                                        inputs.incoming_host.update(cx, |inp, cx| inp.set_value(this.login_wizard.incoming_host.clone(), window, cx));
                                                        inputs.incoming_port.update(cx, |inp, cx| inp.set_value(this.login_wizard.incoming_port.to_string(), window, cx));
                                                        inputs.outgoing_host.update(cx, |inp, cx| inp.set_value(this.login_wizard.outgoing_host.clone(), window, cx));
                                                        inputs.outgoing_port.update(cx, |inp, cx| inp.set_value(this.login_wizard.outgoing_port.to_string(), window, cx));
                                                    }
                                                    cx.notify();
                                                }))
                                                .child("Team / Support"),
                                        ),
                                ),
                        )
                        // Error message if any
                        .child(if let WizardStep::Failed(err) = &self.login_wizard.step {
                            div()
                                .flex()
                                .p(px(8.0))
                                .rounded_md()
                                .bg(rgb(0x450a0a))
                                .border_1()
                                .border_color(rgb(0xef4444))
                                .text_xs()
                                .text_color(rgb(0xfca5a5))
                                .child(format!("⚠️ {}", err))
                        } else {
                            div()
                        })
                        // Modal Actions Footer
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .justify_end()
                                .gap(px(10.0))
                                .pt(px(8.0))
                                .border_t_1()
                                .border_color(rgb(0x1f293d))
                                .child(
                                    div()
                                        .id("wizard-btn-cancel")
                                        .px(px(14.0))
                                        .py(px(7.0))
                                        .rounded_md()
                                        .bg(rgb(0x1e293b))
                                        .text_xs()
                                        .text_color(rgb(0xcbd5e1))
                                        .cursor_pointer()
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.active_modal = ActiveModal::None;
                                            this.wizard_inputs = None;
                                            cx.notify();
                                        }))
                                        .child("Cancel"),
                                )
                                .child(
                                    if is_oauth {
                                        let btn_label = match selected_p {
                                            ProviderType::Gmail => "Sign in with Google (Browser) 🌐",
                                            ProviderType::Graph => "Sign in with Microsoft (Browser) 🏢",
                                            _ => "Sign in with Browser 🌐",
                                        };
                                        div()
                                            .id("wizard-btn-oauth")
                                            .px(px(18.0))
                                            .py(px(7.0))
                                            .rounded_md()
                                            .bg(rgb(0x2563eb))
                                            .text_xs()
                                            .font_weight(FontWeight::BOLD)
                                            .text_color(rgb(0xffffff))
                                            .cursor_pointer()
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.start_oauth2_flow(window, cx);
                                            }))
                                            .child(btn_label)
                                    } else {
                                        div()
                                            .id("wizard-btn-connect")
                                            .px(px(18.0))
                                            .py(px(7.0))
                                            .rounded_md()
                                            .bg(rgb(0x2563eb))
                                            .text_xs()
                                            .font_weight(FontWeight::BOLD)
                                            .text_color(rgb(0xffffff))
                                            .cursor_pointer()
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                let (email, password, name, mut in_host, mut in_port, mut out_host, mut out_port) = if let Some(inputs) = &this.wizard_inputs {
                                                    (
                                                        inputs.email.read(cx).value().trim().to_string(),
                                                        inputs.password.read(cx).value().trim().to_string(),
                                                        inputs.name.read(cx).value().trim().to_string(),
                                                        inputs.incoming_host.read(cx).value().trim().to_string(),
                                                        inputs.incoming_port.read(cx).value().trim().parse::<u16>().unwrap_or(993),
                                                        inputs.outgoing_host.read(cx).value().trim().to_string(),
                                                        inputs.outgoing_port.read(cx).value().trim().parse::<u16>().unwrap_or(587),
                                                    )
                                                } else {
                                                    (
                                                        this.login_wizard.email.trim().to_string(),
                                                        this.login_wizard.password_or_token.trim().to_string(),
                                                        this.login_wizard.name.trim().to_string(),
                                                        this.login_wizard.incoming_host.trim().to_string(),
                                                        this.login_wizard.incoming_port,
                                                        this.login_wizard.outgoing_host.trim().to_string(),
                                                        this.login_wizard.outgoing_port,
                                                    )
                                                };

                                                if email.is_empty() || !email.contains('@') {
                                                    this.login_wizard.step = WizardStep::Failed("Please provide a valid email address".into());
                                                    this.show_toast("Please provide a valid email address", true, cx);
                                                    cx.notify();
                                                    return;
                                                }
                                                if password.is_empty() {
                                                    this.login_wizard.step = WizardStep::Failed("Password or authentication token cannot be empty".into());
                                                    this.show_toast("Password or authentication token cannot be empty", true, cx);
                                                    cx.notify();
                                                    return;
                                                }

                                                // Autodiscover host & ports if left blank
                                                if in_host.is_empty() || out_host.is_empty() {
                                                    this.login_wizard.apply_autodiscover_for_email(&email);
                                                    if in_host.is_empty() {
                                                        in_host = this.login_wizard.incoming_host.clone();
                                                        in_port = this.login_wizard.incoming_port;
                                                    }
                                                    if out_host.is_empty() {
                                                        out_host = this.login_wizard.outgoing_host.clone();
                                                        out_port = this.login_wizard.outgoing_port;
                                                    }
                                                }

                                                this.login_wizard.email = email.clone();
                                                this.login_wizard.password_or_token = password.clone();
                                                this.login_wizard.name = name;
                                                this.login_wizard.incoming_host = in_host;
                                                this.login_wizard.incoming_port = in_port;
                                                this.login_wizard.outgoing_host = out_host;
                                                this.login_wizard.outgoing_port = out_port;

                                                let acct = match this.login_wizard.validate_and_build_account() {
                                                    Ok(a) => a,
                                                    Err(e) => {
                                                        this.show_toast(format!("Configuration error: {e}"), true, cx);
                                                        this.login_wizard.step = WizardStep::Failed(e);
                                                        cx.notify();
                                                        return;
                                                    }
                                                };

                                                this.login_wizard.step = WizardStep::Validating;
                                                this.status_message = format!("Connecting to {}...", acct.email);
                                                cx.notify();

                                                // Persist credentials to native OS keyring
                                                if let Some(ref k) = acct.auth_config.keyring_key
                                                    && let Ok(entry) = keyring::Entry::new("vespetrel", k)
                                                {
                                                    let _ = entry.set_password(&password);
                                                }

                                                let pool_opt = this.storage_pool.clone();
                                                let acct_clone = acct.clone();
                                                let password_clone = password.clone();
                                                let sync_sender = this.sync_sender.clone();

                                                cx.spawn(async move |this, cx| {
                                                    let provider = vespetrel_engine::coordinator::make_provider_with_token(&acct_clone, password_clone);
                                                    let folders_res = provider.sync_folder_list().await;
                                                    match folders_res {
                                                        Ok(remote_folders) => {
                                                            let mut local_folders = Vec::new();
                                                            if remote_folders.is_empty() {
                                                                local_folders.push(Folder::new(&acct_clone.id, "INBOX", "INBOX", "INBOX").with_role(FolderRole::Inbox));
                                                                local_folders.push(Folder::new(&acct_clone.id, "Drafts", "Drafts", "Drafts").with_role(FolderRole::Drafts));
                                                                local_folders.push(Folder::new(&acct_clone.id, "Sent", "Sent", "Sent").with_role(FolderRole::Sent));
                                                                local_folders.push(Folder::new(&acct_clone.id, "Trash", "Trash", "Trash").with_role(FolderRole::Trash));
                                                            } else {
                                                                for rf in &remote_folders {
                                                                    let role = match rf.role_hint.as_deref().unwrap_or("") {
                                                                        "inbox" | "Inbox" => FolderRole::Inbox,
                                                                        "drafts" | "Drafts" => FolderRole::Drafts,
                                                                        "sent" | "Sent" => FolderRole::Sent,
                                                                        "trash" | "Trash" => FolderRole::Trash,
                                                                        "archive" | "Archive" => FolderRole::Archive,
                                                                        "spam" | "Spam" | "junk" | "Junk" => FolderRole::Junk,
                                                                        _ => FolderRole::Custom,
                                                                    };
                                                                    let mut f = Folder::new(&acct_clone.id, &rf.remote_id, &rf.name, &rf.path).with_role(role);
                                                                    f.uid_validity = rf.uid_validity;
                                                                    f.highest_mod_seq = rf.highest_mod_seq;
                                                                    local_folders.push(f);
                                                                }
                                                            }

                                                            if let Some(pool) = pool_opt {
                                                                let acct_save = acct_clone.clone();
                                                                let f_save = local_folders.clone();
                                                                if let Ok(conn) = pool.get().await {
                                                                    let _ = conn.interact(move |c| {
                                                                        let _ = vespetrel_storage::repo::upsert_account(c, &acct_save);
                                                                        for f in &f_save {
                                                                            let _ = vespetrel_storage::repo::upsert_folder(c, f);
                                                                        }
                                                                    }).await;
                                                                }
                                                            }

                                                            let _ = this.update(cx, |view, cx| {
                                                                view.selected_folder_id = local_folders.first().map(|f| f.id.clone());
                                                                view.folders.extend(local_folders);
                                                                view.accounts.push(acct_clone.clone());
                                                                view.status_message = format!("✓ Successfully connected {} ({:?})", acct_clone.email, acct_clone.provider_type);
                                                                view.show_toast(format!("✓ Account {} connected successfully", acct_clone.email), false, cx);
                                                                view.active_modal = ActiveModal::None;
                                                                view.wizard_inputs = None;
                                                                view.login_wizard.step = WizardStep::Completed;
                                                                let _ = sync_sender.send(SyncEvent::FolderListUpdated(remote_folders));
                                                                cx.notify();
                                                            });
                                                        }
                                                        Err(e) => {
                                                            let _ = this.update(cx, |view, cx| {
                                                                view.status_message = format!("⚠️ Connection error: {e}");
                                                                view.show_toast(format!("⚠️ Connection error: {e}"), true, cx);
                                                                view.login_wizard.step = WizardStep::Failed(format!("{e}"));
                                                                cx.notify();
                                                            });
                                                        }
                                                    }
                                                }).detach();
                                            }))
                                            .child("Connect Account 🚀")
                                    },
                                ),
                        ),
                )
        }

        /// Initiates the OAuth2 PKCE authorization flow via system default web browser
        pub fn start_oauth2_flow(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
            let provider_type = self.login_wizard.provider_type.clone();
            let mut client_id = if let Some(inputs) = &self.wizard_inputs {
                inputs.client_id.read(cx).value().trim().to_string()
            } else {
                String::new()
            };
            if client_id.is_empty() {
                client_id = match provider_type {
                    ProviderType::Gmail => {
                        std::env::var("VESPETREL_GOOGLE_CLIENT_ID").unwrap_or_default()
                    }
                    ProviderType::Graph => {
                        std::env::var("VESPETREL_MICROSOFT_CLIENT_ID").unwrap_or_default()
                    }
                    _ => String::new(),
                };
            }

            if client_id.is_empty() {
                let err_msg = format!(
                    "OAuth2 requires a Client ID. Please enter your {} OAuth Client ID or switch to the 'App Password' tab.",
                    match provider_type {
                        ProviderType::Gmail => "Google Cloud",
                        ProviderType::Graph => "Microsoft Entra / Azure",
                        _ => "Provider",
                    }
                );
                self.show_toast(err_msg.clone(), true, cx);
                self.login_wizard.step = WizardStep::Failed(err_msg);
                cx.notify();
                return;
            }

            let user_email = if let Some(inputs) = &self.wizard_inputs {
                inputs.email.read(cx).value().trim().to_string()
            } else {
                String::new()
            };
            let user_name = if let Some(inputs) = &self.wizard_inputs {
                inputs.name.read(cx).value().trim().to_string()
            } else {
                String::new()
            };

            self.login_wizard.step = WizardStep::OAuth2Waiting;
            self.login_wizard.oauth_status = Some("Starting browser authorization...".into());
            self.status_message = "Waiting for browser OAuth2 authorization...".into();
            cx.notify();

            let pool_opt = self.storage_pool.clone();
            let sync_sender = self.sync_sender.clone();

            cx.spawn(async move |this, cx| {
                let (listener, port) = match vespetrel_crypto::OAuth2Engine::bind_loopback().await {
                    Ok(res) => res,
                    Err(e) => {
                        let _ = this.update(cx, |view, cx| {
                            view.show_toast(format!("Failed to bind loopback listener: {e}"), true, cx);
                            view.login_wizard.step = WizardStep::Failed(format!("Failed to bind loopback listener: {e}"));
                            cx.notify();
                        });
                        return;
                    }
                };

                let mut oauth_cfg = match provider_type {
                    ProviderType::Gmail => vespetrel_crypto::OAuth2Config::google(&client_id),
                    ProviderType::Graph => vespetrel_crypto::OAuth2Config::microsoft(&client_id),
                    _ => return,
                };
                oauth_cfg.redirect_uri = format!("http://127.0.0.1:{port}/callback");
                let engine = vespetrel_crypto::OAuth2Engine::new(oauth_cfg.clone());
                let (auth_url, csrf_token, verifier) = engine.auth_url();

                tracing::info!(auth_url=%auth_url, "Launching system browser for OAuth2 authorization");
                #[cfg(windows)]
                {
                    let spawn_res = std::process::Command::new("rundll32")
                        .args(["url.dll,FileProtocolHandler", &auth_url])
                        .spawn();
                    if spawn_res.is_err() {
                        let _ = std::process::Command::new("cmd")
                            .args(["/c", "start", "", &auth_url])
                            .spawn();
                    }
                }
                #[cfg(not(windows))]
                {
                    let _ = std::process::Command::new("xdg-open")
                        .arg(&auth_url)
                        .spawn();
                }

                let _ = this.update(cx, |view, cx| {
                    view.login_wizard.oauth_status = Some(format!(
                        "Browser opened! Complete sign-in in your browser (loopback port {port})..."
                    ));
                    cx.notify();
                });

                let code_res = engine.wait_for_callback_on_listener(listener, 180, Some(csrf_token.secret())).await;
                let code = match code_res {
                    Ok(c) => c,
                    Err(e) => {
                        let _ = this.update(cx, |view, cx| {
                            view.show_toast(format!("OAuth2 callback failed: {e}"), true, cx);
                            view.login_wizard.step = WizardStep::Failed(format!("OAuth2 callback failed: {e}"));
                            cx.notify();
                        });
                        return;
                    }
                };

                let _ = this.update(cx, |view, cx| {
                    view.login_wizard.step = WizardStep::Validating;
                    view.status_message = "Exchanging authorization code for tokens...".into();
                    cx.notify();
                });

                let token_bundle = match engine.exchange_code(code, verifier.secret().to_string()).await {
                    Ok(b) => b,
                    Err(e) => {
                        let _ = this.update(cx, |view, cx| {
                            view.show_toast(format!("Token exchange failed: {e}"), true, cx);
                            view.login_wizard.step = WizardStep::Failed(format!("Token exchange failed: {e}"));
                            cx.notify();
                        });
                        return;
                    }
                };

                let (final_email, final_name) = if user_email.is_empty() {
                    match provider_type {
                        ProviderType::Gmail => fetch_google_userinfo(&token_bundle.access_token).await.unwrap_or((user_email, user_name)),
                        ProviderType::Graph => fetch_microsoft_userinfo(&token_bundle.access_token).await.unwrap_or((user_email, user_name)),
                        _ => (user_email, user_name),
                    }
                } else {
                    (user_email, user_name)
                };

                if final_email.is_empty() || !final_email.contains('@') {
                    let _ = this.update(cx, |view, cx| {
                        view.show_toast("Could not determine user email address from OAuth2 token", true, cx);
                        view.login_wizard.step = WizardStep::Failed("Could not determine user email address from OAuth2 token".into());
                        cx.notify();
                    });
                    return;
                }

                let display_name = if final_name.is_empty() { final_email.clone() } else { final_name };

                let ak_key = format!("vespetrel_oauth_{final_email}");
                if let Ok(entry) = keyring::Entry::new("vespetrel", &ak_key) {
                    let _ = entry.set_password(&token_bundle.access_token);
                }
                let mut rk_key = None;
                if let Some(ref rt) = token_bundle.refresh_token {
                    let k = format!("vespetrel_refresh_{final_email}");
                    if let Ok(entry) = keyring::Entry::new("vespetrel", &k) {
                        let _ = entry.set_password(rt);
                    }
                    rk_key = Some(k);
                }

                let expires_at = chrono::Utc::now().timestamp() + token_bundle.expires_in as i64;

                let mut acct = Account::new(display_name, final_email.clone(), provider_type.clone());
                acct.auth_config.auth_method = vespetrel_core::account::AuthMethod::OAuth2;
                acct.auth_config.username = Some(final_email.clone());
                acct.auth_config.keyring_key = Some(ak_key);
                acct.auth_config.refresh_token_keyring_key = rk_key;
                acct.auth_config.expires_at = Some(expires_at);
                acct.auth_config.oauth = Some(vespetrel_core::account::OAuthConfig {
                    client_id: oauth_cfg.client_id.clone(),
                    auth_url: oauth_cfg.auth_url.clone(),
                    token_url: oauth_cfg.token_url.clone(),
                    redirect_uri: oauth_cfg.redirect_uri.clone(),
                    scopes: oauth_cfg.scopes.clone(),
                });

                let provider = vespetrel_engine::coordinator::make_provider_with_token(&acct, token_bundle.access_token.clone());
                let folders_res = provider.sync_folder_list().await;
                match folders_res {
                    Ok(remote_folders) => {
                        let mut local_folders = Vec::new();
                        if remote_folders.is_empty() {
                            local_folders.push(Folder::new(&acct.id, "INBOX", "INBOX", "INBOX").with_role(FolderRole::Inbox));
                            local_folders.push(Folder::new(&acct.id, "Drafts", "Drafts", "Drafts").with_role(FolderRole::Drafts));
                            local_folders.push(Folder::new(&acct.id, "Sent", "Sent", "Sent").with_role(FolderRole::Sent));
                            local_folders.push(Folder::new(&acct.id, "Trash", "Trash", "Trash").with_role(FolderRole::Trash));
                        } else {
                            for rf in &remote_folders {
                                let role = match rf.role_hint.as_deref().unwrap_or("") {
                                    "inbox" | "Inbox" => FolderRole::Inbox,
                                    "drafts" | "Drafts" => FolderRole::Drafts,
                                    "sent" | "Sent" => FolderRole::Sent,
                                    "trash" | "Trash" => FolderRole::Trash,
                                    "archive" | "Archive" => FolderRole::Archive,
                                    "spam" | "Spam" | "junk" | "Junk" => FolderRole::Junk,
                                    _ => FolderRole::Custom,
                                };
                                let mut f = Folder::new(&acct.id, &rf.remote_id, &rf.name, &rf.path).with_role(role);
                                f.uid_validity = rf.uid_validity;
                                f.highest_mod_seq = rf.highest_mod_seq;
                                local_folders.push(f);
                            }
                        }

                        if let Some(pool) = pool_opt {
                            let acct_save = acct.clone();
                            let f_save = local_folders.clone();
                            if let Ok(conn) = pool.get().await {
                                let _ = conn.interact(move |c| {
                                    let _ = vespetrel_storage::repo::upsert_account(c, &acct_save);
                                    for f in &f_save {
                                        let _ = vespetrel_storage::repo::upsert_folder(c, f);
                                    }
                                }).await;
                            }
                        }

                        let _ = this.update(cx, |view, cx| {
                            view.selected_folder_id = local_folders.first().map(|f| f.id.clone());
                            view.folders.extend(local_folders);
                            view.accounts.push(acct.clone());
                            view.status_message = format!("✓ Successfully connected {} via OAuth2", acct.email);
                            view.show_toast(format!("✓ Successfully connected {} via OAuth2", acct.email), false, cx);
                            view.active_modal = ActiveModal::None;
                            view.wizard_inputs = None;
                            view.login_wizard.step = WizardStep::Completed;
                            let _ = sync_sender.send(SyncEvent::FolderListUpdated(remote_folders));
                            cx.notify();
                        });
                    }
                    Err(e) => {
                        let _ = this.update(cx, |view, cx| {
                            view.status_message = format!("⚠️ Connection error: {e}");
                            view.show_toast(format!("⚠️ Connection error: {e}"), true, cx);
                            view.login_wizard.step = WizardStep::Failed(format!("{e}"));
                            cx.notify();
                        });
                    }
                }
            }).detach();
        }
    }

    async fn fetch_google_userinfo(access_token: &str) -> anyhow::Result<(String, String)> {
        let client = reqwest::Client::builder()
            .user_agent("Vespetrel/0.1")
            .timeout(std::time::Duration::from_secs(10))
            .build()?;
        let resp = client
            .get("https://openidconnect.googleapis.com/v1/userinfo")
            .bearer_auth(access_token)
            .send()
            .await?;
        if !resp.status().is_success() {
            anyhow::bail!("Google userinfo request failed: {}", resp.status());
        }
        let val: serde_json::Value = resp.json().await?;
        let email = val["email"].as_str().unwrap_or("").to_string();
        let name = val["name"].as_str().unwrap_or(&email).to_string();
        if email.is_empty() {
            anyhow::bail!("missing email in Google userinfo response");
        }
        Ok((email, name))
    }

    async fn fetch_microsoft_userinfo(access_token: &str) -> anyhow::Result<(String, String)> {
        let client = reqwest::Client::builder()
            .user_agent("Vespetrel/0.1")
            .timeout(std::time::Duration::from_secs(10))
            .build()?;
        let resp = client
            .get("https://graph.microsoft.com/v1.0/me")
            .bearer_auth(access_token)
            .send()
            .await?;
        if !resp.status().is_success() {
            anyhow::bail!("Microsoft Graph me request failed: {}", resp.status());
        }
        let val: serde_json::Value = resp.json().await?;
        let email = val["mail"]
            .as_str()
            .or_else(|| val["userPrincipalName"].as_str())
            .unwrap_or("")
            .to_string();
        let name = val["displayName"].as_str().unwrap_or(&email).to_string();
        if email.is_empty() {
            anyhow::bail!("missing email in Microsoft Graph me response");
        }
        Ok((email, name))
    }

    /// Launch the GPUI Desktop Application
    pub fn run_gpui_app(
        sync_rx: flume::Receiver<SyncEvent>,
        sync_tx: flume::Sender<SyncEvent>,
        storage_pool: Option<vespetrel_storage::db::StoragePool>,
    ) {
        gpui_kit::application().run(move |cx: &mut App| {
            gpui_kit::init(cx);
            let rx = sync_rx.clone();
            let tx = sync_tx.clone();
            let pool = storage_pool.clone();
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
                move |window, cx| {
                    let view = cx.new(|cx| MainWindow::from_storage(cx, rx, tx, pool));
                    cx.new(|cx| gpui_kit::component::Root::new(view, window, cx))
                },
            );
        });
    }
}
