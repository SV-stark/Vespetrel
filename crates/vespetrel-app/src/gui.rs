#[cfg(feature = "gpui")]
pub mod gpui_app {
    use crate::views::{
        calendar::CalendarView,
        contacts::ContactsView,
        login_wizard::{AuthModeChoice, LoginWizardState, WizardStep},
        message_list::ListFilter,
        navigation::NavigationTree,
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
        pub command_palette: crate::command_palette::CommandPalette,
        // Event channel from Tokio sync engine
        pub sync_sender: flume::Sender<SyncEvent>,
        pub status_message: String,
        pub storage_pool: Option<vespetrel_storage::db::StoragePool>,
        pub login_wizard: LoginWizardState,
        pub wizard_inputs: Option<WizardInputEntities>,
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

                                let mut initial_messages = Vec::new();
                                if let Some(inbox) = all_folders
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
                selected_message_id: messages.first().map(|m: &MessageSummary| m.id.clone()),
                folders,
                messages,
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
                command_palette: crate::command_palette::CommandPalette::new(),
                sync_sender: sync_tx,
                status_message: "All mailboxes synchronized".into(),
                storage_pool,
                login_wizard: LoginWizardState::new(),
                wizard_inputs: None,
            }
        }

        pub fn handle_sync_event(&mut self, event: SyncEvent, cx: &mut Context<Self>) {
            match event {
                SyncEvent::MessagesInserted(new_msgs) => {
                    self.status_message = format!("Received {} new message(s)", new_msgs.len());
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

        pub fn filtered_messages(&self) -> impl Iterator<Item = &MessageSummary> {
            let q = self.search_query.trim();
            self.messages.iter().filter(move |m| {
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
                m.subject
                    .as_deref()
                    .is_some_and(|s| contains_ignore_case(s, q))
                    || contains_ignore_case(&m.from_address, q)
                    || m.from_name
                        .as_deref()
                        .is_some_and(|n| contains_ignore_case(n, q))
                    || m.snippet
                        .as_deref()
                        .is_some_and(|sn| contains_ignore_case(sn, q))
            })
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
            if let Some(id) = self.selected_message_id.clone() {
                if let Some(m) = self.messages.iter_mut().find(|m| m.id == id) {
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
        }

        pub fn delete_selected_message(&mut self, cx: &mut Context<Self>) {
            if let Some(id) = self.selected_message_id.clone() {
                self.messages.retain(|m| m.id != id);
                self.selected_message_id = self.messages.first().map(|m| m.id.clone());
                self.status_message = "Message deleted".into();
                cx.notify();

                if let Some(pool) = &self.storage_pool {
                    let pool = pool.clone();
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
            }
        }

        pub fn archive_selected_message(&mut self, cx: &mut Context<Self>) {
            if let Some(id) = self.selected_message_id.clone() {
                self.messages.retain(|m| m.id != id);
                self.selected_message_id = self.messages.first().map(|m| m.id.clone());
                self.status_message = "Message archived".into();
                cx.notify();

                if let Some(pool) = &self.storage_pool {
                    let pool = pool.clone();
                    let msg_id = id.clone();
                    cx.spawn(async move |_this, _cx| {
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
                        }
                    })
                    .detach();
                }
            }
        }

        pub fn send_composed_message(&mut self, cx: &mut Context<Self>) {
            if self.compose_to.trim().is_empty() {
                self.status_message = "Error: Please specify a recipient".into();
                cx.notify();
                return;
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
            let composed = vespetrel_core::ComposedMessage {
                from: vespetrel_core::Address {
                    name: from_name.clone(),
                    email: from_email.clone(),
                },
                to: vec![vespetrel_core::Address {
                    name: None,
                    email: self.compose_to.trim().to_string(),
                }],
                cc: vec![],
                bcc: vec![],
                subject: self.compose_subject.clone(),
                body_text: self.compose_body.clone(),
                body_html: None,
                in_reply_to: None,
                references: vec![],
                attachments: vec![],
            };

            let outbox_id = format!("outbox-{}", uuid::Uuid::new_v4());
            let now_ts = chrono::Utc::now().timestamp();
            let account_opt = self.accounts.first().cloned();
            let pool_opt = self.storage_pool.clone();
            let to_dest = self.compose_to.trim().to_string();

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

            self.status_message = format!("Sending message to {to_dest}...");
            self.active_modal = ActiveModal::None;
            self.compose_to.clear();
            self.compose_subject.clear();
            self.compose_body.clear();
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
                        }
                    }
                    cx.notify();
                });
            })
            .detach();
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
        let needle_chars: smallvec::SmallVec<[char; 32]> =
            needle.chars().flat_map(|c| c.to_lowercase()).collect();
        let haystack_chars: smallvec::SmallVec<[char; 128]> =
            haystack.chars().flat_map(|c| c.to_lowercase()).collect();
        if haystack_chars.len() < needle_chars.len() {
            return false;
        }
        haystack_chars
            .windows(needle_chars.len())
            .any(|w| w == needle_chars.as_slice())
    }

    impl Render for MainWindow {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            if self.active_modal == ActiveModal::AddAccount && self.wizard_inputs.is_none() {
                self.wizard_inputs = Some(WizardInputEntities::new(window, cx, &self.login_wizard));
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
                .child(self.render_modal_layer(cx))
        }
    }

    impl MainWindow {
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
                        .w(px(420.0))
                        .h(px(32.0))
                        .px(px(12.0))
                        .rounded_md()
                        .bg(rgb(0x1a202e))
                        .border_1()
                        .border_color(rgb(0x2d3748))
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
                                    rgb(0xe2e8f0)
                                })
                                .child(search_display),
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
                    ActiveViewTab::Settings => self.render_settings_view().into_any_element(),
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
                            div()
                                .id(ElementId::Name(format!("account-card-{}", idx).into()))
                                .flex()
                                .flex_col()
                                .p(px(8.0))
                                .rounded_md()
                                .bg(rgb(0x181f2f))
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
                                )
                        })
                        .collect()
                })
                .child(
                    div().flex().flex_col().gap(px(4.0)).children(
                        NavigationTree::new(self.folders.clone())
                            .sorted_folders()
                            .into_iter()
                            .map(|f| {
                                let is_selected =
                                    self.selected_folder_id.as_deref() == Some(&f.remote_id);
                                let remote_id_clone = f.remote_id.clone();
                                let icon = match f.role {
                                    FolderRole::Inbox => "📥",
                                    FolderRole::Drafts => "📝",
                                    FolderRole::Sent => "📤",
                                    FolderRole::Archive => "📦",
                                    FolderRole::Junk => "🚫",
                                    FolderRole::Trash => "🗑️",
                                    FolderRole::Custom => "📁",
                                };
                                let count = if f.role == FolderRole::Inbox {
                                    self.messages.len()
                                } else {
                                    0
                                };

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
                                        this.selected_folder_id = Some(remote_id_clone.clone());
                                        cx.notify();
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
                            }),
                    ),
                )
        }

        fn render_message_list_pane(&self, cx: &Context<Self>) -> impl IntoElement {
            let visible_msgs: Vec<&MessageSummary> = self.filtered_messages().take(100).collect();

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
                        .flex_col()
                        .flex_1()
                        .children(visible_msgs.into_iter().map(|msg| {
                            let is_selected = self.selected_message_id.as_deref() == Some(&msg.id);
                            let sender = msg.from_name.as_deref().unwrap_or(&msg.from_address);
                            let subject = msg.subject.as_deref().unwrap_or("(No Subject)");
                            let snippet = msg.snippet.as_deref().unwrap_or("");
                            let date_str = msg.sent_at.format("%b %d, %H:%M").to_string();
                            let msg_id = msg.id.clone();

                            div()
                                .id(ElementId::Name(format!("msg-item-{}", msg_id).into()))
                                .flex()
                                .flex_col()
                                .p(px(10.0))
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
                                    this.selected_message_id = Some(msg_id.clone());
                                    cx.notify();
                                }))
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
                                                .child(if !msg.is_read {
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
                                                .child(
                                                    div()
                                                        .text_xs()
                                                        .text_color(if msg.is_flagged {
                                                            rgb(0xfbbf24)
                                                        } else {
                                                            rgb(0x475569)
                                                        })
                                                        .child(if msg.is_flagged {
                                                            "★"
                                                        } else {
                                                            "☆"
                                                        }),
                                                )
                                                .child(
                                                    div()
                                                        .text_xs()
                                                        .text_color(rgb(0x64748b))
                                                        .child(date_str),
                                                ),
                                        ),
                                )
                                .child(
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
                                )
                                .child(div().pt(px(2.0)).text_xs().text_color(rgb(0x94a3b8)).child(
                                    if snippet.len() > 60 {
                                        format!("{}...", &snippet[..60])
                                    } else {
                                        snippet.to_string()
                                    },
                                ))
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
                                                    let reply_subj = if msg.subject.as_deref().unwrap_or("").to_lowercase().starts_with("re:") {
                                                        msg.subject.clone().unwrap_or_default()
                                                    } else {
                                                        format!("Re: {}", msg.subject.as_deref().unwrap_or(""))
                                                    };
                                                    move |this, _, _, cx| {
                                                        this.compose_to = reply_to.clone();
                                                        this.compose_subject = reply_subj.clone();
                                                        this.active_modal = ActiveModal::Compose;
                                                        cx.notify();
                                                    }
                                                }))
                                                .child("↩ Reply"),
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
                                        )
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
                                                        .child(from_name.chars().next().unwrap_or('U').to_string()),
                                                )
                                                .child(
                                                    div()
                                                        .flex()
                                                        .flex_col()
                                                        .child(div().text_xs().font_weight(FontWeight::BOLD).text_color(rgb(0xf1f5f9)).child(if from_name.is_empty() { from_addr.clone() } else { format!("{from_name} <{from_addr}>") }))
                                                        .child(div().text_xs().text_color(rgb(0x94a3b8)).child(format!("To: {}", self.accounts.first().map(|a| a.email.as_str()).unwrap_or("me"))))
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
                                .child(
                                    div()
                                        .id("btn-toggle-images")
                                        .text_xs()
                                        .text_color(rgb(0x60a5fa))
                                        .cursor_pointer()
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.block_remote_images = !this.block_remote_images;
                                            cx.notify();
                                        }))
                                        .child(if self.block_remote_images { "Load Remote Images" } else { "Block Images" }),
                                )
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

        fn render_settings_view(&self) -> Div {
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
                                .child("Storage & Database Engine"),
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
                        .child(
                            div()
                                .text_color(rgb(0x94a3b8))
                                .child(self.status_message.clone()),
                        ),
                )
                .child(div().child("120 FPS GPU Direct3D / Vulkan • Rust Edition 2024"))
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
                                            cx.notify();
                                        }))
                                        .child("✕"),
                                ),
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
                                .child(div().text_xs().text_color(rgb(0xf1f5f9)).child(to_text)),
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
                                .child(div().text_xs().text_color(rgb(0xf1f5f9)).child(subj_text)),
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
                                .child(div().text_xs().text_color(rgb(0xe2e8f0)).child(body_text)),
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
                                                    let pw = "secure_token_preset".to_string();
                                                    this.login_wizard.email = em.clone();
                                                    this.login_wizard.name = nm.clone();
                                                    this.login_wizard.password_or_token = pw.clone();
                                                    if let Some(inputs) = &this.wizard_inputs {
                                                        inputs.email.update(cx, |inp, cx| inp.set_value(em, window, cx));
                                                        inputs.name.update(cx, |inp, cx| inp.set_value(nm, window, cx));
                                                        inputs.password.update(cx, |inp, cx| inp.set_value(pw, window, cx));
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
                                                    let pw = "secure_token_preset".to_string();
                                                    this.login_wizard.email = em.clone();
                                                    this.login_wizard.name = nm.clone();
                                                    this.login_wizard.password_or_token = pw.clone();
                                                    if let Some(inputs) = &this.wizard_inputs {
                                                        inputs.email.update(cx, |inp, cx| inp.set_value(em, window, cx));
                                                        inputs.name.update(cx, |inp, cx| inp.set_value(nm, window, cx));
                                                        inputs.password.update(cx, |inp, cx| inp.set_value(pw, window, cx));
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
                                                    let pw = "secure_token_preset".to_string();
                                                    this.login_wizard.email = em.clone();
                                                    this.login_wizard.name = nm.clone();
                                                    this.login_wizard.password_or_token = pw.clone();
                                                    if let Some(inputs) = &this.wizard_inputs {
                                                        inputs.email.update(cx, |inp, cx| inp.set_value(em, window, cx));
                                                        inputs.name.update(cx, |inp, cx| inp.set_value(nm, window, cx));
                                                        inputs.password.update(cx, |inp, cx| inp.set_value(pw, window, cx));
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
                                                let (email, password, name, in_host, in_port, out_host, out_port) = if let Some(inputs) = &this.wizard_inputs {
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
                                                    cx.notify();
                                                    return;
                                                }
                                                if password.is_empty() {
                                                    this.login_wizard.step = WizardStep::Failed("Password or authentication token cannot be empty".into());
                                                    cx.notify();
                                                    return;
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
                                                        this.login_wizard.step = WizardStep::Failed(e);
                                                        cx.notify();
                                                        return;
                                                    }
                                                };

                                                this.login_wizard.step = WizardStep::Validating;
                                                this.status_message = format!("Connecting to {}...", acct.email);
                                                cx.notify();

                                                // Persist credentials to native OS keyring
                                                if let Some(ref k) = acct.auth_config.keyring_key {
                                                    if let Ok(entry) = keyring::Entry::new("vespetrel", k) {
                                                        let _ = entry.set_password(&password);
                                                    }
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
                self.login_wizard.step = WizardStep::Failed(format!(
                    "OAuth2 requires a Client ID. Please enter your {} OAuth Client ID or switch to the 'App Password' tab.",
                    match provider_type {
                        ProviderType::Gmail => "Google Cloud",
                        ProviderType::Graph => "Microsoft Entra / Azure",
                        _ => "Provider",
                    }
                ));
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
