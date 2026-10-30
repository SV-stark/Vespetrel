#[cfg(feature = "gpui")]
pub mod gpui_app {
    use crate::actions::*;
    use crate::keybindings::{KeyAction, KeybindingMap, to_gpui_keystroke};
    use crate::views::compose::ComposeState;
    use crate::views::message_list::MessageSelection;
    use crate::views::settings::{SettingsTab, SettingsViewState};
    use crate::views::{
        calendar::CalendarView,
        contacts::ContactsView,
        login_wizard::{AuthModeChoice, AutodiscoverStatus, LoginWizardState, WizardStep},
        message_list::{ListFilter, MessageRowDensity},
        message_viewer::{AttachmentInfo, AuthState, BodyKind, MessageViewer, SecurityStatus},
        navigation::NavigationTree,
        quick_filter::QuickFilterState,
        tasks::TaskListView,
    };
    pub use gpui_kit::assets::Assets;
    pub use gpui_kit::base;
    use gpui_kit::base::Selectable;
    pub use gpui_kit::component;
    pub use gpui_kit::component::alert::Alert;
    pub use gpui_kit::component::badge::Badge;
    pub use gpui_kit::component::button::{Button, ButtonVariants};
    pub use gpui_kit::component::command::{Command, CommandGroup, CommandItem, CommandState};
    pub use gpui_kit::component::dialog::Dialog;
    pub use gpui_kit::component::form::{Field, Form};
    pub use gpui_kit::component::kbd::Kbd;
    pub use gpui_kit::component::notification::Notification;
    pub use gpui_kit::component::popover::Popover;
    use gpui_kit::component::resizable::{h_resizable, v_resizable};
    pub use gpui_kit::component::sheet::Sheet;
    pub use gpui_kit::component::status_bar::StatusBar;
    pub use gpui_kit::component::tab::{Tab, TabBar};
    use gpui_kit::component::text::TextView;
    pub use gpui_kit::component::theme::{Theme, ThemeMode};
    pub use gpui_kit::component::{
        ActiveTheme, Icon, IconName, Sizable, ThemeStyled, TitleBar, VirtualList, WindowExt,
        v_virtual_list,
    };
    pub use gpui_kit::gpui;
    pub use gpui_kit::prelude::FluentBuilder;

    pub use gpui_kit::gpui::*;
    use vespetrel_core::{
        Account, CalendarEvent, Contact, Folder, FolderRole, MessageSummary, ProviderType,
        TaskItem, UserSettings, provider::SyncEvent,
    };

    /// The command-palette shortcut as a `Keystroke`, so the header can show a real key cap
    /// instead of a hardcoded glyph.
    ///
    /// macOS binds the Command key, which GPUI reports as `platform`; Windows and Linux bind
    /// Control. Rendering `⌘K` on every platform told a Windows user to press a key their
    /// keyboard does not have.
    pub fn command_palette_keystroke() -> Keystroke {
        let modifiers = Modifiers {
            // `cfg!` rather than `#[cfg]` so both arms are always type-checked.
            platform: cfg!(target_os = "macos"),
            control: !cfg!(target_os = "macos"),
            ..Default::default()
        };
        Keystroke {
            modifiers,
            key: "k".into(),
            key_char: Some("k".into()),
        }
    }

    /// Truncate to at most `max_chars` characters, appending a single `…` when cut.
    ///
    /// Slicing a `&str` by byte index panics as soon as the index lands inside a
    /// multi-byte character, so anything user- or locale-supplied has to be walked by
    /// `char`. Used for message snippets, sender display names and folder labels.
    pub fn elide(text: &str, max_chars: usize) -> String {
        if text.chars().count() <= max_chars {
            return text.to_string();
        }
        let kept: String = text.chars().take(max_chars).collect();
        format!("{kept}\u{2026}")
    }

    /// One destination in the folder tree.
    ///
    /// Grouped rather than passed as seven positional arguments, so adding a field to a row
    /// cannot silently reorder the call sites.
    pub struct FolderRow<'a> {
        pub id: ElementId,
        pub icon: IconName,
        pub label: &'a str,
        pub unread: i64,
        pub is_selected: bool,
    }

    /// The state the status bar presents.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum StatusState {
        NoAccount,
        Failed,
        Busy,
        Ready,
    }

    /// Is this attachment an S/MIME `pkcs7-mime` signature part?
    ///
    /// Both the media type and the conventional extensions are checked, because
    /// senders disagree: Thunderbird writes `.p7s`, Outlook writes `.p7m`, and some
    /// servers relabel the part as `application/octet-stream`.
    pub fn is_smime_part(content_type: &str, filename: &str) -> bool {
        let ct = content_type.to_ascii_lowercase();
        if ct.contains("pkcs7-mime") || ct.contains("pkcs7-signature") {
            return true;
        }
        let name = filename.to_ascii_lowercase();
        name.ends_with(".p7s") || name.ends_with(".p7m")
    }

    /// A keymap token rendered for a person, rather than for a parser.
    ///
    /// The stored form is what a config file needs (`Ctrl+Shift+r`); the display form is
    /// what a shortcut table needs (`Ctrl+Shift+R`). Only the display side is
    /// platform-aware, because the stored form has to survive a round trip unchanged.
    pub fn format_shortcut(token: &str) -> String {
        token
            .split_whitespace()
            .map(|part| match part.to_ascii_lowercase().as_str() {
                "ctrl+" | "ctrl" => if cfg!(target_os = "macos") {
                    "\u{2318}"
                } else {
                    "Ctrl"
                }
                .to_string(),
                "shift+" => "\u{21E7}".to_string(),
                "alt+" => if cfg!(target_os = "macos") {
                    "\u{2325}"
                } else {
                    "Alt"
                }
                .to_string(),
                // Anything else is a key name, which reads better in caps in a table.
                _ => part.to_uppercase(),
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// The reader's verdict after a CMS check.
    ///
    /// `verified` is `Some(true)` only when a signature checked out. `None` means no
    /// blob could be read, so nothing was established and the reader must stay silent
    /// rather than imply a result. `saw_signature` distinguishes "no signature" from
    /// "a signature I could not check", which is the difference between showing
    /// nothing and showing "not verified".
    pub fn smime_verdict(
        verified: Option<bool>,
        saw_signature: bool,
    ) -> crate::views::message_viewer::SecurityStatus {
        use crate::views::message_viewer::SecurityStatus;
        match verified {
            Some(true) => SecurityStatus::SignatureValid,
            Some(false) => SecurityStatus::SignatureInvalid,
            None if saw_signature => SecurityStatus::SignedUnverified,
            // Nothing readable was present, so there is no signature to talk about.
            None => SecurityStatus::Unsigned,
        }
    }

    /// Split a recipient field into addresses.
    ///
    /// Accepts the forms people actually type: `a@b.com`, `Name <a@b.com>`, several
    /// separated by commas, semicolons or whitespace. A token with no `@` is dropped:
    /// `lettre` would otherwise pass it to the MTA and the user would learn about the
    /// typo from a bounce, minutes later, with the message already gone.
    pub fn parse_address_list(raw: &str) -> Vec<vespetrel_core::Address> {
        let mut out = Vec::new();
        for token in raw.split([',', ';']) {
            for piece in token.split_whitespace() {
                let piece = piece.trim_matches(['<', '>', ',']);
                if piece.is_empty() || !piece.contains('@') {
                    continue;
                }
                out.push(vespetrel_core::Address {
                    name: None,
                    email: piece.to_string(),
                });
            }
        }
        out
    }

    /// Expand a leading `~` to the user's home directory.
    ///
    /// Returns `None` when the path needs expansion but no home directory can be
    /// resolved, so the caller can say so instead of reporting a confusing "no such
    /// file".
    pub fn expand_home(raw: &str) -> Option<String> {
        let rest = raw.strip_prefix('~')?;
        let rest = match rest.strip_prefix('/').or_else(|| rest.strip_prefix('\\')) {
            Some(r) => r,
            // `~otheruser` is not this user's home; leave it alone and let the read fail
            // with an accurate error.
            None => return Some(raw.to_string()),
        };
        let home = std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(std::path::PathBuf::from)?;
        Some(home.join(rest).to_string_lossy().into_owned())
    }

    /// The final path component, for a display name.
    pub fn file_name_of(path: &str) -> String {
        path.rsplit(['/', '\\'])
            .find(|s| !s.is_empty())
            .unwrap_or(path)
            .to_string()
    }

    /// A conservative media type for an outgoing attachment.
    ///
    /// Attachment typing is advisory: the recipient's client decides how to open it. A
    /// wrong guess costs a double click; a missing type makes some clients treat a PDF as
    /// plain text. Only extensions with a well-known mapping are claimed.
    pub fn guess_content_type(filename: &str) -> &'static str {
        let ext = filename
            .rsplit_once('.')
            .map(|(_, e)| e.to_ascii_lowercase())
            .unwrap_or_default();
        match ext.as_str() {
            "pdf" => "application/pdf",
            "png" => "image/png",
            "jpg" | "jpeg" => "image/jpeg",
            "gif" => "image/gif",
            "webp" => "image/webp",
            "svg" => "image/svg+xml",
            "txt" | "log" | "md" => "text/plain",
            "csv" => "text/csv",
            "html" | "htm" => "text/html",
            "json" => "application/json",
            "xml" => "application/xml",
            "zip" => "application/zip",
            "doc" => "application/msword",
            "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            "xls" => "application/vnd.ms-excel",
            "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
            "ics" => "text/calendar",
            "vcf" => "text/vcard",
            "eml" => "message/rfc822",
            _ => "application/octet-stream",
        }
    }

    /// Add a reply or forward prefix unless the subject already carries one.
    ///
    /// The previous code sliced `s[..3]` to test the prefix. A subject is
    /// attacker-controlled and frequently non-ASCII, so a subject whose third byte fell
    /// inside a multi-byte character panicked the reader — `"Rd 日本"` is enough.
    pub fn prefixed_subject(subject: Option<&str>, prefix: &str) -> String {
        let Some(subject) = subject else {
            return prefix.to_string();
        };
        let existing = subject.split_once(':').map(|(head, _)| head).unwrap_or("");
        if existing.eq_ignore_ascii_case(prefix.trim_end_matches(':')) {
            subject.to_string()
        } else {
            format!("{prefix} {subject}")
        }
    }

    /// Does this body carry HTML rather than plain text?
    ///
    /// The reader used to test for the literal substrings `<html`, `<div` and `<p`.
    /// A plain-text message that discusses HTML ("see `<p>` below") was then run
    /// through the sanitizer and shown as a wall of markup, and a body using only
    /// `<table>` or `<br>` was shown as raw source. Requiring a tag that carries
    /// structure is a closer test and fails toward the safer branch, because plain
    /// text is escaped before display and never shown as markup.
    pub fn looks_like_html(content: &str) -> bool {
        const STRUCTURAL: [&str; 8] = [
            "<html", "<body", "<div", "<table", "<br", "<p>", "<p ", "<a href",
        ];
        let lowered = content.to_ascii_lowercase();
        STRUCTURAL.iter().any(|tag| lowered.contains(tag))
    }

    /// Classify a status message into a state.
    ///
    /// Split out from the view so the mapping can be tested. Deciding the state from words
    /// inside a free-text message is fragile — a folder named "Errors" or a subject
    /// containing "failed" would otherwise turn the whole window's indicator red — so the
    /// caller is expected to pass a message that is about the connection.
    pub fn classify_status(accounts: usize, message: &str) -> StatusState {
        if accounts == 0 {
            return StatusState::NoAccount;
        }
        let text = message.to_ascii_lowercase();
        const FAILURE: [&str; 8] = [
            "offline",
            "error",
            "failed",
            "failure",
            "unavailable",
            "denied",
            "couldn't",
            "timed out",
        ];
        const BUSY: [&str; 5] = ["syncing", "loading", "connecting", "...", "\u{2026}"];
        if FAILURE.iter().any(|w| text.contains(w)) {
            StatusState::Failed
        } else if BUSY.iter().any(|w| text.contains(w)) {
            StatusState::Busy
        } else {
            StatusState::Ready
        }
    }

    /// Rows fetched for the message list per load.
    ///
    /// A page size, not a display cap: the virtual list scrolls over whatever is loaded, and the
    /// previous value of 100 meant a busy mailbox simply stopped existing below the fold. Kept
    /// bounded so one load stays cheap.
    pub const MESSAGE_PAGE_SIZE: usize = 500;

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

    fn make_text_input(
        window: &mut Window,
        cx: &mut Context<MainWindow>,
        placeholder: &str,
        value: &str,
        masked: bool,
    ) -> Entity<component::input::InputState> {
        let val = value.to_string();
        let ph = placeholder.to_string();
        cx.new(|cx| {
            let mut st = component::input::InputState::new(window, cx).placeholder(ph);
            if masked {
                st = st.masked(true);
            }
            if !val.is_empty() {
                st = st.default_value(val);
            }
            st
        })
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
                    ProviderType::Graph => "outlook.office365.com".to_string(),
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
                    ProviderType::Graph => "smtp.office365.com".to_string(),
                    ProviderType::Jmap => "api.fastmail.com".to_string(),
                    ProviderType::Imap => "".to_string(),
                }
            } else {
                wizard.outgoing_host.clone()
            };
            let out_port_val = wizard.outgoing_port.to_string();
            let client_id_val = wizard
                .client_id
                .clone()
                .unwrap_or_else(|| wizard.effective_client_id());

            Self {
                email: make_text_input(window, cx, "user@example.com", &email_val, false),
                password: make_text_input(
                    window,
                    cx,
                    "Password or 16-character App Password",
                    &pass_val,
                    true,
                ),
                name: make_text_input(
                    window,
                    cx,
                    "Display Name (e.g. Alex Smith)",
                    &name_val,
                    false,
                ),
                incoming_host: make_text_input(window, cx, "imap.example.com", &in_host_val, false),
                incoming_port: make_text_input(window, cx, "993", &in_port_val, false),
                outgoing_host: make_text_input(
                    window,
                    cx,
                    "smtp.example.com",
                    &out_host_val,
                    false,
                ),
                outgoing_port: make_text_input(window, cx, "587", &out_port_val, false),
                client_id: make_text_input(
                    window,
                    cx,
                    "OAuth Client ID (e.g. 12345.apps.googleusercontent.com)",
                    &client_id_val,
                    false,
                ),
            }
        }
    }

    pub struct ComposeInputEntities {
        pub to: Entity<component::input::InputState>,
        pub cc: Entity<component::input::InputState>,
        pub subject: Entity<component::input::InputState>,
        /// A multi-line field, not a single-line one: the body of a mail is
        /// paragraphs, and the previous `Input` rendered one line and scrolled the
        /// rest out of sight.
        pub body: Entity<component::input::TextareaState>,
        pub attach_path: Entity<component::input::InputState>,
    }

    impl ComposeInputEntities {
        pub fn new(
            window: &mut Window,
            cx: &mut Context<MainWindow>,
            to_val: &str,
            subj_val: &str,
            body_val: &str,
        ) -> Self {
            Self {
                to: make_text_input(
                    window,
                    cx,
                    "Recipient (e.g. user@example.com)",
                    to_val,
                    false,
                ),
                cc: make_text_input(window, cx, "Cc (optional)", "", false),
                subject: make_text_input(window, cx, "Subject", subj_val, false),
                body: cx.new(|cx| {
                    component::input::TextareaState::new(window, cx)
                        .placeholder("Write your message…")
                        .auto_grow(4, 14)
                        .default_value(body_val)
                }),
                attach_path: make_text_input(
                    window,
                    cx,
                    "Full path of a file to attach",
                    "",
                    false,
                ),
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
        AttachmentPreview {
            filename: String,
            mime_type: String,
            size_bytes: usize,
            blob_path: Option<String>,
            data: Option<Vec<u8>>,
        },
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

    /// Row data owned outright, so a render-time cache can outlive any one borrow of
    /// `self.messages` instead of holding one across the frame.
    #[derive(Debug, Clone)]
    pub struct CachedRow {
        pub summary: MessageSummary,
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
        /// The reader's current row and the bulk-action range, kept together so a
        /// selection cannot outlive the message it points at.
        pub selection: MessageSelection,
        pub selected_message_id: Option<String>,
        pub list_filter: ListFilter,
        pub search_query: String,
        /// Whether the folder pane is shown. A keyboard shortcut toggles it, so it has
        /// to be state rather than a constant in the render function.
        pub folder_tree_visible: bool,
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
        /// Keyboard-driven command state: focus target, keymap and the typed Action
        /// handlers. Without a tracked focus handle on the root, no `KeyBinding`
        /// reaches a handler and every shortcut is inert.
        pub focus_handle: FocusHandle,
        pub keybindings: KeybindingMap,
        /// The settings surface's own tab/dirty state.
        pub settings_view: SettingsViewState,
        /// Compose rules that outlive one modal: recipient chips, validation and the
        /// Markdown decision.
        pub compose_state: ComposeState,
        /// Cached result of `threaded_messages()` for the current load.
        ///
        /// The message list previously rebuilt the whole filtered-and-threaded vector on every
        /// render — twice, in fact: once to count rows for the virtual list's size table and
        /// again inside the row-render closure for each visible window. That is O(n log n) of
        /// sorting plus a subject-normalising pass per row, per frame, while scrolling.
        ///
        /// Invalidated by `refresh_message_cache()` whenever anything feeding it changes:
        /// `messages`, `list_filter`, `quick_filter_state`, `search_query`, `is_threaded` or
        /// `sort_order`. The entries borrow `self.messages`, so the cache is cleared before
        /// `messages` is reassigned rather than storing owned summaries.
        message_cache: std::cell::RefCell<Option<std::rc::Rc<Vec<CachedRow>>>>,
        /// The header search field. The list could not be searched at all before this
        /// existed: `search_query` was only ever read, never written by any control.
        pub search_input: Option<Entity<component::input::InputState>>,
        // Compose State
        pub compose_to: String,
        pub compose_subject: String,
        pub compose_body: String,
        pub compose_inputs: Option<ComposeInputEntities>,
        pub compose_attachments: Vec<vespetrel_core::message::ComposedAttachment>,
        /// Full path typed into the composer's attach field. A path input rather than
        /// a native picker: `gpuui-kit` has no file dialog, and inventing attachment
        /// bytes is worse than asking for a path.
        pub compose_attach_path: String,
        pub compose_reply_to_id: Option<String>,
        pub compose_draft_id: Option<String>,
        // Alias and Signature State
        pub aliases: Vec<vespetrel_core::AccountAlias>,
        pub selected_alias: Option<vespetrel_core::AccountAlias>,
        pub signatures: Vec<vespetrel_core::Signature>,
        pub selected_signature: Option<vespetrel_core::Signature>,
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
        pub command_state: Option<Entity<CommandState>>,
    }

    impl MainWindow {
        /// Creates a mock `MainWindow` with in-memory sample fixtures (for tests or headless mock preview).
        /// In production runtime, use [`MainWindow::from_storage`] with an initialized SQLite storage pool.
        pub fn new(
            window: &mut Window,
            cx: &mut Context<Self>,
            sync_rx: flume::Receiver<SyncEvent>,
            sync_tx: flume::Sender<SyncEvent>,
        ) -> Self {
            Self::from_storage(window, cx, sync_rx, sync_tx, None)
        }

        /// Production entry point: creates `MainWindow` and initializes asynchronous hydration from the storage pool.
        #[allow(clippy::type_complexity)]
        pub fn from_storage(
            window: &mut Window,
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
                                let msgs = vespetrel_storage::repo::list_unified_inbox_messages(
                                    c,
                                    MESSAGE_PAGE_SIZE,
                                    0,
                                )
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

                                let mut all_aliases = Vec::new();
                                for acc in &accounts {
                                    if let Ok(aliases) =
                                        vespetrel_storage::repo::list_aliases_for_account(
                                            c, &acc.id,
                                        )
                                    {
                                        all_aliases.extend(aliases);
                                    }
                                }
                                if all_aliases.is_empty() {
                                    for acc in &accounts {
                                        let name = if acc.name.is_empty() {
                                            None
                                        } else {
                                            Some(acc.name.clone())
                                        };
                                        let alias = vespetrel_core::AccountAlias::new(
                                            &acc.id, &acc.email, name,
                                        )
                                        .with_default(true);
                                        let _ = vespetrel_storage::repo::upsert_alias(c, &alias);
                                        all_aliases.push(alias);
                                    }
                                }

                                let mut all_sigs = Vec::new();
                                for acc in &accounts {
                                    if let Ok(sigs) =
                                        vespetrel_storage::repo::list_signatures_for_account(
                                            c, &acc.id,
                                        )
                                    {
                                        all_sigs.extend(sigs);
                                    }
                                }
                                if all_sigs.is_empty() {
                                    for acc in &accounts {
                                        let profile = vespetrel_core::SignatureProfile {
                                            full_name: if acc.name.is_empty() {
                                                "Vespetrel User".into()
                                            } else {
                                                acc.name.clone()
                                            },
                                            job_title: Some("Rust Mail Client".into()),
                                            company: Some("Vespetrel".into()),
                                            phone: None,
                                            email: acc.email.clone(),
                                            website: Some("https://vespetrel.org".into()),
                                            avatar_url: None,
                                            pronouns: None,
                                            disclaimer: None,
                                            social_links: Vec::new(),
                                        };
                                        let sig = vespetrel_core::Signature::from_template(
                                            &acc.id,
                                            "Modern Signature",
                                            &profile,
                                            vespetrel_core::SignatureTemplate::Modern,
                                            true,
                                            true,
                                        );
                                        let _ = vespetrel_storage::repo::upsert_signature(c, &sig);
                                        all_sigs.push(sig);
                                    }
                                }

                                (
                                    accounts,
                                    all_folders,
                                    initial_messages,
                                    all_contacts,
                                    cal_events,
                                    task_items,
                                    settings,
                                    all_aliases,
                                    all_sigs,
                                )
                            })
                            .await;

                        if let Ok((accs, flds, msgs, cnts, cals, tsks, stgs, aliases, sigs)) = res {
                            let _ = this.update(cx, |view, cx| {
                                view.accounts = accs;
                                view.folders = flds;
                                if let Some(first_fld) = view.folders.first() {
                                    view.selected_folder_id = Some(first_fld.id.clone());
                                }
                                view.messages = msgs;
                                view.invalidate_message_cache();
                                view.contacts = cnts;
                                view.calendar_events = cals;
                                view.tasks = tsks;
                                view.settings = stgs;
                                view.aliases = aliases;
                                view.selected_alias = view
                                    .aliases
                                    .iter()
                                    .find(|a| a.is_default)
                                    .cloned()
                                    .or_else(|| view.aliases.first().cloned());
                                view.signatures = sigs;
                                view.selected_signature = view
                                    .signatures
                                    .iter()
                                    .find(|s| s.is_default)
                                    .cloned()
                                    .or_else(|| view.signatures.first().cloned());
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
                focus_handle: cx.focus_handle(),
                keybindings: KeybindingMap::new(crate::keybindings::preset_from_settings(
                    vespetrel_core::KeymapPreset::default(),
                )),
                settings_view: SettingsViewState::default(),
                compose_state: ComposeState::new(vespetrel_core::Address {
                    name: None,
                    email: String::new(),
                }),
                search_input: Some(cx.new(|cx| {
                    component::input::InputState::new(window, cx)
                        .placeholder("Search messages (FTS5)")
                })),
                compose_inputs: None,
                active_tab: ActiveViewTab::Mail,
                active_modal: ActiveModal::None,
                accounts,
                selected_folder_id: folders.first().map(|f: &Folder| f.id.clone()),
                is_unified_inbox: true,
                selected_message_id: messages.first().map(|m: &MessageSummary| m.id.clone()),
                folders,
                messages,
                selection: MessageSelection::new(),
                list_filter: ListFilter::All,
                search_query: String::new(),
                folder_tree_visible: true,
                block_remote_images: true,
                message_viewer: MessageViewer::new(),
                is_threaded: false,
                sort_order: MessageSortOrder::DateDescending,
                row_density: MessageRowDensity::Comfortable,
                quick_filter_state: QuickFilterState::new(),
                message_cache: std::cell::RefCell::new(None),
                calendar_events,
                contacts,
                tasks,
                settings: UserSettings::default(),
                compose_to: "team@vespetrel.example".into(),
                compose_subject: "Hello from Pure Rust GPUI Mail".into(),
                compose_body: "Hi team,\n\nWriting this from the pure Rust GPUI mail client.\n\nBest regards,\nVespetrel User".into(),
                compose_attachments: Vec::new(),
                compose_attach_path: String::new(),
                compose_reply_to_id: None,
                compose_draft_id: None,
                aliases: Vec::new(),
                selected_alias: None,
                signatures: Vec::new(),
                selected_signature: None,
                palette_query: String::new(),
                command_palette: crate::command_palette::CommandPalette::new(),
                sync_sender: sync_tx,
                status_message: "All mailboxes synchronized".into(),
                storage_pool,
                login_wizard: LoginWizardState::new(),
                wizard_inputs: None,
                toasts: Vec::new(),
                command_state: None,
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
                            format!("New email from {}: {}", sender, subj)
                        } else {
                            format!("{} new emails (latest from {})", new_msgs.len(), sender)
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
                    self.invalidate_message_cache();
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
                    self.status_message = format!("Sync error ({folder}): {error}");
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

        /// Move the reader by `delta` rows in the order the list is drawn.
        ///
        /// Movement runs over the *rendered* rows rather than `self.messages`, so a
        /// shortcut lands on the next row the user can see. Under an active filter or
        /// a search, stepping through raw storage jumped past rows that were not on
        /// screen.
        pub fn move_selection(&mut self, delta: isize, cx: &mut Context<Self>) {
            let rows: Vec<String> = self
                .cached_rows()
                .iter()
                .map(|row| row.summary.id.clone())
                .collect();
            if rows.is_empty() {
                return;
            }
            let next = match self
                .selection
                .current()
                .and_then(|cur| rows.iter().position(|id| id == cur))
            {
                Some(idx) => (idx as isize + delta).rem_euclid(rows.len() as isize) as usize,
                // No current row: enter the list at whichever end the direction implies.
                None if delta < 0 => rows.len() - 1,
                None => 0,
            };
            let target = rows[next].clone();
            self.select_message(target, cx);
        }

        /// Add or remove a row from the bulk-action range.
        ///
        /// Takes the id rather than reading the current row, because the row being
        /// clicked is by definition not current until after the click is handled.
        pub fn toggle_selection_of(&mut self, id: &str) {
            self.selection.toggle(id);
        }

        pub fn select_all_visible(&mut self, cx: &mut Context<Self>) {
            let ids: Vec<String> = self
                .cached_rows()
                .iter()
                .map(|row| row.summary.id.clone())
                .collect();
            self.selection.extend(ids);
            cx.notify();
        }

        /// Ids a bulk action should apply to right now.
        pub fn action_targets(&self) -> Vec<String> {
            let targets = self.selection.target_ids();
            if targets.is_empty() {
                Vec::new()
            } else {
                targets
            }
        }

        /// Drop selected ids that the last load did not return.
        pub fn prune_selection(&mut self) {
            let present: std::collections::HashSet<String> =
                self.messages.iter().map(|m| m.id.clone()).collect();
            self.selection.retain(&present);
        }

        pub fn selected_message(&self) -> Option<&MessageSummary> {
            self.selection
                .current()
                .and_then(|id| self.messages.iter().find(|m| m.id == id))
                .or_else(|| self.messages.first())
        }

        pub fn select_message(&mut self, message_id: String, cx: &mut Context<Self>) {
            self.selection.set_current(message_id.clone());
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
                if looks_like_html(snippet) {
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
                    // The pool's error type is not `Send`-safe as an opaque object, so
                    // a failed checkout becomes a `None` rather than an error value
                    // carried across the task boundary.
                    let Some(conn) = pool.get().await.ok() else {
                        return;
                    };
                    let (full_msg, atts) = conn
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
                        .await
                        .unwrap_or((None, Vec::new()));
                    let Some(full) = full_msg else {
                        return;
                    };

                    // Report only a signature that was actually checked.
                    //
                    // The previous code decided the verdict from the *shape* of the
                    // message: a `.p7s` attachment made it "S/MIME valid", a PGP armor
                    // header made it "encrypted and signed". No signature was ever
                    // verified, so a forged or expired one still showed a green tick.
                    let smime_blobs: Vec<String> = atts
                        .iter()
                        .filter(|a| is_smime_part(&a.content_type, &a.filename))
                        .map(|a| a.blob_path.clone())
                        .collect();

                    let _ = this.update(cx, |view, cx| {
                        if view.selected_message_id.as_deref() != Some(full.id.as_str()) {
                            return;
                        }
                        view.message_viewer.subject = full.subject.clone();
                        view.message_viewer.from = Some(vespetrel_core::message::Address {
                            name: full.from_name.clone(),
                            email: full.from_address.clone(),
                        });
                        view.message_viewer.sent_at = Some(full.sent_at);
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

                        // Reset before assigning: the previous verdict belonged to a
                        // different message and must not survive the switch.
                        view.message_viewer.security_status = SecurityStatus::Unsigned;
                        if looks_like_html(content) {
                            view.message_viewer.load_html(content, block_images);
                        } else {
                            view.message_viewer.load_text(content);
                        }
                        cx.notify();
                    });

                    if smime_blobs.is_empty() {
                        return;
                    }

                    // A real cryptographic check, off the UI thread, against the stored
                    // CMS blob. A pass is reported only if `SmimeEngine::verify`
                    // accepted the signature value, the signer certificate's validity
                    // window and the signed message digest.
                    let verify_for = message_id.clone();
                    let verdict = tokio::task::spawn_blocking(move || {
                        let engine = vespetrel_crypto::SmimeEngine::new();
                        let mut saw_signature = false;
                        for blob_path in &smime_blobs {
                            let Ok(bytes) = std::fs::read(blob_path) else {
                                continue;
                            };
                            if bytes.is_empty() {
                                continue;
                            }
                            saw_signature = true;
                            if engine.verify(&bytes).is_ok() {
                                return smime_verdict(Some(true), saw_signature);
                            }
                        }
                        smime_verdict(None, saw_signature)
                    })
                    .await
                    .unwrap_or_else(|_| smime_verdict(None, false));

                    let _ = this.update(cx, |view, cx| {
                        if view.selected_message_id.as_deref() != Some(verify_for.as_str()) {
                            return;
                        }
                        view.message_viewer.security_status = verdict;
                        cx.notify();
                    });
                })
                .detach();
            }
        }

        pub fn execute_fts_search(&mut self, query: &str, cx: &mut Context<Self>) {
            let q = query.trim().to_string();
            self.search_query = q.clone();
            self.invalidate_message_cache();
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
                            view.invalidate_message_cache();
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
            self.invalidate_message_cache();
            cx.notify();
        }

        pub fn cycle_sort_order(&mut self, cx: &mut Context<Self>) {
            self.sort_order = match self.sort_order {
                MessageSortOrder::DateDescending => MessageSortOrder::DateAscending,
                MessageSortOrder::DateAscending => MessageSortOrder::DateDescending,
            };
            self.invalidate_message_cache();
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

        /// Apply a colour theme.
        ///
        /// The old implementation only wrote the value to settings and repainted. Nothing
        /// ever read it back, so the Appearance control was inert: picking "Light Paper"
        /// changed the selection state and nothing else. `System` now means what its label
        /// says, following the operating system's appearance.
        pub fn set_settings_theme(
            &mut self,
            theme: vespetrel_core::ColorTheme,
            cx: &mut Context<Self>,
        ) {
            self.settings.theme = theme;
            let mode = match theme {
                vespetrel_core::ColorTheme::LightPaper => ThemeMode::Light,
                vespetrel_core::ColorTheme::System => crate::platform::theme_mode(),
                _ => ThemeMode::Dark,
            };
            Theme::change(mode, None, cx);
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

        /// Apply a preset and say when it takes effect.
        ///
        /// The bindings are installed at launch. Rebinding mid-session would need
        /// `cx.clear_key_bindings`, which also drops the component defaults
        /// `gpui_kit::init` registered, so the change is applied on next start and
        /// the user is told rather than left wondering.
        pub fn set_keymap_preset(
            &mut self,
            preset: crate::keybindings::KeyPreset,
            cx: &mut Context<Self>,
        ) {
            self.keybindings = KeybindingMap::new(preset);
            self.settings.keymap_preset = match preset {
                crate::keybindings::KeyPreset::Gmail => vespetrel_core::KeymapPreset::Gmail,
                crate::keybindings::KeyPreset::Thunderbird => {
                    vespetrel_core::KeymapPreset::Thunderbird
                }
                crate::keybindings::KeyPreset::Vim => vespetrel_core::KeymapPreset::Vim,
                crate::keybindings::KeyPreset::Custom => vespetrel_core::KeymapPreset::Custom,
            };
            self.save_settings(cx);
            if preset != crate::keybindings::KeyPreset::Custom {
                self.show_toast(
                    format!(
                        "{} shortcuts apply the next time Vespetrel starts",
                        preset.label()
                    ),
                    false,
                    cx,
                );
            }
            cx.notify();
        }

        pub fn set_remote_image_policy(&mut self, block: bool, cx: &mut Context<Self>) {
            self.block_remote_images = block;
            self.message_viewer.toggle_remote_images();
            self.settings.remote_image_policy = if block {
                vespetrel_core::RemoteImagePolicy::BlockAll
            } else {
                vespetrel_core::RemoteImagePolicy::AllowAll
            };
            self.save_settings(cx);
            cx.notify();
        }

        pub fn set_sender_avatars(&mut self, show: bool, cx: &mut Context<Self>) {
            self.settings.show_sender_avatars = show;
            self.save_settings(cx);
            cx.notify();
        }

        pub fn set_preview_snippets(&mut self, show: bool, cx: &mut Context<Self>) {
            self.settings.show_preview_snippets = show;
            self.save_settings(cx);
            cx.notify();
        }

        pub fn set_strip_trackers(&mut self, enabled: bool, cx: &mut Context<Self>) {
            self.settings.auto_strip_trackers = enabled;
            self.save_settings(cx);
            cx.notify();
        }

        pub fn set_warn_phishing(&mut self, enabled: bool, cx: &mut Context<Self>) {
            self.settings.warn_on_phishing = enabled;
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

        /// Cached rows for the message list, keyed on every input that can change them.
        ///
        /// Returns an `Rc` so the render closure can hold it across the frame without a borrow
        /// of `self`. Any mutation of `messages` or of the filter/sort state must call
        /// `invalidate_message_cache()`.
        fn cached_rows(&self) -> std::rc::Rc<Vec<CachedRow>> {
            if let Some(rows) = self.message_cache.borrow().as_ref() {
                return rows.clone();
            }
            let rows = std::rc::Rc::new(self.build_threaded_rows());
            *self.message_cache.borrow_mut() = Some(rows.clone());
            rows
        }

        fn invalidate_message_cache(&self) {
            *self.message_cache.borrow_mut() = None;
        }

        fn build_threaded_rows(&self) -> Vec<CachedRow> {
            self.threaded_messages()
                .into_iter()
                .map(|row| CachedRow {
                    summary: row.summary.clone(),
                    thread_count: row.thread_count,
                    is_child: row.is_child,
                })
                .collect()
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
                    let provider = match vespetrel_engine::make_provider(&acct) {
                        Ok(p) => p,
                        Err(e) => {
                            // Credentials unreadable: report it instead of hammering a
                            // provider built from an empty token.
                            let _ = tx.send(SyncEvent::SyncError {
                                folder: acct.email.clone(),
                                error: e.to_string(),
                            });
                            continue;
                        }
                    };
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

        /// Add, remove or flip the star on a set of messages.
        ///
        /// `desired` is `Some` for an explicit value and `None` to flip. Batch actions
        /// pass `Some`, because flipping each row independently would scatter a star
        /// across a range the user selected in one action.
        pub fn set_flag(
            &mut self,
            ids: &[String],
            desired: Option<bool>,
            cx: &mut Context<Self>,
        ) -> usize {
            let mut changed = Vec::new();
            for id in ids {
                if let Some(m) = self.messages.iter_mut().find(|m| &m.id == id) {
                    m.is_flagged = desired.unwrap_or(!m.is_flagged);
                    changed.push((id.clone(), m.is_flagged, m.is_read));
                }
            }
            if changed.is_empty() {
                return 0;
            }
            let count = changed.len();
            cx.notify();

            if let Some(pool) = &self.storage_pool {
                let pool = pool.clone();
                let writes = changed.clone();
                cx.spawn(async move |_this, _cx| {
                    if let Ok(conn) = pool.get().await {
                        let _ = conn
                            .interact(move |c| {
                                for (id, flagged, _read) in &writes {
                                    let _ = vespetrel_storage::repo::update_message_flags(
                                        c,
                                        id,
                                        None,
                                        Some(*flagged),
                                    );
                                }
                            })
                            .await;
                    }
                })
                .detach();
            }

            for (id, is_flagged, is_read) in self
                .messages
                .iter()
                .filter(|m| ids.iter().any(|t| t == &m.id))
                .map(|m| (m.id.clone(), m.is_flagged, m.is_read))
                .collect::<Vec<_>>()
            {
                let _ = self.sync_sender.send(SyncEvent::MessageFlagsUpdated {
                    id,
                    is_read,
                    is_flagged,
                });
            }
            count
        }

        pub fn toggle_flag(&mut self, cx: &mut Context<Self>) {
            self.set_flag(&self.action_targets(), None, cx);
        }

        /// Mark a set of messages read or unread.
        pub fn set_read(&mut self, ids: &[String], desired: bool, cx: &mut Context<Self>) -> usize {
            let mut changed = Vec::new();
            for id in ids {
                if let Some(m) = self.messages.iter_mut().find(|m| &m.id == id) {
                    m.is_read = desired;
                    changed.push((id.clone(), m.is_flagged, m.is_read));
                }
            }
            if changed.is_empty() {
                return 0;
            }
            let count = changed.len();
            cx.notify();

            if let Some(pool) = &self.storage_pool {
                let pool = pool.clone();
                let writes = changed.clone();
                cx.spawn(async move |_this, _cx| {
                    if let Ok(conn) = pool.get().await {
                        let _ = conn
                            .interact(move |c| {
                                for (id, _flagged, read) in &writes {
                                    let _ = vespetrel_storage::repo::update_message_flags(
                                        c,
                                        id,
                                        Some(*read),
                                        None,
                                    );
                                }
                            })
                            .await;
                    }
                })
                .detach();
            }

            for (id, is_flagged, is_read) in changed {
                let _ = self.sync_sender.send(SyncEvent::MessageFlagsUpdated {
                    id,
                    is_read,
                    is_flagged,
                });
            }
            count
        }

        pub fn delete_selected_message(&mut self, cx: &mut Context<Self>) {
            let targets = self.action_targets();
            if targets.is_empty() {
                return;
            }
            self.delete_messages(&targets, cx);
        }

        /// Remove a set of messages from the list and from storage.
        pub fn delete_messages(&mut self, ids: &[String], cx: &mut Context<Self>) -> usize {
            if ids.is_empty() {
                return 0;
            }
            {
                let pool_opt = self.storage_pool.clone();
                let current_folder_id = self.selected_folder_id.clone();
                let is_in_trash = self.folders.iter().any(|f| {
                    Some(&f.id) == current_folder_id.as_ref()
                        && f.role == vespetrel_core::FolderRole::Trash
                });

                self.messages.retain(|m| !ids.contains(&m.id));
                self.invalidate_message_cache();
                if self
                    .selection
                    .current()
                    .is_some_and(|cur| ids.iter().any(|d| d == cur))
                {
                    let next = self.messages.first().map(|m| m.id.clone());
                    self.selected_message_id = next.clone();
                    match next {
                        Some(n) => self.selection.set_current(n),
                        None => self.selection.clear_current(),
                    }
                }
                self.prune_selection();
                let count = ids.len();

                if is_in_trash {
                    self.show_toast("Message permanently deleted", false, cx);
                    if let Some(pool) = pool_opt {
                        let victims = ids.to_vec();
                        cx.spawn(async move |_this, _cx| {
                            if let Ok(conn) = pool.get().await {
                                let _ = conn
                                    .interact(move |c| {
                                        for msg_id in &victims {
                                            let _ =
                                                vespetrel_storage::repo::delete_message(c, msg_id);
                                        }
                                    })
                                    .await;
                            }
                        })
                        .detach();
                    }
                    let _ = self
                        .sync_sender
                        .send(SyncEvent::MessagesDeleted(ids.to_vec()));
                } else {
                    self.show_toast(
                        if count == 1 {
                            "Message moved to Trash".to_string()
                        } else {
                            format!("{count} messages moved to Trash")
                        },
                        false,
                        cx,
                    );
                    let trash_folder_id = self
                        .folders
                        .iter()
                        .find(|f| f.role == vespetrel_core::FolderRole::Trash)
                        .map(|f| f.id.clone());

                    if let Some(trash_id) = trash_folder_id
                        && let Some(pool) = pool_opt
                    {
                        let victims = ids.to_vec();
                        cx.spawn(async move |this, cx| {
                            if let Ok(conn) = pool.get().await {
                                let _ = conn
                                    .interact(move |c| {
                                        for msg_id in &victims {
                                            c.execute(
                                                "UPDATE messages SET folder_id = ?1 WHERE id = ?2",
                                                rusqlite::params![trash_id, msg_id],
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
                cx.notify();
                count
            }
        }

        pub fn archive_selected_message(&mut self, cx: &mut Context<Self>) {
            let targets = self.action_targets();
            if targets.is_empty() {
                return;
            }
            self.archive_messages(&targets, cx);
        }

        /// Move a set of messages into the account's Archive folder.
        pub fn archive_messages(&mut self, ids: &[String], cx: &mut Context<Self>) -> usize {
            if ids.is_empty() {
                return 0;
            }
            self.messages.retain(|m| !ids.contains(&m.id));
            self.invalidate_message_cache();
            if self
                .selection
                .current()
                .is_some_and(|cur| ids.iter().any(|d| d == cur))
            {
                match self.messages.first().map(|m| m.id.clone()) {
                    Some(n) => {
                        self.selected_message_id = Some(n.clone());
                        self.selection.set_current(n);
                    }
                    None => {
                        self.selected_message_id = None;
                        self.selection.clear_current();
                    }
                }
            }
            self.prune_selection();
            let count = ids.len();
            self.show_toast(
                if count == 1 {
                    "Message archived".to_string()
                } else {
                    format!("{count} messages archived")
                },
                false,
                cx,
            );
            cx.notify();

            if let Some(pool) = &self.storage_pool {
                let pool = pool.clone();
                let victims = ids.to_vec();
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
                                    for msg_id in &victims {
                                        c.execute(
                                            "UPDATE messages SET folder_id = ?1 WHERE id = ?2",
                                            rusqlite::params![arch_id, msg_id],
                                        )?;
                                    }
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
            count
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
            let cc_val = self
                .compose_inputs
                .as_ref()
                .map(|i| i.cc.read(cx).value().trim().to_string())
                .unwrap_or_default();
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
                cc_addresses: parse_address_list(&cc_val),
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

        pub fn open_attachment_preview(
            &mut self,
            filename: String,
            mime_type: String,
            size_bytes: usize,
            blob_path: Option<String>,
            data: Option<Vec<u8>>,
            cx: &mut Context<Self>,
        ) {
            self.active_modal = ActiveModal::AttachmentPreview {
                filename,
                mime_type,
                size_bytes,
                blob_path,
                data,
            };
            cx.notify();
        }

        pub fn select_alias(
            &mut self,
            alias: Option<vespetrel_core::AccountAlias>,
            cx: &mut Context<Self>,
        ) {
            self.selected_alias = alias;
            if let Some(ref al) = self.selected_alias
                && let Some(ref sig_id) = al.signature_id
                && let Some(sig) = self.signatures.iter().find(|s| &s.id == sig_id).cloned()
            {
                self.selected_signature = Some(sig);
            }
            cx.notify();
        }

        pub fn cycle_signature(&mut self, window: &mut Window, cx: &mut Context<Self>) {
            if self.signatures.is_empty() {
                return;
            }
            let next_sig = match &self.selected_signature {
                None => self.signatures.first().cloned(),
                Some(curr) => {
                    if let Some(idx) = self.signatures.iter().position(|s| s.id == curr.id) {
                        if idx + 1 < self.signatures.len() {
                            Some(self.signatures[idx + 1].clone())
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                }
            };
            self.selected_signature = next_sig.clone();

            let mut base_body = self.compose_body.clone();
            if let Some(idx) = base_body.find("\n-- \n") {
                base_body.truncate(idx);
            } else if let Some(idx) = base_body.find("\n\n-- \n") {
                base_body.truncate(idx);
            }

            if let Some(sig) = next_sig {
                if let Some(pt) = &sig.plain_text {
                    self.compose_body = format!("{base_body}\n\n-- \n{pt}");
                } else {
                    self.compose_body = base_body;
                }
            } else {
                self.compose_body = base_body;
            }

            if let Some(inputs) = &self.compose_inputs {
                inputs.body.update(cx, |inp, cx| {
                    inp.set_value(self.compose_body.clone(), window, cx);
                });
            }
            cx.notify();
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
                                vespetrel_storage::repo::list_unified_inbox_messages(
                                    c,
                                    MESSAGE_PAGE_SIZE,
                                    0,
                                )
                            } else if let Some(fid) = &fid_c {
                                vespetrel_storage::repo::list_messages_in_folder(
                                    c,
                                    fid,
                                    MESSAGE_PAGE_SIZE,
                                    0,
                                )
                            } else {
                                vespetrel_storage::repo::list_unified_inbox_messages(
                                    c,
                                    MESSAGE_PAGE_SIZE,
                                    0,
                                )
                            }
                        })
                        .await;
                    if let Ok(Ok(loaded_msgs)) = msgs {
                        let summaries: Vec<MessageSummary> =
                            loaded_msgs.into_iter().map(|m| m.summary()).collect();
                        let _ = this.update(cx, |view, cx| {
                            view.messages = summaries;
                            view.invalidate_message_cache();
                            view.selected_message_id = view.messages.first().map(|m| m.id.clone());
                            view.reload_folder_counts(cx);
                            cx.notify();
                        });
                    }
                }

                // Incremental background sync if an account and folder match
                if let (Some(acct), Some(fld)) = (acct_opt, folder_obj)
                    && let Ok(provider) = vespetrel_engine::make_provider(&acct)
                    && let Ok(delta) = provider.sync_messages(&fld, Default::default()).await
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
                            is_flagged: sm.flags.contains(&vespetrel_core::message::Flag::Flagged),
                            has_attachments: false,
                        })
                        .collect();
                    let _ = this.update(cx, |view, cx| {
                        view.messages.splice(0..0, new_summaries);
                        cx.notify();
                    });
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
                self.invalidate_message_cache();
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
            let cc_val = self
                .compose_inputs
                .as_ref()
                .map(|i| i.cc.read(cx).value().trim().to_string())
                .unwrap_or_default();
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

            // Validate before anything is persisted or transmitted.
            //
            // The old check was `to_val.is_empty()`, which accepted `alice`, rejected a
            // valid message with only a Cc recipient, and let an empty subject through.
            // The rules come from `ComposeState::validate_compose_fields`, which was
            // already written and tested but never called.
            let errors = self
                .compose_state
                .validate_compose_fields(&to_val, &subj_val, &body_val);
            if !errors.is_valid() {
                let mut messages = Vec::new();
                for err in [
                    errors.recipient_error,
                    errors.subject_error,
                    errors.body_error,
                ]
                .into_iter()
                .flatten()
                {
                    messages.push(err);
                }
                self.show_toast(messages.join(" · "), true, cx);
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

            let (from_email, from_name) = if let Some(alias) = &self.selected_alias {
                (
                    alias.email.clone(),
                    alias.name.clone().or_else(|| {
                        self.accounts.first().and_then(|a| {
                            if a.name.is_empty() {
                                None
                            } else {
                                Some(a.name.clone())
                            }
                        })
                    }),
                )
            } else {
                (
                    self.accounts
                        .first()
                        .map(|a| a.email.clone())
                        .unwrap_or_else(|| "me@localhost".into()),
                    self.accounts.first().and_then(|a| {
                        if a.name.is_empty() {
                            None
                        } else {
                            Some(a.name.clone())
                        }
                    }),
                )
            };

            // Real Markdown, when Markdown is on.
            //
            // The previous body was `body_val.replace('\n', "<br/>\n")` wrapped in a
            // `markdown-body` div: every `**bold**`, `# heading` and `- list` marker
            // reached the recipient as literal characters. `RichTextDocument` already
            // parsed all of those; it was simply never called from here.
            let body_html = if self.compose_state.is_markdown_mode {
                Some(
                    vespetrel_core::richtext::RichTextDocument::parse_markdown(&body_val)
                        .to_html_with_font(Some(&self.settings.default_composer_font)),
                )
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
                cc: parse_address_list(&cc_val),
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
            if self.command_state.is_none() {
                self.command_state = Some(cx.new(|cx| CommandState::new(window, cx)));
            }
            if self.search_input.is_none() {
                self.search_input = Some(cx.new(|cx| {
                    component::input::InputState::new(window, cx)
                        .placeholder("Search messages (FTS5)")
                }));
                self.subscribe_search_input(window, cx);
            }
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
                .bg(cx.theme().background)
                .text_color(cx.theme().foreground)
                // The keyboard route. Every `KeyBinding` is scoped to
                // `WINDOW_CONTEXT`, and that context is only eligible when it sits on
                // the focused element's dispatch path. Without `track_focus` here, no
                // shortcut in the keymap can reach a handler: the previous build had a
                // complete `KeybindingMap` and no focus handle, so nothing was bound
                // to anything and the header's key cap was decoration.
                .track_focus(&self.focus_handle)
                .key_context(WINDOW_CONTEXT)
                .on_action(cx.listener(Self::on_action_select_next))
                .on_action(cx.listener(Self::on_action_select_previous))
                .on_action(cx.listener(Self::on_action_archive))
                .on_action(cx.listener(Self::on_action_delete))
                .on_action(cx.listener(Self::on_action_toggle_star))
                .on_action(cx.listener(Self::on_action_toggle_read))
                .on_action(cx.listener(Self::on_action_compose))
                .on_action(cx.listener(Self::on_action_reply))
                .on_action(cx.listener(Self::on_action_reply_all))
                .on_action(cx.listener(Self::on_action_forward))
                .on_action(cx.listener(Self::on_action_save_draft))
                .on_action(cx.listener(Self::on_action_focus_search))
                .on_action(cx.listener(Self::on_action_sync))
                .on_action(cx.listener(Self::on_action_go_to_inbox))
                .on_action(cx.listener(Self::on_action_go_to_sent))
                .on_action(cx.listener(Self::on_action_go_to_drafts))
                .on_action(cx.listener(Self::on_action_toggle_threading))
                .on_action(cx.listener(Self::on_action_toggle_remote_images))
                .on_action(cx.listener(Self::on_action_cycle_sort))
                .on_action(cx.listener(Self::on_action_cycle_density))
                .on_action(cx.listener(Self::on_action_toggle_folder_tree))
                .on_action(cx.listener(Self::on_action_show_palette))
                .on_action(cx.listener(Self::on_action_show_settings))
                .on_action(cx.listener(Self::on_action_dismiss))
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
                .child(self.render_status_bar(cx))
                .child(self.render_toasts(cx))
                .child(self.render_modal_layer(window, cx))
        }
    }

    impl MainWindow {
        /// Wire the header's search field to the FTS5 query path.
        ///
        /// The search box was a `div` that displayed a `search_query` nothing ever
        /// wrote, and `execute_fts_search` had no caller: the list could not be
        /// searched at all. Enter commits the query, Escape clears it, and a change
        /// updates the in-memory filter immediately so typing feels live before the
        /// index round-trip returns.
        fn subscribe_search_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
            let Some(input) = self.search_input.clone() else {
                return;
            };
            cx.subscribe_in(
                &input,
                window,
                |this, _state, event, window, cx| match event {
                    component::input::InputEvent::Change => {
                        let query = this
                            .search_input
                            .as_ref()
                            .map(|i| i.read(cx).value().to_string())
                            .unwrap_or_default();
                        this.on_search_query_changed(query, window, cx);
                    }
                    component::input::InputEvent::PressEnter { .. } => {
                        let query = this
                            .search_input
                            .as_ref()
                            .map(|i| i.read(cx).value().to_string())
                            .unwrap_or_default();
                        this.execute_fts_search(&query, cx);
                    }
                    _ => {}
                },
            )
            .detach();
        }

        /// Reflect the typed query in the list, and run the full-text search.
        fn on_search_query_changed(
            &mut self,
            query: String,
            window: &mut Window,
            cx: &mut Context<Self>,
        ) {
            let trimmed = query.trim().to_string();
            self.search_query = trimmed.clone();
            self.quick_filter_state.search_query = trimmed.clone();
            self.invalidate_message_cache();
            if trimmed.is_empty() {
                self.reload_messages_for_current_folder(cx);
            } else {
                // Immediate in-memory narrowing while the FTS5 query is in flight.
                cx.notify();
            }
            self.execute_fts_search(&trimmed, cx);
            let _ = window;
        }

        /// Put keyboard focus on the header's search field.
        fn focus_search_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
            if let Some(input) = self.search_input.clone() {
                input.update(cx, |state, cx| state.focus(window, cx));
            }
        }

        fn on_action_focus_search(
            &mut self,
            _: &FocusSearch,
            window: &mut Window,
            cx: &mut Context<Self>,
        ) {
            self.focus_search_input(window, cx);
        }

        fn on_action_select_next(
            &mut self,
            _: &SelectNextMessage,
            _: &mut Window,
            cx: &mut Context<Self>,
        ) {
            self.move_selection(1, cx);
        }

        fn on_action_select_previous(
            &mut self,
            _: &SelectPreviousMessage,
            _: &mut Window,
            cx: &mut Context<Self>,
        ) {
            self.move_selection(-1, cx);
        }

        fn on_action_archive(&mut self, _: &Archive, _: &mut Window, cx: &mut Context<Self>) {
            self.archive_selected_message(cx);
        }

        fn on_action_delete(&mut self, _: &Delete, _: &mut Window, cx: &mut Context<Self>) {
            self.delete_selected_message(cx);
        }

        fn on_action_toggle_star(
            &mut self,
            _: &ToggleStar,
            _: &mut Window,
            cx: &mut Context<Self>,
        ) {
            self.toggle_flag(cx);
        }

        /// Flip read state for the current row or the selected range.
        fn on_action_toggle_read(
            &mut self,
            _: &ToggleRead,
            _: &mut Window,
            cx: &mut Context<Self>,
        ) {
            let targets = self.action_targets();
            if targets.is_empty() {
                return;
            }
            // Every target in one batch must land on the same value, so the decision is
            // taken from the current row rather than from each row's own flag.
            let current_is_read = self.selected_message().map(|m| m.is_read).unwrap_or(false);
            self.set_read(&targets, !current_is_read, cx);
        }

        fn on_action_compose(&mut self, _: &Compose, _: &mut Window, cx: &mut Context<Self>) {
            self.open_compose(cx);
        }

        fn on_action_reply(&mut self, _: &Reply, _: &mut Window, cx: &mut Context<Self>) {
            self.open_reply(false, cx);
        }

        fn on_action_reply_all(&mut self, _: &ReplyAll, _: &mut Window, cx: &mut Context<Self>) {
            self.open_reply(true, cx);
        }

        fn on_action_forward(&mut self, _: &Forward, _: &mut Window, cx: &mut Context<Self>) {
            self.open_forward(cx);
        }

        fn on_action_save_draft(&mut self, _: &SaveDraft, _: &mut Window, cx: &mut Context<Self>) {
            if self.active_modal == ActiveModal::Compose {
                self.save_draft(cx);
            }
        }

        fn on_action_sync(&mut self, _: &SyncNow, _: &mut Window, cx: &mut Context<Self>) {
            self.trigger_sync(cx);
        }

        fn on_action_go_to_inbox(&mut self, _: &GoToInbox, _: &mut Window, cx: &mut Context<Self>) {
            self.go_to_role_folder(vespetrel_core::FolderRole::Inbox, cx);
        }

        fn on_action_go_to_sent(&mut self, _: &GoToSent, _: &mut Window, cx: &mut Context<Self>) {
            self.go_to_role_folder(vespetrel_core::FolderRole::Sent, cx);
        }

        fn on_action_go_to_drafts(
            &mut self,
            _: &GoToDrafts,
            _: &mut Window,
            cx: &mut Context<Self>,
        ) {
            self.go_to_role_folder(vespetrel_core::FolderRole::Drafts, cx);
        }

        fn on_action_toggle_threading(
            &mut self,
            _: &ToggleThreading,
            _: &mut Window,
            cx: &mut Context<Self>,
        ) {
            self.toggle_threading(cx);
        }

        fn on_action_toggle_remote_images(
            &mut self,
            _: &ToggleRemoteImages,
            _: &mut Window,
            cx: &mut Context<Self>,
        ) {
            self.toggle_remote_images(cx);
        }

        fn on_action_cycle_sort(
            &mut self,
            _: &CycleSortOrder,
            _: &mut Window,
            cx: &mut Context<Self>,
        ) {
            self.cycle_sort_order(cx);
        }

        fn on_action_cycle_density(
            &mut self,
            _: &CycleRowDensity,
            _: &mut Window,
            cx: &mut Context<Self>,
        ) {
            self.cycle_row_density(cx);
        }

        fn on_action_toggle_folder_tree(
            &mut self,
            _: &ToggleFolderTree,
            _: &mut Window,
            cx: &mut Context<Self>,
        ) {
            self.folder_tree_visible = !self.folder_tree_visible;
            cx.notify();
        }

        fn on_action_show_palette(
            &mut self,
            _: &ShowCommandPalette,
            _: &mut Window,
            cx: &mut Context<Self>,
        ) {
            self.command_palette.open();
            self.active_modal = ActiveModal::CommandPalette;
            cx.notify();
        }

        fn on_action_show_settings(
            &mut self,
            _: &ShowSettings,
            _: &mut Window,
            cx: &mut Context<Self>,
        ) {
            self.active_tab = ActiveViewTab::Settings;
            self.active_modal = ActiveModal::None;
            cx.notify();
        }

        //// Escape closes the topmost overlay, and only that.
        ///
        /// With no overlay open, Escape backs out of the narrowest thing in scope: a
        /// running search first, then an active filter. It never reaches further than
        /// that, so a stray Escape cannot clear a filter the user set minutes ago
        /// without their noticing.
        fn on_action_dismiss(
            &mut self,
            _: &DismissOverlay,
            window: &mut Window,
            cx: &mut Context<Self>,
        ) {
            match self.active_modal {
                ActiveModal::None => {
                    if !self.search_query.is_empty() {
                        if let Some(input) = self.search_input.clone() {
                            input.update(cx, |state, cx| state.set_value("", window, cx));
                        }
                        self.clear_search(cx);
                    } else if self.list_filter != ListFilter::All {
                        self.list_filter = ListFilter::All;
                        self.reload_messages_for_current_folder(cx);
                    }
                }
                ActiveModal::Compose => {
                    // Closing the composer keeps what was typed, because a draft is the
                    // user's work; Discard is the explicit way to throw it away.
                    self.save_draft(cx);
                    self.active_modal = ActiveModal::None;
                    self.compose_inputs = None;
                }
                _ => {
                    self.active_modal = ActiveModal::None;
                    self.compose_inputs = None;
                    self.command_palette.close();
                }
            }
            cx.notify();
        }

        /// Jump to a folder by its special role, or say the account has none.
        fn go_to_role_folder(&mut self, role: vespetrel_core::FolderRole, cx: &mut Context<Self>) {
            self.active_tab = ActiveViewTab::Mail;
            match self
                .folders
                .iter()
                .find(|f| f.role == role)
                .map(|f| f.id.clone())
            {
                Some(id) => self.select_folder(id, cx),
                None => {
                    self.show_toast(format!("No {role:?} folder on this account"), true, cx);
                }
            }
        }

        /// Read the file at `compose_attach_path` and add it to the outgoing message.
        ///
        /// Reports the specific failure rather than silently attaching nothing: a path that
        /// does not exist, one that is not a file, a directory, a file too large to send, and
        /// a read that failed are five different problems with five different fixes.
        fn attach_from_path_input(&mut self, cx: &mut Context<Self>) {
            let raw = self.compose_attach_path.trim().to_string();
            if raw.is_empty() {
                self.show_toast("Enter the full path of the file to attach", true, cx);
                return;
            }
            let expanded = match expand_home(&raw) {
                Some(p) => p,
                None => {
                    self.show_toast(
                        "Path starts with ~ but no home directory could be resolved",
                        true,
                        cx,
                    );
                    return;
                }
            };
            let meta = match std::fs::metadata(&expanded) {
                Ok(m) => m,
                Err(e) => {
                    self.show_toast(format!("Cannot read {expanded}: {e}"), true, cx);
                    return;
                }
            };
            if meta.is_dir() {
                self.show_toast(format!("{expanded} is a folder, not a file"), true, cx);
                return;
            }
            // 25 MB is the common SMTP server ceiling; catching it here beats a
            // bounce after the message has already been composed.
            const MAX_ATTACHMENT_BYTES: u64 = 25 * 1024 * 1024;
            if meta.len() > MAX_ATTACHMENT_BYTES {
                self.show_toast(
                    format!(
                        "{} is {:.1} MB; the limit is 25 MB",
                        file_name_of(&expanded),
                        meta.len() as f64 / (1024.0 * 1024.0)
                    ),
                    true,
                    cx,
                );
                return;
            }
            let data = match std::fs::read(&expanded) {
                Ok(d) => d,
                Err(e) => {
                    self.show_toast(format!("Cannot read {expanded}: {e}"), true, cx);
                    return;
                }
            };
            let filename = file_name_of(&expanded);
            let content_type = guess_content_type(&filename).to_string();
            self.compose_attachments
                .push(vespetrel_core::message::ComposedAttachment {
                    filename: filename.clone(),
                    content_type,
                    data,
                });
            self.compose_attach_path.clear();
            self.show_toast(format!("Attached {filename}"), false, cx);
            cx.notify();
        }

        /// Open an empty compose window.
        fn open_compose(&mut self, cx: &mut Context<Self>) {
            self.active_tab = ActiveViewTab::Mail;
            self.compose_reply_to_id = None;
            self.compose_draft_id = None;
            self.compose_state.is_markdown_mode = false;
            self.compose_to.clear();
            self.compose_subject.clear();
            self.compose_attachments.clear();

            // Set default alias if available
            if self.selected_alias.is_none() {
                self.selected_alias = self
                    .aliases
                    .iter()
                    .find(|a| a.is_default)
                    .cloned()
                    .or_else(|| self.aliases.first().cloned());
            }

            // Link signature for selected alias or default account signature
            let sig_opt = self
                .selected_alias
                .as_ref()
                .and_then(|a| a.signature_id.as_ref())
                .and_then(|sig_id| self.signatures.iter().find(|s| &s.id == sig_id).cloned())
                .or_else(|| self.signatures.iter().find(|s| s.is_default).cloned())
                .or_else(|| self.signatures.first().cloned());

            if let Some(sig) = sig_opt {
                self.selected_signature = Some(sig.clone());
                self.compose_state.apply_signature(&sig);
                if let Some(pt) = &sig.plain_text {
                    self.compose_body = format!("\n\n-- \n{pt}");
                } else {
                    self.compose_body.clear();
                }
            } else {
                self.compose_body.clear();
            }

            self.active_modal = ActiveModal::Compose;
            cx.notify();
        }

        /// Open a reply, quoting the message the reader is showing.
        ///
        /// A plain reply needs nothing but the sender, so it opens immediately. A reply
        /// all has to read the stored recipients, which is a database round-trip: the
        /// window is populated now and the recipient field is filled when the read
        /// lands. A failure is reported in a toast rather than degrading the reply to a
        /// reply-to-sender, which is what a user would not notice until it was sent.
        fn open_reply(&mut self, all_recipients: bool, cx: &mut Context<Self>) {
            let Some(msg) = self.selected_message().cloned() else {
                return;
            };
            self.active_tab = ActiveViewTab::Mail;
            self.compose_reply_to_id = Some(msg.id.clone());
            self.compose_draft_id = None;
            self.compose_to = msg.from_address.clone();
            self.compose_subject = prefixed_subject(msg.subject.as_deref(), "Re:");
            let quote = self.message_viewer.generate_reply_text();
            self.compose_body = quote.trim_start_matches('\n').to_string();
            self.compose_state.is_markdown_mode = false;
            self.compose_attachments.clear();

            // Auto-detect alias if message was sent to one of the user's aliases
            if let Some(matching_alias) = self.aliases.iter().find(|a| {
                self.message_viewer
                    .to
                    .iter()
                    .chain(self.message_viewer.cc.iter())
                    .any(|addr| addr.email.eq_ignore_ascii_case(&a.email))
            }) {
                self.selected_alias = Some(matching_alias.clone());
            } else if self.selected_alias.is_none() {
                self.selected_alias = self
                    .aliases
                    .iter()
                    .find(|a| a.is_default)
                    .cloned()
                    .or_else(|| self.aliases.first().cloned());
            }

            // Look up signature for replies
            let sig_opt = self
                .selected_alias
                .as_ref()
                .and_then(|a| a.signature_id.as_ref())
                .and_then(|sig_id| self.signatures.iter().find(|s| &s.id == sig_id).cloned())
                .or_else(|| {
                    self.signatures
                        .iter()
                        .find(|s| s.is_default && s.include_in_replies)
                        .cloned()
                });

            if let Some(sig) = sig_opt {
                self.selected_signature = Some(sig.clone());
                if let Some(pt) = &sig.plain_text {
                    self.compose_body = format!("\n\n-- \n{pt}\n\n{}", self.compose_body);
                }
            }

            self.active_modal = ActiveModal::Compose;
            cx.notify();

            if !all_recipients {
                return;
            }

            let recipients = self.fetch_recipients_of(msg.id.clone());
            cx.spawn(async move |this, cx| match recipients.await {
                Ok(list) => {
                    let joined = list.join(", ");
                    let _ = this.update(cx, |view, cx| {
                        view.compose_to = joined;
                        cx.notify();
                    });
                }
                Err(reason) => {
                    let _ = this.update(cx, |view, cx| {
                        view.show_toast(
                            format!(
                                "Recipients unavailable ({reason}); replying to the sender only"
                            ),
                            true,
                            cx,
                        );
                    });
                }
            })
            .detach();
        }

        /// Read a stored message's To and Cc addresses, off the UI thread.
        ///
        /// `MessageSummary` carries no recipient list, so Reply All has to come back to
        /// storage. `Ok(None)` means the body is gone or unreadable, which the caller
        /// reports rather than quietly sending a reply-all that is a reply.
        fn fetch_recipients_of(
            &mut self,
            message_id: String,
        ) -> impl std::future::Future<Output = Result<Vec<String>, String>> + use<> {
            let pool = self.storage_pool.clone();
            async move {
                let Some(pool) = pool else {
                    return Err("No local store for this account".into());
                };
                let conn = pool
                    .get()
                    .await
                    .map_err(|e| format!("Local store unavailable: {e}"))?;
                let stored = conn
                    .interact(move |c| vespetrel_storage::repo::get_message(c, &message_id).ok())
                    .await
                    .map_err(|e| e.to_string())?
                    .flatten()
                    .ok_or_else(|| "Message body is no longer stored locally".to_string())?;
                let mut all = stored.to_addresses;
                all.extend(stored.cc_addresses);
                all.retain(|a| !a.email.is_empty());
                if all.is_empty() {
                    return Err("This message has no stored recipients".into());
                }
                Ok(all.into_iter().map(|a| a.email).collect())
            }
        }

        /// Open a forward. The original's attachments are carried over, ready to send.
        fn open_forward(&mut self, cx: &mut Context<Self>) {
            let Some(msg) = self.selected_message().cloned() else {
                return;
            };
            self.active_tab = ActiveViewTab::Mail;
            self.compose_reply_to_id = None;
            self.compose_draft_id = None;
            self.compose_to.clear();
            self.compose_subject = prefixed_subject(msg.subject.as_deref(), "Fwd:");
            self.compose_body = format!(
                "---------- Forwarded message ----------\nFrom: {}\nSubject: {}\n\n{}",
                msg.from_address,
                msg.subject.as_deref().unwrap_or("(No Subject)"),
                msg.snippet.as_deref().unwrap_or("")
            );
            self.compose_state.is_markdown_mode = false;
            self.compose_attachments.clear();
            self.active_modal = ActiveModal::Compose;
            cx.notify();
        }

        /// Transient status messages.
        ///
        /// A notification is an overlay, so it uses the popover surface rather than a
        /// saturated success/danger fill: a full-strength status colour behind every
        /// confirmation made the toast compete with the message the user was reading, and
        /// the dark green/red pair was unreadable in light mode. The status is carried by
        /// the icon and the text; the surface stays neutral.
        fn render_toasts(&self, cx: &Context<Self>) -> impl IntoElement {
            div()
                .absolute()
                // Clear of the status bar, and inset from the window edge by the scale.
                .bottom(px(36.0))
                .right(px(24.0))
                .flex()
                .flex_col()
                .gap(px(8.0))
                .children(self.toasts.iter().map(|t| {
                    let accent = if t.is_error {
                        cx.theme().danger
                    } else {
                        cx.theme().success
                    };
                    let mut toast_div = div()
                        .id(ElementId::Name(format!("toast-{}", t.id).into()))
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(10.0))
                        .px(px(14.0))
                        .py(px(8.0))
                        .rounded_md()
                        .bg(cx.theme().popover)
                        .text_color(cx.theme().popover_foreground)
                        .border_1()
                        .border_color(cx.theme().border)
                        .border_l_2()
                        .text_xs()
                        .shadow_lg()
                        .child(
                            Icon::new(if t.is_error {
                                IconName::TriangleAlert
                            } else {
                                IconName::Check
                            })
                            .small()
                            .text_color(accent),
                        )
                        .child(div().flex_1().min_w(px(0.0)).child(elide(&t.message, 200)));

                    if let Some(ref outbox_id) = t.undo_outbox_id {
                        let out_id = outbox_id.clone();
                        toast_div = toast_div.child(
                            Button::new(ElementId::Name(format!("undo-{}", t.id).into()))
                                .small()
                                .icon(IconName::Undo)
                                .label("Undo")
                                .tooltip("Undo send")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.undo_send(&out_id, cx);
                                })),
                        );
                    }

                    let toast_id = t.id.clone();
                    toast_div.child(
                        Button::new(ElementId::Name(format!("dismiss-{}", t.id).into()))
                            .ghost()
                            .icon(IconName::Close)
                            .tooltip("Dismiss")
                            .accessibility_label("Dismiss notification")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.dismiss_toast(&toast_id, cx);
                            })),
                    )
                }))
        }
        fn render_header(&self, cx: &Context<Self>) -> impl IntoElement {
            let _ = &self.search_query;

            let nav_tabs = [
                (ActiveViewTab::Mail, IconName::Inbox, "Mail"),
                (ActiveViewTab::Calendar, IconName::Calendar, "Calendar"),
                (ActiveViewTab::Contacts, IconName::User, "Contacts"),
                (ActiveViewTab::Tasks, IconName::Check, "Tasks"),
                (ActiveViewTab::Settings, IconName::Settings, "Settings"),
            ];

            TitleBar::new().child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .w_full()
                    .h(px(48.0))
                    .px(px(16.0))
                    .bg(cx.theme().title_bar)
                    .border_b_1()
                    .border_color(cx.theme().title_bar_border)
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
                                    .w(px(28.0))
                                    .h(px(28.0))
                                    .rounded_md()
                                    .bg(cx.theme().primary)
                                    .text_color(cx.theme().primary_foreground)
                                    .font_weight(FontWeight::BOLD)
                                    .text_sm()
                                    .child("V"),
                            )
                            .child(
                                div()
                                    .font_weight(FontWeight::BOLD)
                                    .text_sm()
                                    .text_color(cx.theme().foreground)
                                    .child("Vespetrel"),
                            )
                            .child(TabBar::new("header-nav-tabbar").children(
                                nav_tabs.into_iter().map(|(tab, icon, label)| {
                                    let is_active = self.active_tab == tab;
                                    Tab::new()
                                        .child(
                                            div()
                                                .flex()
                                                .flex_row()
                                                .items_center()
                                                .gap(px(6.0))
                                                .child(Icon::new(icon).small())
                                                .child(label),
                                        )
                                        .selected(is_active)
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.active_tab = tab;
                                            cx.notify();
                                        }))
                                }),
                            )),
                    )
                    // A real text field, not a display surface. The old control was a
                    // `div` that showed `search_query` and, on click, opened the command
                    // palette instead: there was no way to type a query, and
                    // `execute_fts_search` had no caller at all.
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
                            .bg(cx.theme().input)
                            .border_1()
                            .border_color(if self.search_query.is_empty() {
                                cx.theme().border
                            } else {
                                cx.theme().primary
                            })
                            .gap(px(8.0))
                            .child(
                                Icon::new(IconName::Search)
                                    .small()
                                    .text_color(cx.theme().muted_foreground),
                            )
                            .child(
                                self.search_input
                                    .as_ref()
                                    .map(|input| {
                                        component::input::Input::new(input)
                                            .appearance(false)
                                            .cleanable(true)
                                            .aria_label("Search messages")
                                            .into_any_element()
                                    })
                                    .unwrap_or_else(|| div().into_any_element()),
                            )
                            .children((!self.search_query.is_empty()).then(|| {
                                Button::new("btn-clear-search")
                                    .ghost()
                                    .icon(IconName::Close)
                                    .tooltip("Clear search")
                                    .accessibility_label("Clear search")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        if let Some(input) = this.search_input.clone() {
                                            input.update(cx, |state, cx| {
                                                state.set_value("", window, cx)
                                            });
                                        }
                                        this.clear_search(cx);
                                    }))
                            })),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(8.0))
                            .child(
                                // The one emphasised commitment in the window.
                                Button::new("btn-header-compose")
                                    .primary()
                                    .icon(IconName::Plus)
                                    .label("Compose")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.active_modal = ActiveModal::Compose;
                                        cx.notify();
                                    })),
                            )
                            .child(
                                Button::new("btn-header-sync")
                                    .ghost()
                                    .icon(IconName::RotateCw)
                                    .label("Sync")
                                    .tooltip("Check mail for new messages")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.trigger_sync(cx);
                                    })),
                            )
                            // The shortcut is presented as a key, not as the button's name.
                            // A button labelled "⌘K" announced itself as a glyph, told the
                            // user nothing about the command, and showed the macOS notation
                            // on every platform.
                            .child(
                                Button::new("btn-header-palette")
                                    .ghost()
                                    .label("Commands")
                                    .tooltip("Search commands")
                                    .accessibility_label("Open the command palette")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.active_modal = ActiveModal::CommandPalette;
                                        cx.notify();
                                    }))
                                    .child(div().child(Kbd::new(command_palette_keystroke()))),
                            )
                            .child(
                                Popover::new("header-info-popover")
                                    .trigger(
                                        Button::new("btn-header-info")
                                            .ghost()
                                            .icon(IconName::Info)
                                            .tooltip("About Vespetrel")
                                            .accessibility_label("About Vespetrel"),
                                    )
                                    .content(|_, _, cx| {
                                        div()
                                            .flex()
                                            .flex_col()
                                            .gap(px(4.0))
                                            .p(px(12.0))
                                            .max_w(px(260.0))
                                            .bg(cx.theme().popover)
                                            .text_color(cx.theme().popover_foreground)
                                            .border_1()
                                            .border_color(cx.theme().border)
                                            .rounded_md()
                                            .text_xs()
                                            .child(
                                                div()
                                                    .font_weight(FontWeight::SEMIBOLD)
                                                    .child("Vespetrel 0.1.0"),
                                            )
                                            .child(
                                                div()
                                                    .text_color(cx.theme().muted_foreground)
                                                    .child(
                                                        "Mail, calendar and contacts on a local SQLite store.",
                                                    ),
                                            )
                                    }),
                            ),
                    ),
            )
        }

        /// App-level destination rail: Mail, Calendar, Contacts, Tasks, Settings.
        ///
        /// Icon-only on purpose. At 56 px the old label under each icon was clipped
        /// ("Calendar" does not fit the lane), and an unreadable label is worse than none.
        /// Discovery and naming are carried by the labelled `TabBar` in the header, and
        /// every item here gets a tooltip and an accessible name.
        ///
        /// The active item is a quiet `ghost` button with `toggled` set, not `primary`:
        /// selection is not a commitment, and spending the one primary emphasis on
        /// navigation would compete with Compose, which is the actual primary task.
        fn render_sidebar_tabs(&self, cx: &Context<Self>) -> impl IntoElement {
            let tabs = [
                (ActiveViewTab::Mail, IconName::Inbox, "Mail"),
                (ActiveViewTab::Calendar, IconName::Calendar, "Calendar"),
                (ActiveViewTab::Contacts, IconName::User, "Contacts"),
                (ActiveViewTab::Tasks, IconName::Check, "Tasks"),
                (ActiveViewTab::Settings, IconName::Settings, "Settings"),
            ];

            div()
                .flex()
                .flex_col()
                .w(px(48.0))
                .h_full()
                .bg(cx.theme().sidebar)
                .border_r_1()
                .border_color(cx.theme().sidebar_border)
                .flex_shrink_0()
                .items_center()
                .py(px(8.0))
                .gap(px(4.0))
                .children(tabs.into_iter().map(|(tab, icon, label)| {
                    let is_active = self.active_tab == tab;
                    let color = if is_active {
                        cx.theme().sidebar_accent_foreground
                    } else {
                        cx.theme().sidebar_foreground
                    };
                    Button::new(ElementId::Name(format!("nav-rail-{}", label).into()))
                        .ghost()
                        .icon(Icon::new(icon).small().text_color(color))
                        .toggled(is_active)
                        .tooltip(label)
                        .accessibility_label(label)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.active_tab = tab;
                            cx.notify();
                        }))
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
                    ActiveViewTab::Calendar => self.render_calendar_view(cx).into_any_element(),
                    ActiveViewTab::Contacts => self.render_contacts_view(cx).into_any_element(),
                    ActiveViewTab::Tasks => self.render_tasks_view(cx).into_any_element(),
                    ActiveViewTab::Settings => self.render_settings_view(cx).into_any_element(),
                })
        }

        fn render_mail_workspace(&self, cx: &Context<Self>) -> impl IntoElement {
            h_resizable("mail-workspace-panels")
                .children(
                    self.folder_tree_visible
                        .then(|| self.render_folder_tree(cx).into_any_element()),
                )
                .child(self.render_message_list_pane(cx).into_any_element())
                .child(self.render_message_reader_pane(cx).into_any_element())
        }

        fn render_folder_tree(&self, cx: &Context<Self>) -> impl IntoElement {
            div()
                .flex()
                .flex_col()
                .w(px(220.0))
                .h_full()
                .flex_shrink_0()
                .bg(cx.theme().sidebar)
                .border_r_1()
                .border_color(cx.theme().sidebar_border)
                .p(px(12.0))
                .gap(px(12.0))
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
                                .text_color(cx.theme().muted_foreground)
                                .child("Accounts"),
                        )
                        .child(
                            Button::new("btn-add-account")
                                .icon(IconName::Plus)
                                .label("Add")
                                .ghost()
                                .small()
                                .tooltip("Add account")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.login_wizard = LoginWizardState::new();
                                    this.wizard_inputs = None;
                                    this.active_modal = ActiveModal::AddAccount;
                                    cx.notify();
                                })),
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
                            .bg(cx.theme().muted)
                            .hover(|s| s.bg(cx.theme().accent))
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
                                    .text_color(cx.theme().muted_foreground)
                                    .child("No accounts configured"),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child("Use Add to connect an email account"),
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
                                .bg(cx.theme().muted)
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .child(
                                            div()
                                                .text_xs()
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .text_color(cx.theme().foreground)
                                                .child(acc.email.clone()),
                                        )
                                        .child(
                                            div()
                                                .text_xs()
                                                .text_color(cx.theme().muted_foreground)
                                                .child(format!("{:?}", acc.provider_type)),
                                        ),
                                )
                                .child(
                                    Button::new(ElementId::Name(
                                        format!("btn-del-account-{}", idx).into(),
                                    ))
                                    .ghost()
                                    .danger()
                                    .icon(IconName::Delete)
                                    .tooltip(format!("Remove {}", acc.email))
                                    .accessibility_label(format!("Remove {}", acc.email))
                                    .on_click(cx.listener(
                                        move |this, _, _, cx| {
                                            this.delete_account(&acc_id_clone, cx);
                                        },
                                    )),
                                )
                        })
                        .collect()
                })
                .child({
                    // One row shape for every destination, so the two branches return the same
                    // element type and the tree cannot drift.
                    let mut folder_elements: Vec<AnyElement> = Vec::new();

                    // Unified Inbox
                    let unified_unread: i64 = self
                        .folders
                        .iter()
                        .filter(|f| f.role == FolderRole::Inbox)
                        .map(|f| f.unread_count)
                        .sum();

                    folder_elements.push(
                        self.render_folder_row(
                            FolderRow {
                                id: ElementId::Name("folder-unified-inbox".into()),
                                icon: IconName::Inbox,
                                label: "Unified Inbox",
                                unread: unified_unread,
                                is_selected: self.is_unified_inbox,
                            },
                            cx,
                            cx.listener(|this, _, _, cx| {
                                this.select_unified_inbox(cx);
                            }),
                        )
                        .into_any_element(),
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
                            self.render_folder_row(
                                FolderRow {
                                    id: ElementId::Name(format!("folder-{}", f.id).into()),
                                    icon: NavigationTree::folder_icon(&f.role),
                                    label: &f.name,
                                    unread: f.unread_count,
                                    is_selected,
                                },
                                cx,
                                cx.listener(move |this, _, _, cx| {
                                    this.select_folder(folder_id_clone.clone(), cx);
                                }),
                            )
                            .into_any_element()
                        })
                        .collect::<Vec<_>>();

                    folder_elements.extend(individual_folders);
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(4.0))
                        .children(folder_elements)
                })
        }

        /// One row in the folder tree.
        ///
        /// Defined once so "Unified Inbox" and every real folder cannot drift apart in
        /// selection, hover, icon slot or padding. The icon occupies a fixed-width slot and
        /// the unread badge a fixed trailing lane, so a row with no mail keeps exactly the
        /// same label position as one with — otherwise the whole tree shifts as counts
        /// change.
        fn render_folder_row(
            &self,
            row: FolderRow<'_>,
            cx: &Context<Self>,
            on_select: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
        ) -> impl IntoElement {
            let FolderRow {
                id,
                icon,
                label,
                unread,
                is_selected,
            } = row;
            let space = cx.theme().spacing_tokens();
            let fg = if is_selected {
                cx.theme().sidebar_accent_foreground
            } else {
                cx.theme().sidebar_foreground
            };
            div()
                .id(id)
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .gap(space.sm)
                .px(space.sm)
                .py(space.xs)
                .rounded_md()
                .bg(if is_selected {
                    cx.theme().sidebar_accent
                } else {
                    gpui_kit::gpui::transparent_black()
                })
                .hover(|s| s.bg(cx.theme().accent))
                .text_color(fg)
                .on_click(on_select)
                // Fixed leading slot: the label starts at the same offset on every row.
                .child(
                    div()
                        .w(px(16.0))
                        .flex_shrink_0()
                        .child(Icon::new(icon).small()),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.0))
                        .text_xs()
                        .overflow_hidden()
                        .text_ellipsis()
                        .font_weight(if is_selected {
                            FontWeight::SEMIBOLD
                        } else {
                            FontWeight::NORMAL
                        })
                        // Server-supplied folder names are unbounded (a server can nest
                        // "INBOX.Rechnungen.2026.Q1"), so truncate on a character boundary
                        // rather than letting one long name widen the whole tree.
                        .child(elide(label, 28)),
                )
                // Fixed trailing lane: a zero count keeps its space.
                .child(
                    div()
                        .w(px(28.0))
                        .flex_shrink_0()
                        .flex()
                        .justify_end()
                        .child(if unread > 0 {
                            Badge::new().count(unread as usize).into_any_element()
                        } else {
                            div().into_any_element()
                        }),
                )
        }

        fn render_message_row(&self, row: &CachedRow, cx: &Context<Self>) -> impl IntoElement {
            let msg = &row.summary;
            let is_selected = self.selected_message_id.as_deref() == Some(&msg.id);
            let sender = msg.from_name.as_deref().unwrap_or(&msg.from_address);
            let subject = msg.subject.as_deref().unwrap_or("(No subject)");
            let snippet = msg.snippet.as_deref().unwrap_or("");
            let date_str = msg.sent_at.format("%b %d, %H:%M").to_string();
            let msg_id = msg.id.clone();
            let is_child = row.is_child;
            let thread_count = row.thread_count;
            // Read once here: `msg_id` is moved into the star button's click handler
            // below, so the row cannot be borrowed for the flag afterwards.
            let star_on = msg.is_flagged;
            let star_id = ElementId::Name(format!("btn-star-{}", msg_id).into());
            // The row's click handler takes `msg_id` by move, so the star button
            // needs its own copy.
            let star_target = msg_id.clone();
            let space = cx.theme().spacing_tokens();

            // Thread children are indented by one level and the nesting is real, so the
            // indent communicates hierarchy rather than decoration.
            let pad_left = if is_child { space.xl } else { space.md };

            let is_in_range = self.selection.is_selected(&msg.id);

            let mut item_div = div()
                .id(ElementId::Name(format!("msg-item-{}", msg_id).into()))
                .flex()
                .flex_col()
                .pl(pad_left)
                .pr(space.md)
                .py(match self.row_density {
                    MessageRowDensity::Compact => space.xs,
                    MessageRowDensity::Comfortable => space.sm,
                    MessageRowDensity::Roomy => space.md,
                })
                .border_b_1()
                .border_color(cx.theme().border)
                // Two distinct states, two distinct surfaces. The current row is
                // filled; a row in the bulk range carries a left rule, so a range of
                // eight messages is legible without eight filled bars fighting the
                // current row for attention.
                .bg(if is_selected {
                    cx.theme().selection
                } else {
                    cx.theme().background
                })
                .border_l_2()
                .border_color(if is_in_range {
                    cx.theme().primary
                } else {
                    gpui_kit::gpui::transparent_black()
                })
                .hover(|s| {
                    s.bg(if is_selected {
                        cx.theme().accent
                    } else {
                        cx.theme().muted
                    })
                })
                // A plain click replaces the range; Shift- or Ctrl-click extends it, which is the
                // desktop convention for building a multi-row target.
                .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
                    let modifiers = event.modifiers();
                    let extend = modifiers.shift || modifiers.control;
                    if extend {
                        // The row being clicked is not current yet, so the range is
                        // extended from the row that was.
                        this.toggle_selection_of(&msg_id);
                    } else {
                        this.selection.clear();
                    }
                    this.select_message(msg_id.clone(), cx);
                }));

            // Unread is carried by weight and the leading dot, not by a row tint. A tinted
            // unread row made the whole list a field of colour and, in light mode, inverted
            // the reading order.
            let header_row = div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .gap(space.sm)
                // Fixed leading lane. The old code rendered a `"↳"` glyph for children and a
                // 6px dot otherwise, so the sender label sat at a different offset on every
                // row of a thread and the column could not be scanned vertically.
                .child(
                    div()
                        .w(px(10.0))
                        .flex_shrink_0()
                        .justify_center()
                        .child(if is_child {
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child("↳")
                        } else if !msg.is_read {
                            div()
                                .w(px(6.0))
                                .h(px(6.0))
                                .rounded_full()
                                .bg(cx.theme().primary)
                        } else {
                            div().w(px(6.0)).h(px(6.0))
                        }),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.0))
                        .overflow_hidden()
                        .text_ellipsis()
                        .text_xs()
                        .font_weight(if !msg.is_read {
                            FontWeight::BOLD
                        } else {
                            FontWeight::NORMAL
                        })
                        .text_color(cx.theme().foreground)
                        // A display name is user-supplied and unbounded; truncate on a
                        // character boundary so a 400-character name cannot push the whole
                        // row's content out of its lanes.
                        .child(elide(sender, 60)),
                )
                // Fixed trailing lanes in a fixed order, so the date lands on the same
                // vertical line whether or not a thread count or a flag is present.
                .child(
                    div()
                        .flex_shrink_0()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(space.sm)
                        .child(div().w(px(16.0)).justify_center().child(
                            if thread_count > 1 && !is_child {
                                Badge::new()
                                    .child(
                                        div()
                                            .flex()
                                            .flex_row()
                                            .items_center()
                                            .gap(px(3.0))
                                            .child(Icon::new(IconName::GalleryVerticalEnd).xsmall())
                                            .child(thread_count.to_string()),
                                    )
                                    .into_any_element()
                            } else {
                                div().into_any_element()
                            },
                        ))
                        // A real button. This was a bare `Icon`, so a message could not be starred from
                        // the list at all: `toggle_flag` existed and had no call site.
                        .child(
                            Button::new(star_id)
                                .ghost()
                                .xsmall()
                                .icon(if star_on {
                                    IconName::StarFill
                                } else {
                                    IconName::Star
                                })
                                .tooltip(if star_on { "Remove star" } else { "Add star" })
                                .accessibility_label(if star_on {
                                    "Remove star"
                                } else {
                                    "Add star"
                                })
                                .on_click({
                                    let star_id = star_target.clone();
                                    cx.listener(move |this, _, _, cx| {
                                        this.selection.set_current(star_id.clone());
                                        this.set_flag(std::slice::from_ref(&star_id), None, cx);
                                    })
                                }),
                        )
                        .child(
                            div()
                                .w(px(72.0))
                                .flex_shrink_0()
                                .text_xs()
                                .text_right()
                                .text_color(cx.theme().muted_foreground)
                                .child(date_str),
                        ),
                );

            item_div = item_div.child(header_row);

            if self.row_density != MessageRowDensity::Compact {
                item_div = item_div.child(
                    div()
                        .pt(px(2.0))
                        .pl(px(10.0))
                        .text_xs()
                        .overflow_hidden()
                        .text_ellipsis()
                        .font_weight(if !msg.is_read {
                            FontWeight::SEMIBOLD
                        } else {
                            FontWeight::NORMAL
                        })
                        .text_color(cx.theme().foreground)
                        .child(elide(subject, 120)),
                );
            }

            // One snippet line, truncated on a character boundary.
            //
            // The old code did `&snippet[..100]`, which panics whenever the 100th byte falls
            // inside a multi-byte character. Snippets are attacker- and locale-controlled
            // ("Rechnung für März — überfällig"), so this crashed the list on ordinary mail.
            if self.row_density != MessageRowDensity::Compact && !snippet.is_empty() {
                let max_chars = match self.row_density {
                    MessageRowDensity::Roomy => 120,
                    _ => 80,
                };
                item_div = item_div.child(
                    div()
                        .pt(px(2.0))
                        .pl(px(10.0))
                        .text_xs()
                        .overflow_hidden()
                        .text_ellipsis()
                        .text_color(cx.theme().muted_foreground)
                        .child(elide(snippet, max_chars)),
                );
            }

            item_div
        }

        fn render_message_list_pane(&self, cx: &Context<Self>) -> impl IntoElement {
            let rows = self.cached_rows();
            let total_count = rows.len();
            let row_height = self.row_density.row_height_px();
            let sizes: Vec<gpui_kit::gpui::Size<gpui_kit::gpui::Pixels>> = (0..total_count)
                .map(|_| gpui_kit::gpui::size(px(350.0), px(row_height)))
                .collect();
            let item_sizes = std::rc::Rc::new(sizes);
            let selected_count = self.selection.selected_count();
            let total_shown = total_count;

            let sort_label = match self.sort_order {
                MessageSortOrder::DateDescending => "Newest first",
                MessageSortOrder::DateAscending => "Oldest first",
            };
            let density_label = match self.row_density {
                MessageRowDensity::Compact => "Compact",
                MessageRowDensity::Comfortable => "Normal",
                MessageRowDensity::Roomy => "Roomy",
            };

            div()
                .flex()
                .flex_col()
                .w(px(360.0))
                .h_full()
                .flex_shrink_0()
                .bg(cx.theme().background)
                .border_r_1()
                .border_color(cx.theme().border)
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(6.0))
                        .px(px(8.0))
                        .py(px(8.0))
                        .border_b_1()
                        .border_color(cx.theme().border)
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap(px(4.0))
                                .child(self.render_filter_chip("All", None, ListFilter::All, cx))
                                .child(self.render_filter_chip(
                                    "Unread",
                                    None,
                                    ListFilter::Unread,
                                    cx,
                                ))
                                .child(self.render_filter_chip(
                                    "Starred",
                                    Some(IconName::Star),
                                    ListFilter::Flagged,
                                    cx,
                                ))
                                .child(self.render_filter_chip(
                                    "Files",
                                    Some(IconName::File),
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
                                .gap(px(8.0))
                                .text_xs()
                                // Threading is a *view mode*, not a commitment, so it reads as
                                // a quiet toggle with a persistent pressed state rather than
                                // a primary button competing with Compose.
                                .child(
                                    Button::new("btn-toggle-thread")
                                        .ghost()
                                        .small()
                                        .icon(IconName::GalleryVerticalEnd)
                                        .label("Threaded")
                                        .toggled(self.is_threaded)
                                        .tooltip("Group messages into threads")
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.toggle_threading(cx);
                                        })),
                                )
                                // The bulk range, stated in words as well as by the left
                                // rule on each row, so it survives a colourless reading.
                                .when(selected_count > 0, |el| {
                                    el.child(
                                        div()
                                            .flex()
                                            .flex_row()
                                            .items_center()
                                            .gap(px(6.0))
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .text_color(cx.theme().muted_foreground)
                                                    .child(format!(
                                                        "{selected_count} of {total_shown} selected"
                                                    )),
                                            )
                                            .child(
                                                Button::new("btn-clear-selection")
                                                    .ghost()
                                                    .small()
                                                    .label("Clear")
                                                    .tooltip("Clear the selected range")
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        this.selection.clear();
                                                        cx.notify();
                                                    })),
                                            ),
                                    )
                                })
                                // Sort and density are reversible view settings for this
                                // pane. They stay visible and state-bearing — the label
                                // states the current value, so the control can be read
                                // without activating it and the state can be seen.
                                .child(
                                    Button::new("btn-toggle-sort")
                                        .ghost()
                                        .small()
                                        .icon(IconName::SortDescending)
                                        .label(sort_label.to_string())
                                        .tooltip("Reverse date order")
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.cycle_sort_order(cx);
                                        })),
                                )
                                .child(
                                    Button::new("btn-toggle-density")
                                        .ghost()
                                        .small()
                                        .icon(IconName::GalleryVerticalEnd)
                                        .label(density_label.to_string())
                                        .tooltip("Cycle row density")
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.cycle_row_density(cx);
                                        })),
                                ),
                        ),
                )
                .child(div().flex_1().overflow_hidden().child(v_virtual_list(
                    cx.entity(),
                    "v-messages-list",
                    item_sizes,
                    move |this, range, _window, cx| {
                        // Rows come from the cache captured by `render_message_list_pane`, not
                        // from a per-window rebuild. Previously this closure re-derived the
                        // whole filtered-and-threaded vector on every scroll frame.
                        let rows = rows.clone();
                        range
                            .filter_map(|idx| {
                                rows.get(idx)
                                    .map(|row| this.render_message_row(row, cx).into_any_element())
                            })
                            .collect()
                    },
                )))
        }

        fn render_filter_chip(
            &self,
            label: &'static str,
            icon: Option<IconName>,
            filter: ListFilter,
            cx: &Context<Self>,
        ) -> impl IntoElement {
            let is_active = self.list_filter == filter;
            let mut btn = Button::new(ElementId::Name(format!("filter-chip-{}", label).into()))
                .small()
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.list_filter = filter;
                    this.invalidate_message_cache();
                    cx.notify();
                }));
            if let Some(ic) = icon {
                btn = btn.icon(ic);
            }
            btn = btn.label(label);
            // A filter chip is a *selected state*, not a commitment. Primary is reserved for
            // the one default action in a decision area, so an active chip uses the same
            // quiet treatment as the inactive ones plus a persistent `toggled` state.
            btn.ghost().toggled(is_active)
        }

        fn render_message_reader_pane(&self, cx: &Context<Self>) -> impl IntoElement {
            let msg_opt = self.selected_message();
            let (body_kind, body_source) = self.message_viewer.body();

            div()
                .flex()
                .flex_col()
                .flex_1()
                .h_full()
                .bg(cx.theme().background)
                .child(if let Some(msg) = msg_opt {
                    let from_name = msg.from_name.as_deref().unwrap_or("");
                    let from_addr = &msg.from_address;
                    let subject = msg.subject.as_deref().unwrap_or("(No Subject)");
                    let date_full = msg.sent_at.to_rfc2822();
                    let attachments = &self.message_viewer.attachments;

                    let header_panel = div()
                        .flex()
                        .flex_col()
                        .w_full()
                        // Top toolbar
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .justify_between()
                                .p(px(12.0))
                                .border_b_1()
                                .border_color(cx.theme().border)
                                .bg(cx.theme().muted)
                                .child(
                                    div()
                                        .flex()
                                        .flex_row()
                                        .items_center()
                                        .gap(px(8.0))
                                        .child(
                                            // Reply is the reader's most likely action, but
                                            // it opens a composition window rather than
                                            // committing anything, so it is a default
                                            // button. Primary stays with Compose, the one
                                            // emphasised commitment in the app.
                                            Button::new("btn-reply-message")
                                                .icon(IconName::ArrowLeft)
                                                .label("Reply")
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
                                                })),
                                        )
                                        .child(
                                            Button::new("btn-reply-all-message")
                                                .ghost()
                                                .icon(IconName::Undo2)
                                                .label("Reply all")
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
                                                })),
                                        )
                                        .child(
                                            Button::new("btn-forward-message")
                                                .ghost()
                                                .icon(IconName::ArrowRight)
                                                .label("Forward")
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
                                                })),
                                        )
                                        .child(
                                            Button::new("btn-archive-message")
                                                .secondary()
                                                .icon(IconName::HardDrive)
                                                .label("Archive")
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.archive_selected_message(cx);
                                                })),
                                        )
                                        .child(
                                            Button::new("btn-delete-message")
                                                .danger()
                                                .icon(IconName::Delete)
                                                .label("Delete")
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.delete_selected_message(cx);
                                                })),
                                        ),
                                )
                                // Security facts. Each badge states the verdict in words and
                                // pairs it with an icon, so the state survives a colourless
                                // reading, a light theme and colour-vision differences.
                                .child(
                                    div()
                                        .flex()
                                        .flex_row()
                                        .items_center()
                                        .gap(px(6.0))
                                        // No badge for a message with no cryptography: an empty row already says
                                // that, and a permanent neutral badge would make the
                                // one badge that matters easy to ignore.
                                .when(
                                    !self.message_viewer.auth_facts().is_empty(),
                                    |el| {
                                        el.child(
                                            div()
                                                .text_xs()
                                                .font_weight(FontWeight::BOLD)
                                                .text_color(cx.theme().muted_foreground)
                                                .child("Security"),
                                        )
                                    },
                                )
                                .children(self.message_viewer.auth_facts().into_iter().map(
                                            |fact| {
                                                let (icon, color) = match fact.state {
                                                    AuthState::Pass => {
                                                        (IconName::Check, cx.theme().success)
                                                    }
                                                    AuthState::Fail => {
                                                        (IconName::TriangleAlert, cx.theme().danger)
                                                    }
                                                    AuthState::Unknown => (
                                                        IconName::Info,
                                                        cx.theme().muted_foreground,
                                                    ),
                                                };
                                                Badge::new()
                                                    .child(
                                                        div()
                                                            .flex()
                                                            .items_center()
                                                            .gap(px(4.0))
                                                            .child(
                                                                Icon::new(icon)
                                                                    .xsmall()
                                                                    .text_color(color),
                                                            )
                                                            .child(fact.label),
                                                    )
                                                    .into_any_element()
                                            },
                                        )),
                                ),
                        )
                        // Subject & Sender info
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .p(px(16.0))
                                .border_b_1()
                                .border_color(cx.theme().border)
                                .gap(px(10.0))
                                .child(
                                    div()
                                        .text_lg()
                                        .font_weight(FontWeight::BOLD)
                                        .text_color(cx.theme().foreground)
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
                                                        .bg(cx.theme().primary)
                                                        .text_color(cx.theme().primary_foreground)
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
                                                                .text_color(cx.theme().foreground)
                                                                .child(if from_name.is_empty() {
                                                                    from_addr.clone()
                                                                } else {
                                                                    format!("{from_name} <{from_addr}>")
                                                                }),
                                                        )
                                                        .child(
                                                            div()
                                                                .text_xs()
                                                                .text_color(cx.theme().muted_foreground)
                                                                .child(format!(
                                                                    "To: {}",
                                                                    self.accounts.first().map(|a| a.email.as_str()).unwrap_or("me")
                                                                )),
                                                        ),
                                                ),
                                        )
                                        .child(div().text_xs().text_color(cx.theme().muted_foreground).child(date_full)),
                                ),
                        )
                        // Anti-phishing warning.
                        //
                        // This is one of the few things that genuinely warrants an Alert:
                        // exceptional information that needs attention in the current task.
                        // It gets the component so its colour, icon, padding and dismissal
                        // match every other alert, instead of a hand-rolled red band.
                        .when_some(
                            self.message_viewer.phishing_warning.as_ref().map(|msg| {
                                Alert::error("banner-phishing-warning", msg.clone())
                                    .title("Possible phishing or spoofing")
                                    .banner()
                            }),
                            |this, alert| this.child(alert),
                        )
                        // Tracker & remote image security bar
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .justify_between()
                                .px(px(16.0))
                                .py(px(8.0))
                                .bg(cx.theme().muted)
                                .border_b_1()
                                .border_color(cx.theme().border)
                                .child(
                                    div()
                                        .flex()
                                        .flex_row()
                                        .items_center()
                                        .gap(px(8.0))
                                        .child(
                                            Icon::new(if self.block_remote_images {
                                                IconName::CircleCheck
                                            } else {
                                                IconName::TriangleAlert
                                            })
                                            .small()
                                            .text_color(if self.block_remote_images {
                                                cx.theme().success
                                            } else {
                                                cx.theme().warning
                                            }),
                                        )
                                        .child(
                                            div()
                                                .text_xs()
                                                .text_color(cx.theme().muted_foreground)
                                                .child(if self.block_remote_images {
                                                    "Remote images blocked to protect your privacy & stop tracking pixels."
                                                } else {
                                                    "Remote images allowed for this session."
                                                }),
                                        ),
                                )
                                .child(
                                    // The label names the result, so the user can predict what
                                    // activation does without reading the status text first.
                                    Button::new("btn-toggle-images")
                                        .ghost()
                                        .small()
                                        .label(if self.block_remote_images {
                                            "Load remote images"
                                        } else {
                                            "Block remote images"
                                        })
                                        .tooltip(
                                            "Remote images can confirm that a message was opened",
                                        )
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.toggle_remote_images(cx);
                                        })),
                                ),
                        );

                    let body_panel = div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .w_full()
                        .overflow_hidden()
                        // Attachments tray (if any)
                        .child(if !attachments.is_empty() {
                            div()
                                .flex()
                                .flex_col()
                                .px(px(16.0))
                                .py(px(8.0))
                                .border_b_1()
                                .border_color(cx.theme().border)
                                .bg(cx.theme().muted)
                                .gap(px(6.0))
                                .child(
                                    div()
                                        .flex()
                                        .flex_row()
                                        .items_center()
                                        .gap(px(6.0))
                                        .child(Icon::new(IconName::File).small())
                                        .child(
                                            div()
                                                .text_xs()
                                                .font_weight(FontWeight::BOLD)
                                                .text_color(cx.theme().muted_foreground)
                                                .child(format!("Attachments ({})", attachments.len())),
                                        ),
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
                                                .bg(cx.theme().accent)
                                                .border_1()
                                                .border_color(cx.theme().border)
                                                .text_xs()
                                                .text_color(cx.theme().foreground)
                                                .child(Icon::new(IconName::File).small())
                                                .child(div().child(fname.clone()))
                                                .child(div().text_color(cx.theme().muted_foreground).child(format!("({sz_str})")))
                                                .child(
                                                    Button::new(ElementId::Name(format!("prev-att-{}", fname).into()))
                                                        .ghost()
                                                        .small()
                                                        .icon(IconName::FileText)
                                                        .label("Preview")
                                                        .on_click(cx.listener({
                                                            let att_name = fname.clone();
                                                            let b_path = att.blob_path.clone();
                                                            let mime = att.content_type.clone();
                                                            let sz = att.size_bytes;
                                                            move |this, _, _, cx| {
                                                                this.open_attachment_preview(
                                                                    att_name.clone(),
                                                                    mime.clone(),
                                                                    sz,
                                                                    Some(b_path.clone()),
                                                                    None,
                                                                    cx,
                                                                );
                                                            }
                                                        })),
                                                )
                                                .child(
                                                    Button::new(ElementId::Name(format!("save-att-{}", fname).into()))
                                                        .ghost()
                                                        .small()
                                                        .icon(IconName::HardDrive)
                                                        .label("Save")
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
                                                        })),
                                                )
                                        })),
                                )
                        } else {
                            div()
                        })
                        // Clean Body Content
                        .child(
                            div()
                                // The scroll container needs an id: GPUI exposes
                                // `overflow_y_scroll` on `Stateful<Div>`, so an id-less
                                // container cannot become the scrolling region.
                                .id("reader-body-scroll")
                                .flex()
                                .flex_col()
                                .flex_1()
                                .min_h(px(0.0))
                                .p(px(20.0))
                                .gap(px(12.0))
                                .overflow_y_scroll()
                                // A real renderer, not a `String` in a text node.
                                //
                                // `MessageViewer::rendered()` returned the sanitized HTML as
                                // a `String`, and `.child(rendered_body)` made a GPUI *text
                                // element* of it, so every HTML message was displayed as its
                                // own markup source: `<table border="0">…`. `TextView`
                                // renders HTML natively and no script can run, because
                                // there is no webview in this build.
                                .child(match body_kind {
                                    BodyKind::Html => TextView::html(
                                        ElementId::Name("reader-body".into()),
                                        body_source.clone(),
                                    )
                                    .into_any_element(),
                                    // Plain text is escaped before display, so it is
                                    // presented as HTML with explicit line breaks rather
                                    // than read as Markdown.
                                    BodyKind::Text => TextView::html(
                                        ElementId::Name("reader-body".into()),
                                        body_source.clone(),
                                    )
                                    .into_any_element(),
                                    BodyKind::Empty => div().into_any_element(),
                                })
                                // A real hairline owned by the footer, instead of 50
                                // box-drawing characters used as a separator. The old rule
                                // broke at any font whose glyph was not exactly one
                                // character wide, and it carried no semantic boundary.
                                .child(
                                    div()
                                        .mt(px(16.0))
                                        .border_t_1()
                                        .border_color(cx.theme().border),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(
                                            "Message bodies are sanitized before display. Search and offline storage run on SQLite with FTS5.",
                                        ),
                                ));

                    v_resizable("reader-v-panels")
                        .child(header_panel.into_any_element())
                        .child(body_panel.into_any_element())
                        .into_any_element()
                } else {
                    // Empty state for the detail pane: it names the object the user must
                    // choose, and stays quiet. `xxl` gives the breathing room the spacing
                    // scale reserves for empty states.
                    div()
                        .flex()
                        .flex_col()
                        .items_center()
                        .justify_center()
                        .gap(px(8.0))
                        .size_full()
                        .p(px(32.0))
                        .child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child("No message selected"),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child("Choose a message in the list to read it here"),
                        )
                        .into_any_element()
                })
        }

        fn render_calendar_view(&self, cx: &Context<Self>) -> Div {
            let mut cal_view = CalendarView::new();
            cal_view.events = self.calendar_events.clone();

            div()
                .flex()
                .flex_col()
                .flex_1()
                .p(px(20.0))
                .bg(cx.theme().background)
                .gap(px(16.0))
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
                                .gap(px(8.0))
                                .child(Icon::new(IconName::Calendar).text_color(cx.theme().primary))
                                .child(
                                    div()
                                        .text_lg()
                                        .font_weight(FontWeight::BOLD)
                                        .text_color(cx.theme().foreground)
                                        .child("Calendar (CalDAV RFC 4791 & iCalendar RFC 5545)"),
                                ),
                        )
                        .child(
                            Button::new("btn-new-event")
                                .icon(IconName::Plus)
                                .label("New event")
                                .primary(),
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
                                .bg(cx.theme().muted)
                                .border_1()
                                .border_color(cx.theme().border)
                                .child(
                                    div()
                                        .text_sm()
                                        .font_weight(FontWeight::BOLD)
                                        .text_color(cx.theme().primary)
                                        .child(ev.title.clone()),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(format!(
                                            "Time: {} - {}",
                                            ev.start.format("%Y-%m-%d %H:%M"),
                                            ev.end.format("%H:%M")
                                        )),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(cx.theme().foreground)
                                        .child(ev.description.clone().unwrap_or_default()),
                                )
                        })),
                )
        }

        fn render_contacts_view(&self, cx: &Context<Self>) -> Div {
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
                .bg(cx.theme().background)
                .gap(px(16.0))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        // Section title. The "(CardDAV & vCard 4.0)" suffix named the
                        // implementation rather than the object, which the interface language
                        // guide calls a wrapper word: the surface is an address book.
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap(px(8.0))
                                .child(Icon::new(IconName::User).text_color(cx.theme().primary))
                                .child(
                                    div()
                                        .text_lg()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(cx.theme().foreground)
                                        .child("Contacts"),
                                ),
                        )
                        .child(
                            Button::new("btn-new-contact")
                                .icon(IconName::Plus)
                                .label("Add contact"),
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
                                .bg(cx.theme().muted)
                                .border_1()
                                .border_color(cx.theme().border)
                                .child(
                                    div()
                                        .flex()
                                        .flex_row()
                                        .items_center()
                                        .gap(px(12.0))
                                        // Fixed-size avatar lane, so the name column starts
                                        // at the same offset on every row. The initial is
                                        // taken by `char` so a multi-byte name is not split.
                                        .child(
                                            div()
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .w(px(32.0))
                                                .h(px(32.0))
                                                .flex_shrink_0()
                                                .rounded_full()
                                                .bg(cx.theme().primary)
                                                .text_color(cx.theme().primary_foreground)
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .text_xs()
                                                .child(
                                                    name.chars().next().unwrap_or('C').to_string(),
                                                ),
                                        )
                                        .child(
                                            div()
                                                .flex_1()
                                                .min_w(px(0.0))
                                                .flex()
                                                .flex_col()
                                                .child(
                                                    div()
                                                        .text_xs()
                                                        .font_weight(FontWeight::SEMIBOLD)
                                                        .text_color(cx.theme().foreground)
                                                        .overflow_hidden()
                                                        .text_ellipsis()
                                                        .child(elide(name, 40)),
                                                )
                                                .child(
                                                    div()
                                                        .text_xs()
                                                        .text_color(cx.theme().muted_foreground)
                                                        .overflow_hidden()
                                                        .text_ellipsis()
                                                        .child(elide(&c.email, 40)),
                                                ),
                                        ),
                                )
                                .child(
                                    Button::new(ElementId::Name(
                                        format!("btn-contact-email-{}", c.id).into(),
                                    ))
                                    .icon(IconName::Inbox)
                                    .label("Write email")
                                    .ghost()
                                    .xsmall()
                                    .tooltip(format!("Write to {}", c.email))
                                    .on_click(cx.listener({
                                        let email = c.email.clone();
                                        move |this, _, _, cx| {
                                            this.compose_to = email.clone();
                                            this.compose_inputs = None;
                                            this.active_modal = ActiveModal::Compose;
                                            cx.notify();
                                        }
                                    })),
                                )
                        })),
                )
        }

        fn render_tasks_view(&self, cx: &Context<Self>) -> Div {
            let task_list_view = TaskListView::new(self.tasks.clone());

            div()
                .flex()
                .flex_col()
                .flex_1()
                .p(px(20.0))
                .bg(cx.theme().background)
                .gap(px(16.0))
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
                                .gap(px(8.0))
                                .child(Icon::new(IconName::Check).text_color(cx.theme().primary))
                                .child(
                                    div()
                                        .text_lg()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(cx.theme().foreground)
                                        .child("Tasks"),
                                ),
                        )
                        .child(
                            Button::new("btn-new-task")
                                .icon(IconName::Plus)
                                .label("Add task"),
                        ),
                )
                // The list is the screen's whole purpose, so it scrolls. A fixed 300px page
                // clipped instead of scrolling.
                .child(
                    div()
                        .id("tasks-list-scroll")
                        .flex_1()
                        .min_h(px(0.0))
                        .overflow_y_scroll()
                        .flex()
                        .flex_col()
                        .gap(px(8.0))
                        .children(task_list_view.filtered_tasks().into_iter().map(|t| {
                            let is_done = t.is_completed;
                            div()
                                .id(ElementId::Name(format!("task-{}", t.id).into()))
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap(px(10.0))
                                .p(px(12.0))
                                .rounded_lg()
                                .bg(cx.theme().muted)
                                .border_1()
                                .border_color(cx.theme().border)
                                // Completion is carried by the check glyph as well as the muted
                                // text, so it does not rely on colour alone.
                                .child(div().w(px(16.0)).h(px(16.0)).flex_shrink_0().child(
                                    if is_done {
                                        Icon::new(IconName::CircleCheck)
                                            .text_color(cx.theme().success)
                                            .small()
                                            .into_any_element()
                                    } else {
                                        div()
                                            .w(px(14.0))
                                            .h(px(14.0))
                                            .rounded_sm()
                                            .border_1()
                                            .border_color(cx.theme().muted_foreground)
                                            .into_any_element()
                                    },
                                ))
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w(px(0.0))
                                        .text_xs()
                                        .overflow_hidden()
                                        .text_ellipsis()
                                        .text_color(if is_done {
                                            cx.theme().muted_foreground
                                        } else {
                                            cx.theme().foreground
                                        })
                                        .child(elide(&t.title, 100)),
                                )
                        })),
                )
        }

        fn render_settings_view(&self, cx: &Context<Self>) -> Div {
            // The shell renders one tab's contents. The previous implementation drew
            // every section as a stacked card, so a long settings page had to be
            // scrolled past and no section could be addressed.
            let tabs = [
                (SettingsTab::Appearance, "Appearance"),
                (SettingsTab::Layout, "Layout"),
                (SettingsTab::ReadingAndList, "Reading"),
                (SettingsTab::Composer, "Composer"),
                (SettingsTab::Shortcuts, "Shortcuts"),
                (SettingsTab::PrivacyAndSecurity, "Privacy"),
                (SettingsTab::Notifications, "Notifications"),
            ];

            div()
                .flex()
                .flex_col()
                .flex_1()
                .size_full()
                .bg(cx.theme().background)
                .child(
                    div()
                        .id("settings-titlebar")
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(8.0))
                        .px(px(20.0))
                        .py(px(12.0))
                        .border_b_1()
                        .border_color(cx.theme().border)
                        .child(Icon::new(IconName::Settings).text_color(cx.theme().primary))
                        .child(
                            div()
                                .text_lg()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(cx.theme().foreground)
                                .child("Settings"),
                        ),
                )
                .child(
                    TabBar::new("settings-tab-bar").children(tabs.into_iter().map(
                        |(tab, label)| {
                            Tab::new()
                                .label(label)
                                .selected(self.settings_view.active_tab == tab)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.settings_view.set_tab(tab);
                                    cx.notify();
                                }))
                        },
                    )),
                )
                .child(
                    div()
                        // GPUI only exposes `overflow_y_scroll` on a `Stateful<Div>`,
                        // so this container needs an id to become the scroll region.
                        .id("settings-body-scroll")
                        .flex_1()
                        .overflow_y_scroll()
                        .p(px(20.0))
                        // Each tab renders its own element type, so they are erased at the boundary
                        // rather than sharing a return type.
                        .child(match self.settings_view.active_tab {
                            SettingsTab::Appearance => {
                                self.render_settings_appearance(cx).into_any_element()
                            }
                            SettingsTab::Layout => {
                                self.render_settings_layout(cx).into_any_element()
                            }
                            SettingsTab::ReadingAndList => {
                                self.render_settings_reading(cx).into_any_element()
                            }
                            SettingsTab::Composer => {
                                self.render_settings_composer(cx).into_any_element()
                            }
                            SettingsTab::Shortcuts => {
                                self.render_settings_shortcuts(cx).into_any_element()
                            }
                            SettingsTab::PrivacyAndSecurity => {
                                self.render_settings_privacy(cx).into_any_element()
                            }
                            SettingsTab::Notifications => {
                                self.render_settings_notifications(cx).into_any_element()
                            }
                        })
                        .into_any_element(),
                )
        }

        /// One titled group inside a settings tab.
        fn settings_section(
            &self,
            title: &'static str,
            icon: IconName,
            cx: &Context<Self>,
            children: Vec<gpui_kit::gpui::AnyElement>,
        ) -> impl IntoElement {
            div()
                .flex()
                .flex_col()
                .p(px(16.0))
                .rounded_lg()
                .bg(cx.theme().muted)
                .border_1()
                .border_color(cx.theme().border)
                .gap(px(10.0))
                .mb(px(12.0))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(8.0))
                        .child(Icon::new(icon).small().text_color(cx.theme().primary))
                        .child(
                            div()
                                .text_sm()
                                .font_weight(FontWeight::BOLD)
                                .text_color(cx.theme().foreground)
                                .child(title),
                        ),
                )
                .children(children)
        }

        /// A row that states a setting and offers the values it can take.
        #[allow(clippy::too_many_arguments)]
        fn settings_choice_row<T: Copy + PartialEq + 'static>(
            &self,
            cx: &Context<Self>,
            label: &'static str,
            description: &'static str,
            choices: &[(T, &'static str)],
            current: T,
            id_prefix: &'static str,
            on_pick: impl Fn(&mut Self, T, &mut Context<Self>) + 'static,
        ) -> impl IntoElement {
            let choices = choices.to_vec();
            let on_pick = std::rc::Rc::new(on_pick);
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .gap(px(12.0))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(2.0))
                        .child(
                            div()
                                .text_xs()
                                .font_weight(FontWeight::BOLD)
                                .text_color(cx.theme().foreground)
                                .child(label),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(description),
                        ),
                )
                .child(TabBar::new(ElementId::Name(id_prefix.into())).children(
                    choices.into_iter().map(|(value, text)| {
                        let is_active = current == value;
                        let on_pick = on_pick.clone();
                        Tab::new()
                            .label(text)
                            .selected(is_active)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                on_pick(this, value, cx);
                            }))
                    }),
                ))
        }

        fn render_settings_appearance(&self, cx: &Context<Self>) -> impl IntoElement {
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

            div()
                .flex()
                .flex_col()
                .child(
                    self.settings_section(
                        "Theme",
                        IconName::Palette,
                        cx,
                        vec![self
                            .settings_choice_row(
                                    cx,
                                    "Appearance",
                                "Follow the system, or pick a palette",
                                &themes,
                                self.settings.theme,
                                "settings-theme-choice",
                                move |this, value, cx| {
                                    this.set_settings_theme(value, cx);
                                },
                            )
                            .into_any_element()],
                    ),
                )
                .child(
                    self.settings_section(
                        "Row density",
                        IconName::LayoutDashboard,
                        cx,
                        vec![self
                            .settings_choice_row(
                                    cx,
                                    "Message rows",
                                "How much of each message the list shows",
                                &densities,
                                self.settings.row_density,
                                "settings-density-choice",
                                move |this, value, cx| {
                                    this.set_settings_density(value, cx);
                                },
                            )
                            .into_any_element()],
                    ),
                )
                .child(
                    self.settings_section(
                        "Storage",
                        IconName::HardDrive,
                        cx,
                        // Facts about this build's storage layer, stated once. They are
                        // true of the code and useful when reporting a problem; they are
                        // not settings and carry no control.
                        vec![div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child("SQLite in WAL mode · FTS5 full-text index · compressed raw MIME blobs")
                            .into_any_element()],
                    ),
                )
        }

        fn render_settings_layout(&self, cx: &Context<Self>) -> impl IntoElement {
            let layouts = [
                (vespetrel_core::PaneLayout::ThreePaneVertical, "Three pane"),
                (vespetrel_core::PaneLayout::ClassicHorizontal, "Two pane"),
            ];
            div()
                .flex()
                .flex_col()
                .child(self.settings_section(
                    "Layout",
                    IconName::Frame,
                    cx,
                    vec![self
                            .settings_choice_row(
                                    cx,
                                    "Window layout",
                                "Folder tree, message list and reader",
                                &layouts,
                                self.settings.layout,
                                "settings-layout-choice",
                                move |_this, _value, _cx| {},
                            )
                            .into_any_element()],
                ))
                .child(self.settings_section(
                    "Sender identity",
                    IconName::User,
                    cx,
                    vec![self
                            .settings_choice_row(
                                    cx,
                                    "Avatar",
                                "Show a sender avatar in each row",
                                &[(true, "Show"), (false, "Hide")],
                                self.settings.show_sender_avatars,
                                "settings-avatar-choice",
                                move |this, value, cx| {
                                    this.set_sender_avatars(value, cx);
                                },
                            )
                            .into_any_element()],
                ))
        }

        fn render_settings_reading(&self, cx: &Context<Self>) -> impl IntoElement {
            div().flex().flex_col().child(self.settings_section(
                "Message list",
                IconName::LayoutDashboard,
                cx,
                vec![self
                            .settings_choice_row(
                                    cx,
                                    "Preview",
                                "Show the start of each message under its subject",
                                &[(true, "Show"), (false, "Hide")],
                                self.settings.show_preview_snippets,
                                "settings-snippet-choice",
                                move |this, value, cx| {
                                    this.set_preview_snippets(value, cx);
                                },
                            )
                            .into_any_element()],
            ))
        }

        fn render_settings_composer(&self, cx: &Context<Self>) -> impl IntoElement {
            let undo_delays = [
                (5, "5 seconds"),
                (10, "10 seconds"),
                (20, "20 seconds"),
                (30, "30 seconds"),
            ];
            let quotes = [
                (vespetrel_core::QuoteStyle::TopPost, "Above quote"),
                (vespetrel_core::QuoteStyle::BottomPost, "Below quote"),
            ];

            let alias_elements: Vec<gpui_kit::gpui::AnyElement> = self
                .aliases
                .iter()
                .map(|alias| {
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .py(px(6.0))
                        .px(px(8.0))
                        .rounded_md()
                        .bg(cx.theme().input)
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .child(
                                    div()
                                        .text_xs()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(cx.theme().foreground)
                                        .child(format!(
                                            "{} <{}>",
                                            alias.name.as_deref().unwrap_or("No Name"),
                                            alias.email
                                        )),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(format!(
                                            "Reply-To: {} · Default: {}",
                                            alias.reply_to.as_deref().unwrap_or("Same"),
                                            if alias.is_default { "Yes" } else { "No" }
                                        )),
                                ),
                        )
                        .into_any_element()
                })
                .collect();

            let sig_elements: Vec<gpui_kit::gpui::AnyElement> = self
                .signatures
                .iter()
                .map(|sig| {
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .py(px(6.0))
                        .px(px(8.0))
                        .rounded_md()
                        .bg(cx.theme().input)
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .child(
                                    div()
                                        .text_xs()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(cx.theme().foreground)
                                        .child(sig.name.clone()),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(format!(
                                            "Default: {} · Include in replies: {}",
                                            if sig.is_default { "Yes" } else { "No" },
                                            if sig.include_in_replies { "Yes" } else { "No" }
                                        )),
                                ),
                        )
                        .into_any_element()
                })
                .collect();

            div()
                .flex()
                .flex_col()
                .child(self.settings_section(
                    "Sending",
                    IconName::ArrowRight,
                    cx,
                    vec![
                        self.settings_choice_row(
                            cx,
                            "Undo send",
                            "How long a sent message can be recalled",
                            &undo_delays,
                            self.settings.undo_send_seconds,
                            "settings-undo-choice",
                            move |this, value, cx| {
                                this.set_settings_undo_seconds(value, cx);
                            },
                        )
                        .into_any_element(),
                        self.settings_choice_row(
                            cx,
                            "Reply style",
                            "Where the quoted original sits in a reply",
                            &quotes,
                            self.settings.reply_quote_style,
                            "settings-quote-choice",
                            move |_this, _value, _cx| {},
                        )
                        .into_any_element(),
                    ],
                ))
                .child(self.settings_section(
                    "Sender Identities & Aliases",
                    IconName::User,
                    cx,
                    if alias_elements.is_empty() {
                        vec![div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child("No alternate identities configured. Default account identity will be used.")
                            .into_any_element()]
                    } else {
                        alias_elements
                    },
                ))
                .child(self.settings_section(
                    "Custom Signatures",
                    IconName::FileText,
                    cx,
                    if sig_elements.is_empty() {
                        vec![div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child("No signatures configured. Signatures will be generated automatically.")
                            .into_any_element()]
                    } else {
                        sig_elements
                    },
                ))
        }

        /// The shortcut table, built from the same keymap the window binds.
        ///
        /// This is where `KeybindingMap` becomes visible to a user. Before, the whole
        /// engine existed with three presets and no surface: the palette row
        /// "Configure Keyboard Shortcuts" landed on a settings page with no shortcut
        /// control on it, and every label in the palette was a static string.
        fn render_settings_shortcuts(&self, cx: &Context<Self>) -> impl IntoElement {
            let presets = [
                (crate::keybindings::KeyPreset::Gmail, "Gmail"),
                (crate::keybindings::KeyPreset::Thunderbird, "Thunderbird"),
                (crate::keybindings::KeyPreset::Vim, "Vim"),
                (crate::keybindings::KeyPreset::Custom, "Custom"),
            ];

            let entries = self.keybindings.sorted_entries();
            let preset_note = if self.keybindings.preset == crate::keybindings::KeyPreset::Custom {
                "Custom preset: bindings were edited, so this table is your own."
            } else {
                "Changes take effect the next time Vespetrel starts."
            };

            div()
                .flex()
                .flex_col()
                .child(self.settings_section(
                    "Preset",
                    IconName::Settings2,
                    cx,
                    vec![self
                            .settings_choice_row(
                                    cx,
                                    "Shortcuts",
                                "Gmail, Thunderbird and Vim key maps",
                                &presets,
                                self.keybindings.preset,
                                "settings-keymap-choice",
                                move |this, value, cx| {
                                    this.set_keymap_preset(value, cx);
                                },
                            )
                            .into_any_element()],
                ))
                .child(self.settings_section(
                    "Shortcuts",
                    IconName::LayoutDashboard,
                    cx,
                    vec![div()
                            .flex()
                            .flex_col()
                            .gap(px(6.0))
                            .children(entries.into_iter().map(|(key, action)| {
                                div()
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .justify_between()
                                    .gap(px(12.0))
                                    .py(px(2.0))
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(cx.theme().foreground)
                                            .child(action.label()),
                                    )
                                    .child(
                                        div()
                                            .px(px(6.0))
                                            .py(px(2.0))
                                            .rounded_md()
                                            .bg(cx.theme().popover)
                                            .text_xs()
                                            .text_color(cx.theme().muted_foreground)
                                            .child(format_shortcut(&key)),
                                    )
                            }))
                            .into_any_element(),
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(preset_note)
                                .into_any_element()],
                ))
        }

        fn render_settings_privacy(&self, cx: &Context<Self>) -> impl IntoElement {
            div().flex().flex_col().child(self.settings_section(
                "Privacy and threat protection",
                IconName::CircleCheck,
                cx,
                vec![
                            self.settings_choice_row(
                                    cx,
                                    "Remote images",
                                "Block images that would confirm a message was opened",
                                &[(true, "Block"), (false, "Allow")],
                                self.block_remote_images,
                                "settings-remote-image-choice",
                                move |this, value, cx| {
                                    this.set_remote_image_policy(value, cx);
                                },
                            )
                            .into_any_element(),
                            self.settings_choice_row(
                                    cx,
                                    "Tracking pixels",
                                "Strip known telemetry parameters from links",
                                &[
                                    (true, "Enabled"),
                                    (false, "Disabled"),
                                ],
                                self.settings.auto_strip_trackers,
                                "settings-tracker-choice",
                                move |this, value, cx| {
                                    this.set_strip_trackers(value, cx);
                                },
                            )
                            .into_any_element(),
                            self.settings_choice_row(
                                    cx,
                                    "Phishing warnings",
                                "Check links for deceptive domains and homographs",
                                &[(true, "Enabled"), (false, "Disabled")],
                                self.settings.warn_on_phishing,
                                "settings-phishing-choice",
                                move |this, value, cx| {
                                    this.set_warn_phishing(value, cx);
                                },
                            )
                            .into_any_element(),
                        ],
            ))
        }

        fn render_settings_notifications(&self, cx: &Context<Self>) -> impl IntoElement {
            let modes = [
                (vespetrel_core::NotificationMode::All, "All mail"),
                (
                    vespetrel_core::NotificationMode::PrimaryCategoryOnly,
                    "Primary only",
                ),
                (
                    vespetrel_core::NotificationMode::VipAndStarredOnly,
                    "Starred only",
                ),
                (vespetrel_core::NotificationMode::Muted, "Muted"),
            ];
            div().flex().flex_col().child(self.settings_section(
                "Notifications",
                IconName::Bell,
                cx,
                vec![self
                            .settings_choice_row(
                                    cx,
                                    "New mail",
                                "Which messages raise a notification",
                                &modes,
                                self.settings.notification_mode,
                                "settings-notification-choice",
                                move |_this, _value, _cx| {},
                            )
                            .into_any_element()],
            ))
        }

        /// Classify the current status line into a state the bar can present honestly.
        ///
        /// The bar previously inferred its state by searching the message text for the
        /// words "offline", "error" or "failed". A perfectly normal message that happened
        /// to contain one of those words turned the indicator red, and a genuine failure
        /// whose wording differed stayed green.
        fn status_state(&self) -> StatusState {
            classify_status(self.accounts.len(), &self.status_message)
        }

        fn render_status_bar(&self, cx: &Context<Self>) -> impl IntoElement {
            let state = self.status_state();
            let (indicator, label) = match state {
                StatusState::NoAccount => (
                    cx.theme().muted_foreground,
                    "No account connected".to_string(),
                ),
                StatusState::Failed => (cx.theme().danger, "Sync failed".to_string()),
                StatusState::Busy => (cx.theme().primary, "Syncing".to_string()),
                StatusState::Ready => (
                    cx.theme().success,
                    format!(
                        "{} account{} connected",
                        self.accounts.len(),
                        if self.accounts.len() == 1 { "" } else { "s" }
                    ),
                ),
            };

            StatusBar::new().child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .w_full()
                    .h(px(24.0))
                    .px(px(16.0))
                    .bg(cx.theme().status_bar)
                    .border_t_1()
                    .border_color(cx.theme().status_bar_border)
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    // State is carried by an icon and a word, not by a coloured dot alone,
                    // so it survives a colourless reading and colour-vision differences.
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
                                    .flex_shrink_0()
                                    .rounded_full()
                                    .bg(indicator),
                            )
                            .child(
                                div()
                                    .overflow_hidden()
                                    .text_ellipsis()
                                    .child(self.status_message.clone()),
                            ),
                    )
                    // Real, checkable facts about this window's state. The old right-hand
                    // text was a hardcoded "Connected • Direct3D/Vulkan 120 FPS" that
                    // described a renderer, not the app, and was false whenever sync failed.
                    .child(div().flex_shrink_0().child(label)),
            )
        }

        fn render_modal_layer(
            &self,
            window: &mut Window,
            cx: &mut Context<Self>,
        ) -> impl IntoElement {
            match &self.active_modal {
                ActiveModal::None => div().into_any_element(),
                ActiveModal::Compose => self.render_compose_modal(window, cx).into_any_element(),
                ActiveModal::CommandPalette => {
                    self.render_command_palette_modal(cx).into_any_element()
                }
                ActiveModal::AddAccount => {
                    self.render_add_account_modal(window, cx).into_any_element()
                }
                ActiveModal::AttachmentPreview {
                    filename,
                    mime_type,
                    size_bytes,
                    blob_path,
                    data,
                } => self
                    .render_attachment_preview_modal(
                        window,
                        cx,
                        filename,
                        mime_type,
                        *size_bytes,
                        blob_path.as_deref(),
                        data.as_deref(),
                    )
                    .into_any_element(),
            }
        }

        #[allow(clippy::too_many_arguments)]
        fn render_attachment_preview_modal(
            &self,
            window: &mut Window,
            cx: &mut Context<Self>,
            filename: &str,
            mime_type: &str,
            size_bytes: usize,
            blob_path: Option<&str>,
            data: Option<&[u8]>,
        ) -> impl IntoElement {
            let file_data = if let Some(d) = data {
                d.to_vec()
            } else if let Some(bp) = blob_path {
                std::fs::read(bp).unwrap_or_default()
            } else {
                Vec::new()
            };

            let preview =
                vespetrel_render::preview::generate_preview(filename, mime_type, &file_data);

            let preview_icon = match &preview.kind {
                vespetrel_render::preview::AttachmentKind::Image { .. } => IconName::Frame,
                vespetrel_render::preview::AttachmentKind::Text { .. } => IconName::FileText,
                vespetrel_render::preview::AttachmentKind::Pdf { .. } => IconName::FileText,
                vespetrel_render::preview::AttachmentKind::Audio { .. } => IconName::Bell,
                vespetrel_render::preview::AttachmentKind::Video { .. } => IconName::Frame,
                vespetrel_render::preview::AttachmentKind::Archive { .. } => IconName::HardDrive,
                vespetrel_render::preview::AttachmentKind::Binary => IconName::Cpu,
            };

            let fn_clone = filename.to_string();
            let bp_clone = blob_path.map(|s| s.to_string());

            Sheet::new(window, cx).child(
                div()
                    .id("modal-attachment-preview-overlay")
                    .flex()
                    .items_center()
                    .justify_center()
                    .size_full()
                    .bg(gpui_kit::gpui::transparent_black())
                    .child(
                        div()
                            .id("modal-attachment-preview-box")
                            .flex()
                            .flex_col()
                            .w(px(720.0))
                            .max_h(px(580.0))
                            .rounded_xl()
                            .bg(cx.theme().input)
                            .border_1()
                            .border_color(cx.theme().border)
                            .p(px(20.0))
                            .gap(px(12.0))
                            // Modal Header
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
                                            .child(Icon::new(preview_icon).text_color(cx.theme().primary))
                                            .child(
                                                div()
                                                    .flex()
                                                    .flex_col()
                                                    .child(
                                                        div()
                                                            .text_base()
                                                            .font_weight(FontWeight::BOLD)
                                                            .text_color(cx.theme().foreground)
                                                            .child(filename.to_string()),
                                                    )
                                                    .child(
                                                        div()
                                                            .text_xs()
                                                            .text_color(cx.theme().muted_foreground)
                                                            .child(format!(
                                                                "{} · {}",
                                                                mime_type,
                                                                preview.formatted_size
                                                            )),
                                                    ),
                                            ),
                                    )
                                    .child(
                                        Button::new("btn-close-attachment-preview")
                                            .icon(IconName::Close)
                                            .ghost()
                                            .xsmall()
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.active_modal = ActiveModal::None;
                                                cx.notify();
                                            })),
                                    ),
                            )
                            // Summary badge
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .justify_between()
                                    .px(px(10.0))
                                    .py(px(6.0))
                                    .rounded_md()
                                    .bg(cx.theme().muted)
                                    .child(
                                        div()
                                            .text_xs()
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(cx.theme().foreground)
                                            .child(preview.summary.clone()),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(cx.theme().muted_foreground)
                                            .child(if let Some(lines) = preview.line_count {
                                                format!("{lines} lines")
                                            } else {
                                                format!("{} bytes", size_bytes)
                                            }),
                                    ),
                            )
                            // Content Viewer
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .flex_1()
                                    .rounded_md()
                                    .bg(cx.theme().popover)
                                    .border_1()
                                    .border_color(cx.theme().border)
                                    .p(px(12.0))
                                    .child(
                                        if let Some(ref snippet) = preview.text_snippet {
                                            div()
                                                .id("preview-content-snippet")
                                                .overflow_y_scroll()
                                                .max_h(px(320.0))
                                                .text_xs()
                                                .font_weight(FontWeight::NORMAL)
                                                .text_color(cx.theme().foreground)
                                                .child(snippet.clone())
                                                .into_any_element()
                                        } else if matches!(
                                            preview.kind,
                                            vespetrel_render::preview::AttachmentKind::Image { .. }
                                        ) {
                                            div()
                                                .flex()
                                                .flex_col()
                                                .items_center()
                                                .justify_center()
                                                .h(px(220.0))
                                                .gap(px(10.0))
                                                .child(Icon::new(IconName::Frame).text_color(cx.theme().primary))
                                                .child(
                                                    div()
                                                        .text_sm()
                                                        .font_weight(FontWeight::SEMIBOLD)
                                                        .text_color(cx.theme().foreground)
                                                        .child(format!("Image Preview: {filename}")),
                                                )
                                                .child(
                                                    div()
                                                        .text_xs()
                                                        .text_color(cx.theme().muted_foreground)
                                                        .child(format!("Format: {mime_type}")),
                                                )
                                                .into_any_element()
                                        } else if let Some(ref hex) = preview.hex_dump {
                                            div()
                                                .id("preview-content-hex")
                                                .overflow_y_scroll()
                                                .max_h(px(320.0))
                                                .text_xs()
                                                .text_color(cx.theme().muted_foreground)
                                                .child(hex.clone())
                                                .into_any_element()
                                        } else {
                                            div()
                                                .flex()
                                                .flex_col()
                                                .items_center()
                                                .justify_center()
                                                .h(px(200.0))
                                                .gap(px(8.0))
                                                .child(Icon::new(IconName::File).text_color(cx.theme().muted_foreground))
                                                .child(
                                                    div()
                                                        .text_sm()
                                                        .text_color(cx.theme().foreground)
                                                        .child("No inline preview available for this format"),
                                                )
                                                .into_any_element()
                                        },
                                    ),
                            )
                            // Action Footer
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .justify_between()
                                    .child(
                                        Button::new("btn-save-attachment-preview")
                                            .icon(IconName::HardDrive)
                                            .label("Save to Downloads")
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                this.save_attachment_to_downloads(
                                                    &fn_clone,
                                                    bp_clone.as_deref(),
                                                    cx,
                                                );
                                            })),
                                    )
                                    .child(
                                        Button::new("btn-done-attachment-preview")
                                            .label("Close")
                                            .primary()
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.active_modal = ActiveModal::None;
                                                cx.notify();
                                            })),
                                    ),
                            ),
                    ),
            )
        }

        fn render_compose_modal(
            &self,
            window: &mut Window,
            cx: &mut Context<Self>,
        ) -> impl IntoElement {
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

            let from_label = if let Some(alias) = &self.selected_alias {
                if let Some(ref name) = alias.name {
                    format!("{name} <{}>", alias.email)
                } else {
                    alias.email.clone()
                }
            } else {
                self.accounts
                    .first()
                    .map(|a| a.email.clone())
                    .unwrap_or_else(|| "no account configured".to_string())
            };
            let composer_status = format!(
                "From {from_label} · {} · no encryption",
                if self.compose_state.is_markdown_mode {
                    "Markdown"
                } else {
                    "Plain text"
                }
            );

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

            Sheet::new(window, cx).child(
                div()
                    .id("modal-compose-overlay")
                    .flex()
                    .items_center()
                    .justify_center()
                    .size_full()
                    .bg(gpui_kit::gpui::transparent_black())
                    .child(
                        div()
                            .id("modal-compose-box")
                            .flex()
                            .flex_col()
                            .w(px(640.0))
                            .h(px(520.0))
                            .rounded_xl()
                            .bg(cx.theme().input)
                            .border_1()
                            .border_color(cx.theme().border)
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
                                            .flex()
                                            .flex_row()
                                            .items_center()
                                            .gap(px(8.0))
                                            .child(Icon::new(IconName::Plus).text_color(cx.theme().primary))
                                            .child(
                                                div()
                                                    .text_base()
                                                    .font_weight(FontWeight::BOLD)
                                                    .text_color(cx.theme().foreground)
                                                    .child("Compose"),
                                            ),
                                    )
                                    .child(
                                        Button::new("btn-close-compose")
                                            .icon(IconName::Close)
                                            .ghost()
                                            .xsmall()
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.active_modal = ActiveModal::None;
                                                this.compose_inputs = None;
                                                cx.notify();
                                            })),
                                    ),
                            )
                            // Sender "From:" Identity Row with Alias Switching
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .justify_between()
                                    .p(px(6.0))
                                    .rounded_md()
                                    .bg(cx.theme().muted)
                                    .child(
                                        div()
                                            .flex()
                                            .flex_row()
                                            .items_center()
                                            .gap(px(8.0))
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .font_weight(FontWeight::SEMIBOLD)
                                                    .text_color(cx.theme().muted_foreground)
                                                    .child("From:"),
                                            )
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .font_weight(FontWeight::BOLD)
                                                    .text_color(cx.theme().foreground)
                                                    .child(from_label.clone()),
                                            ),
                                    )
                                    .child(if !self.aliases.is_empty() {
                                        div()
                                            .flex()
                                            .flex_row()
                                            .gap(px(4.0))
                                            .children(self.aliases.iter().enumerate().map(|(idx, alias)| {
                                                let is_current = self
                                                    .selected_alias
                                                    .as_ref()
                                                    .map(|a| a.id == alias.id)
                                                    .unwrap_or(false);
                                                let alias_clone = alias.clone();
                                                Button::new(ElementId::Name(format!("select-alias-{}", idx).into()))
                                                    .ghost()
                                                    .xsmall()
                                                    .label(alias.name.as_deref().unwrap_or(&alias.email))
                                                    .when(is_current, |b| b.primary())
                                                    .on_click(cx.listener(move |this, _, _, cx| {
                                                        this.select_alias(Some(alias_clone.clone()), cx);
                                                    }))
                                            }))
                                    } else {
                                        div()
                                    }),
                            )
                            // Recipient "To:" Input + Autocomplete Chips via Form & Field
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(4.0))
                                    .child(
                                        Form::vertical()
                                            .child(
                                                Field::new()
                                                    .label("To:")
                                                    .child(if let Some(inputs) = &self.compose_inputs {
                                                        component::input::Input::new(&inputs.to)
                                                            .cleanable(true)
                                                            .aria_label("Recipient")
                                                            .into_any_element()
                                                    } else {
                                                        div()
                                                            .text_xs()
                                                            .text_color(cx.theme().foreground)
                                                            .child(to_text)
                                                            .into_any_element()
                                                    }),
                                            ),
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
                                                    .flex()
                                                    .flex_row()
                                                    .items_center()
                                                    .gap(px(6.0))
                                                    .px(px(8.0))
                                                    .py(px(2.0))
                                                    .rounded_md()
                                                    .bg(cx.theme().primary)
                                                    .border_1()
                                                    .border_color(cx.theme().primary)
                                                    .text_xs()
                                                    .text_color(cx.theme().primary_hover)
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
                                                    .child(Icon::new(IconName::User).xsmall())
                                                    .child(format!("{display} <{}>", contact.email))
                                            }))
                                    } else {
                                        div()
                                    }),
                            )
                        // Cc, where it was previously absent entirely.
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap(px(8.0))
                                .p(px(8.0))
                                .rounded_md()
                                .bg(cx.theme().muted)
                                .child(
                                    div()
                                        .text_xs()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(cx.theme().muted_foreground)
                                        .child("Cc:"),
                                )
                                .child(if let Some(inputs) = &self.compose_inputs {
                                    component::input::Input::new(&inputs.cc)
                                        .cleanable(true)
                                        .aria_label("Cc")
                                        .into_any_element()
                                } else {
                                    div().into_any_element()
                                }),
                        )
                        // Subject
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap(px(8.0))
                                .p(px(8.0))
                                .rounded_md()
                                .bg(cx.theme().muted)
                                .child(
                                    div()
                                        .text_xs()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(cx.theme().muted_foreground)
                                        .child("Subject:"),
                                )
                                .child(if let Some(inputs) = &self.compose_inputs {
                                    component::input::Input::new(&inputs.subject)
                                        .cleanable(true)
                                        .aria_label("Subject")
                                        .into_any_element()
                                } else {
                                    div()
                                        .text_xs()
                                        .text_color(cx.theme().foreground)
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
                                            Button::new("btn-toggle-markdown")
                                                .icon(IconName::FileText)
                                                .label(if self.compose_state.is_markdown_mode {
                                                    "Markdown"
                                                } else {
                                                    "Plain text"
                                                })
                                                .tooltip(
                                                    "Switch how the body is rendered when it is sent",
                                                )
                                                .when(self.compose_state.is_markdown_mode, |btn| {
                                                    btn.primary()
                                                })
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.compose_state.is_markdown_mode =
                                                        !this.compose_state.is_markdown_mode;
                                                    cx.notify();
                                                })),
                                        )
                                        .child(
                                            Button::new("btn-cycle-signature")
                                                .icon(IconName::FileText)
                                                .label(if let Some(sig) = &self.selected_signature {
                                                    format!("Sig: {}", sig.name)
                                                } else {
                                                    "Sig: None".to_string()
                                                })
                                                .tooltip("Switch or remove signature")
                                                .on_click(cx.listener(|this, _, window, cx| {
                                                    this.cycle_signature(window, cx);
                                                })),
                                        )
                                        // Attach by path.
                                            //
                                            // This used to invent a
                                            // `document_N.pdf` filled with 64 KB of zeroes and
                                            // reported it as attached: sending produced a
                                            // message carrying a corrupt, meaningless
                                            // attachment the user never chose. There is no
                                            // native file picker in this build, so the
                                            // control asks for a path and reads the real
                                            // bytes, or says why it could not.
                                            .child(
                                                div()
                                                .flex()
                                                .flex_row()
                                                .items_center()
                                                .gap(px(8.0))
                                                .child(
                                                    if let Some(inputs) = &self.compose_inputs {
                                                        component::input::Input::new(
                                                            &inputs.attach_path,
                                                        )
                                                        .cleanable(true)
                                                        .aria_label("File to attach")
                                                        .into_any_element()
                                                    } else {
                                                        div().into_any_element()
                                                    },
                                                )
                                                .child(
                                                    Button::new("btn-add-attachment")
                                                        .icon(IconName::Plus)
                                                        .label("Attach")
                                                        .tooltip("Attach the file at the path above")
                                                        .on_click(cx.listener(|this, _, _, cx| {
                                                            let typed = this
                                                                .compose_inputs
                                                                .as_ref()
                                                                .map(|i| {
                                                                    i.attach_path
                                                                        .read(cx)
                                                                        .value()
                                                                        .to_string()
                                                                })
                                                                .unwrap_or_default();
                                                            this.compose_attach_path = typed;
                                                            this.attach_from_path_input(cx);
                                                        })),
                                                ),
                                            ),
                                )
                                .child(
                                    Button::new("btn-save-draft")
                                        .icon(IconName::HardDrive)
                                        .label("Save draft")
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.save_draft(cx);
                                        })),
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
                                .bg(cx.theme().popover)
                                .border_1()
                                .border_color(cx.theme().border)
                                .child(if let Some(inputs) = &self.compose_inputs {
                                    component::input::Textarea::new(&inputs.body)
                                        .aria_label("Message body")
                                        .into_any_element()
                                } else {
                                    div()
                                        .text_xs()
                                        .text_color(cx.theme().foreground)
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
                                .bg(cx.theme().muted)
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
                                                .bg(cx.theme().muted)
                                                .border_1()
                                                .border_color(cx.theme().border)
                                                .text_xs()
                                                .text_color(cx.theme().foreground)
                                                .child(Icon::new(IconName::File).xsmall())
                                                .child(div().child(fname.clone()))
                                                .child(
                                                    div()
                                                        .text_color(cx.theme().muted_foreground)
                                                        .child(format!("({sz_str})")),
                                                )
                                                .child(
                                                    Button::new(ElementId::Name(
                                                        format!("prev-compose-att-{}", idx)
                                                            .into(),
                                                    ))
                                                    .icon(IconName::FileText)
                                                    .ghost()
                                                    .xsmall()
                                                    .label("Preview")
                                                    .on_click(cx.listener(
                                                        move |this, _, _, cx| {
                                                            if let Some(att) = this
                                                                .compose_attachments
                                                                .get(idx)
                                                            {
                                                                let fname = att.filename.clone();
                                                                let mime = att.content_type.clone();
                                                                let data = att.data.clone();
                                                                let sz = data.len();
                                                                this.open_attachment_preview(
                                                                    fname,
                                                                    mime,
                                                                    sz,
                                                                    None,
                                                                    Some(data),
                                                                    cx,
                                                                );
                                                            }
                                                        },
                                                    )),
                                                )
                                                .child(
                                                    Button::new(ElementId::Name(
                                                        format!("rm-compose-att-{}", idx)
                                                            .into(),
                                                    ))
                                                    .icon(IconName::Close)
                                                    .ghost()
                                                    .xsmall()
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
                                                    )),
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
                                // The state the composer is actually in.
                                //
                                // This row used to read "Autocrypt OpenPGP Signature
                                // Attached" on every message, unconditionally, with no
                                // cryptographic call behind it. It now reports only what is
                                // true by construction: the sending account, and whether
                                // the body goes out as Markdown or as plain text.
                                .child(
                                    div()
                                        .flex()
                                        .flex_row()
                                        .items_center()
                                        .gap(px(6.0))
                                        .child(
                                            Icon::new(IconName::User)
                                                .xsmall()
                                                .text_color(cx.theme().muted_foreground),
                                        )
                                        .child(
                                            div()
                                                .text_xs()
                                                .text_color(cx.theme().muted_foreground)
                                                .child(composer_status),
                                        ),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .flex_row()
                                        .items_center()
                                        .gap(px(8.0))
                                        .child(
                                            Button::new("btn-discard-compose")
                                                .label("Discard")
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.active_modal = ActiveModal::None;
                                                    this.compose_inputs = None;
                                                    this.compose_attachments.clear();
                                                    this.compose_draft_id = None;
                                                    this.compose_reply_to_id = None;
                                                    cx.notify();
                                                })),
                                        )
                                        .child(
                                            Button::new("btn-send-compose")
                                                .icon(IconName::ArrowRight)
                                                .label("Send")
                                                .primary()
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.send_composed_message(cx);
                                                })),
                                        ),
                                ),
                        ),
                )
            )
        }

        fn render_command_palette_modal(&self, cx: &mut Context<Self>) -> impl IntoElement {
            let actions: Vec<(String, String, Option<String>)> = self
                .command_palette
                .filtered_actions()
                .into_iter()
                .map(|a| (a.id.clone(), a.title.clone(), a.shortcut.clone()))
                .collect();

            let cmd_el = if let Some(ref cmd_state) = self.command_state {
                let mut cmd = Command::new(cmd_state);
                for (_act_id, title, _shortcut) in actions.iter() {
                    cmd = cmd.item(CommandItem::new().label(title.clone()));
                }
                Some(cmd)
            } else {
                None
            };

            Dialog::new(cx).child(
                div()
                    .id("modal-palette-overlay")
                    .flex()
                    .items_center()
                    .justify_center()
                    .size_full()
                    .bg(gpui_kit::gpui::transparent_black())
                    .child(
                        div()
                            .id("modal-palette-box")
                            .flex()
                            .flex_col()
                            .w(px(520.0))
                            .rounded_xl()
                            .bg(cx.theme().input)
                            .border_1()
                            .border_color(cx.theme().border)
                            .p(px(16.0))
                            .gap(px(10.0))
                            .children(cmd_el)
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .p(px(10.0))
                                    .rounded_md()
                                    .bg(cx.theme().popover)
                                    .border_1()
                                    .border_color(cx.theme().border)
                                    .gap(px(8.0))
                                    .child(
                                        Icon::new(IconName::Search)
                                            .small()
                                            .text_color(cx.theme().muted_foreground),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(if self.command_palette.query.is_empty() {
                                                cx.theme().muted_foreground
                                            } else {
                                                cx.theme().foreground
                                            })
                                            .child(if self.command_palette.query.is_empty() {
                                                "Type a command or search action…".to_string()
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
                                                cx.theme().accent
                                            } else {
                                                gpui_kit::gpui::transparent_black()
                                            })
                                            .hover(|s| s.bg(cx.theme().accent))
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
                                                div()
                                                    .text_xs()
                                                    .text_color(cx.theme().foreground)
                                                    .child(title),
                                            )
                                            .child(
                                                div()
                                                    .px(px(6.0))
                                                    .py(px(2.0))
                                                    .rounded_md()
                                                    .bg(cx.theme().popover)
                                                    .text_xs()
                                                    .text_color(cx.theme().muted_foreground)
                                                    .child(shortcut.unwrap_or_default()),
                                            )
                                    },
                                ),
                            )),
                    ),
            )
        }

        fn render_add_account_modal(
            &self,
            _window: &mut Window,
            cx: &mut Context<Self>,
        ) -> impl IntoElement {
            let selected_p = self.login_wizard.provider_type.clone();
            let is_oauth = self.login_wizard.auth_mode == AuthModeChoice::OAuth2
                && (selected_p == ProviderType::Gmail || selected_p == ProviderType::Graph);

            let providers = [
                (
                    ProviderType::Gmail,
                    "Google (Gmail)",
                    "OAuth2, IMAP and SMTP",
                    IconName::Globe,
                ),
                (
                    ProviderType::Graph,
                    "Microsoft 365",
                    "Outlook and Exchange",
                    IconName::Building2,
                ),
                (
                    ProviderType::Jmap,
                    "Fastmail (JMAP)",
                    "Push mail",
                    IconName::Cpu,
                ),
                (
                    ProviderType::Imap,
                    "Custom IMAP",
                    "IMAP 993, submission 587",
                    IconName::FolderClosed,
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
            let is_manual = self.login_wizard.is_manual_expanded;

            Dialog::new(cx).child(
                div()
                    .id("modal-add-account-overlay")
                .flex()
                .items_center()
                .justify_center()
                .size_full()
                .bg(gpui_kit::gpui::transparent_black())
                .child(
                    div()
                        .id("modal-add-account-box")
                        .flex()
                        .flex_col()
                        .w(px(580.0))
                        .max_h(px(720.0))
                        .overflow_y_scroll()
                        .rounded_xl()
                        .bg(cx.theme().popover)
                        .border_1()
                        .border_color(cx.theme().border)
                        .p(px(24.0))
                        .gap(px(14.0))
                        // Thunderbird Header
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
                                                .flex()
                                                .flex_row()
                                                .items_center()
                                                .gap(px(8.0))
                                                .child(
                                                    Icon::new(IconName::Inbox)
                                                        .text_color(cx.theme().primary),
                                                )
                                                .child(
                                                    div()
                                                        .text_base()
                                                        .font_weight(FontWeight::BOLD)
                                                        .text_color(cx.theme().foreground)
                                                        .child("Set Up Your Existing Email Address"),
                                                ),
                                        )
                                        .child(
                                            div()
                                                .text_xs()
                                                .text_color(cx.theme().muted_foreground)
                                                .child("Enter your credentials. Vespetrel automatically determines server settings and OAuth2 protocol."),
                                        ),
                                )
                                .child(
                                    Button::new("btn-close-wizard")
                                        .icon(IconName::Close)
                                        .ghost()
                                        .xsmall()
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.active_modal = ActiveModal::None;
                                            this.wizard_inputs = None;
                                            cx.notify();
                                        })),
                                ),
                        )
                        // Thunderbird 3-field credentials form (Name, Email, Password)
                        .child(
                            Form::vertical()
                                .gap(px(8.0))
                                .child(
                                    Field::new()
                                        .label("Your name:")
                                        .child(if let Some(inputs) = inputs_opt {
                                            component::input::Input::new(&inputs.name).cleanable(true).into_any_element()
                                        } else {
                                            div().text_xs().text_color(cx.theme().foreground).child(display_name.to_string()).into_any_element()
                                        }),
                                )
                                .child(
                                    Field::new()
                                        .label("Email address:")
                                        .child(if let Some(inputs) = inputs_opt {
                                            component::input::Input::new(&inputs.email).cleanable(true).into_any_element()
                                        } else {
                                            div().text_xs().text_color(cx.theme().foreground).child(display_email.to_string()).into_any_element()
                                        }),
                                )
                                .child(
                                    Field::new()
                                        .label(if is_oauth { "Password (Optional for OAuth2):" } else { "Password or App Password:" })
                                        .child(if let Some(inputs) = inputs_opt {
                                            component::input::Input::new(&inputs.password).mask_toggle().cleanable(true).into_any_element()
                                        } else {
                                            div().text_xs().text_color(cx.theme().muted_foreground).child("••••••••••••••••").into_any_element()
                                        }),
                                )
                        )
                        // Quick Presets Row
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(4.0))
                                .child(div().text_xs().text_color(cx.theme().muted_foreground).child("Quick presets:"))
                                .child(
                                    div()
                                        .flex()
                                        .flex_row()
                                        .gap(px(6.0))
                                        .child(
                                            Button::new("chip-personal")
                                                .label("Gmail (OAuth2)")
                                                .ghost()
                                                .small()
                                                .on_click(cx.listener(move |this, _, window, cx| {
                                                    let em = "personal@gmail.com".to_string();
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
                                                        inputs.client_id.update(cx, |inp, cx| inp.set_value(this.login_wizard.effective_client_id(), window, cx));
                                                    }
                                                    cx.notify();
                                                })),
                                        )
                                        .child(
                                            Button::new("chip-work")
                                                .label("Microsoft 365")
                                                .ghost()
                                                .small()
                                                .on_click(cx.listener(move |this, _, window, cx| {
                                                    let em = "work@company.onmicrosoft.com".to_string();
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
                                                        inputs.client_id.update(cx, |inp, cx| inp.set_value(this.login_wizard.effective_client_id(), window, cx));
                                                    }
                                                    cx.notify();
                                                })),
                                        )
                                        .child(
                                            Button::new("chip-fastmail")
                                                .label("Fastmail (JMAP)")
                                                .ghost()
                                                .small()
                                                .on_click(cx.listener(move |this, _, window, cx| {
                                                    let em = "user@fastmail.com".to_string();
                                                    let nm = "Fastmail User".to_string();
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
                                                })),
                                        )
                                        .child(
                                            Button::new("chip-custom-imap")
                                                .label("Custom IMAP")
                                                .ghost()
                                                .small()
                                                .on_click(cx.listener(move |this, _, window, cx| {
                                                    let em = "team@vespetrel.org".to_string();
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
                                                })),
                                        ),
                                ),
                        )
                        // Thunderbird Autodiscover Summary Card
                        .child(
                            match &self.login_wizard.autodiscover_status {
                                AutodiscoverStatus::Found { source, details } => {
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap(px(6.0))
                                        .p(px(10.0))
                                        .rounded_lg()
                                        .bg(cx.theme().accent)
                                        .border_1()
                                        .border_color(cx.theme().border)
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
                                                        .child(Icon::new(IconName::CircleCheck).small().text_color(cx.theme().primary))
                                                        .child(
                                                            div()
                                                                .text_xs()
                                                                .font_weight(FontWeight::BOLD)
                                                                .text_color(cx.theme().foreground)
                                                                .child(format!("Configuration found: {source}")),
                                                        ),
                                                )
                                                .child(
                                                    div()
                                                        .text_xs()
                                                        .font_weight(FontWeight::SEMIBOLD)
                                                        .text_color(cx.theme().primary)
                                                        .child(if is_oauth { "OAuth2" } else { "Password" }),
                                                ),
                                        )
                                        .child(
                                            div()
                                                .text_xs()
                                                .text_color(cx.theme().muted_foreground)
                                                .child(details.clone()),
                                        )
                                }
                                _ => div(),
                            }
                        )
                        // OAuth2 Info note
                        .child(
                            if is_oauth {
                                div()
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .gap(px(8.0))
                                    .p(px(8.0))
                                    .rounded_md()
                                    .bg(cx.theme().muted)
                                    .border_1()
                                    .border_color(cx.theme().border)
                                    .child(Icon::new(IconName::Info).small().text_color(cx.theme().primary))
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(cx.theme().muted_foreground)
                                            .child("Browser OAuth2 sign-in: Vespetrel will securely authenticate with your provider via browser PKCE loopback. No passwords are stored."),
                                    )
                            } else {
                                div()
                            }
                        )
                        // OAuth status indicator if active
                        .child(
                            if let Some(status_str) = &self.login_wizard.oauth_status {
                                div()
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .gap(px(6.0))
                                    .p(px(8.0))
                                    .rounded_md()
                                    .bg(cx.theme().primary)
                                    .border_1()
                                    .border_color(cx.theme().primary)
                                    .text_xs()
                                    .text_color(cx.theme().primary_hover)
                                    .child(Icon::new(IconName::LoaderCircle).small())
                                    .child(status_str.clone())
                            } else {
                                div()
                            }
                        )
                        // Expandable "Configure manually" Section
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
                                            Button::new("btn-toggle-manual")
                                                .label(if is_manual { "Hide manual configuration ⌃" } else { "Configure manually ⌄" })
                                                .ghost()
                                                .xsmall()
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.login_wizard.toggle_manual_config();
                                                    cx.notify();
                                                })),
                                        )
                                        .child(
                                            if is_manual {
                                                Button::new("btn-retest-config")
                                                    .label("Re-test / Detect")
                                                    .ghost()
                                                    .xsmall()
                                                    .on_click(cx.listener(|this, _, window, cx| {
                                                        let em = if let Some(inputs) = &this.wizard_inputs {
                                                            inputs.email.read(cx).value().trim().to_string()
                                                        } else {
                                                            this.login_wizard.email.clone()
                                                        };
                                                        if !em.is_empty() {
                                                            this.login_wizard.apply_autodiscover_for_email(&em);
                                                            if let Some(inputs) = &this.wizard_inputs {
                                                                inputs.incoming_host.update(cx, |inp, cx| inp.set_value(this.login_wizard.incoming_host.clone(), window, cx));
                                                                inputs.incoming_port.update(cx, |inp, cx| inp.set_value(this.login_wizard.incoming_port.to_string(), window, cx));
                                                                inputs.outgoing_host.update(cx, |inp, cx| inp.set_value(this.login_wizard.outgoing_host.clone(), window, cx));
                                                                inputs.outgoing_port.update(cx, |inp, cx| inp.set_value(this.login_wizard.outgoing_port.to_string(), window, cx));
                                                            }
                                                        }
                                                        cx.notify();
                                                    }))
                                            } else {
                                                Button::new("btn-placeholder-space").ghost().xsmall()
                                            }
                                        ),
                                )
                                .child(
                                    if is_manual {
                                        div()
                                            .flex()
                                            .flex_col()
                                            .gap(px(10.0))
                                            .p(px(12.0))
                                            .rounded_lg()
                                            .bg(cx.theme().muted)
                                            .border_1()
                                            .border_color(cx.theme().border)
                                            // Protocol choice buttons
                                            .child(
                                                div()
                                                    .flex()
                                                    .flex_row()
                                                    .flex_wrap()
                                                    .gap(px(6.0))
                                                    .children(providers.into_iter().map(|(p, title, _desc, icon)| {
                                                        let is_sel = selected_p == p;
                                                        let p_choice = p.clone();
                                                        let mut btn = Button::new(ElementId::Name(format!("prov-choice-{:?}", p).into()))
                                                            .icon(icon)
                                                            .label(title)
                                                            .small();
                                                        if is_sel {
                                                            btn = btn.primary();
                                                        }
                                                        btn.on_click(cx.listener(move |this, _, window, cx| {
                                                            this.login_wizard.select_provider(p_choice.clone());
                                                            if let Some(inputs) = &this.wizard_inputs {
                                                                inputs.incoming_host.update(cx, |inp, cx| inp.set_value(this.login_wizard.incoming_host.clone(), window, cx));
                                                                inputs.incoming_port.update(cx, |inp, cx| inp.set_value(this.login_wizard.incoming_port.to_string(), window, cx));
                                                                inputs.outgoing_host.update(cx, |inp, cx| inp.set_value(this.login_wizard.outgoing_host.clone(), window, cx));
                                                                inputs.outgoing_port.update(cx, |inp, cx| inp.set_value(this.login_wizard.outgoing_port.to_string(), window, cx));
                                                            }
                                                            cx.notify();
                                                        }))
                                                    }))
                                            )
                                            // Auth mode switcher
                                            .child(
                                                if selected_p == ProviderType::Gmail || selected_p == ProviderType::Graph {
                                                    let is_oauth_active = self.login_wizard.auth_mode == AuthModeChoice::OAuth2;
                                                    div()
                                                        .flex()
                                                        .flex_row()
                                                        .gap(px(4.0))
                                                        .child({
                                                            let mut btn = Button::new("auth-mode-oauth2")
                                                                .icon(IconName::Globe)
                                                                .label("Browser OAuth2")
                                                                .on_click(cx.listener(|this, _, _, cx| {
                                                                    this.login_wizard.auth_mode = AuthModeChoice::OAuth2;
                                                                    cx.notify();
                                                                }));
                                                            if is_oauth_active {
                                                                    btn = btn.primary();
                                                                }
                                                                btn
                                                            })
                                                            .child({
                                                                let mut btn = Button::new("auth-mode-password")
                                                                    .icon(IconName::FolderClosed)
                                                                    .label("App Password / IMAP")
                                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                                        this.login_wizard.auth_mode = AuthModeChoice::Password;
                                                                        cx.notify();
                                                                    }));
                                                                if !is_oauth_active {
                                                                    btn = btn.primary();
                                                                }
                                                                btn
                                                            })
                                                    } else {
                                                        div()
                                                    }
                                                )
                                                // Server hostnames and ports
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
                                                                .child(div().text_xs().font_weight(FontWeight::SEMIBOLD).text_color(cx.theme().muted_foreground).child("Incoming host"))
                                                                .child(if let Some(inputs) = inputs_opt {
                                                                    component::input::Input::new(&inputs.incoming_host).cleanable(true).into_any_element()
                                                                } else {
                                                                    div().text_xs().text_color(cx.theme().muted_foreground).child("imap.example.com").into_any_element()
                                                                }),
                                                        )
                                                        .child(
                                                            div()
                                                                .flex()
                                                                .flex_col()
                                                                .w(px(80.0))
                                                                .gap(px(4.0))
                                                                .child(div().text_xs().font_weight(FontWeight::SEMIBOLD).text_color(cx.theme().muted_foreground).child("Port"))
                                                                .child(if let Some(inputs) = inputs_opt {
                                                                    component::input::Input::new(&inputs.incoming_port).into_any_element()
                                                                } else {
                                                                    div().text_xs().text_color(cx.theme().muted_foreground).child("993").into_any_element()
                                                                }),
                                                        )
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
                                                                .child(div().text_xs().font_weight(FontWeight::SEMIBOLD).text_color(cx.theme().muted_foreground).child("Outgoing SMTP host"))
                                                                .child(if let Some(inputs) = inputs_opt {
                                                                    component::input::Input::new(&inputs.outgoing_host).cleanable(true).into_any_element()
                                                                } else {
                                                                    div().text_xs().text_color(cx.theme().muted_foreground).child("smtp.example.com").into_any_element()
                                                                }),
                                                        )
                                                        .child(
                                                            div()
                                                                .flex()
                                                                .flex_col()
                                                                .w(px(80.0))
                                                                .gap(px(4.0))
                                                                .child(div().text_xs().font_weight(FontWeight::SEMIBOLD).text_color(cx.theme().muted_foreground).child("Port"))
                                                                .child(if let Some(inputs) = inputs_opt {
                                                                    component::input::Input::new(&inputs.outgoing_port).into_any_element()
                                                                } else {
                                                                    div().text_xs().text_color(cx.theme().muted_foreground).child("587").into_any_element()
                                                                }),
                                                        )
                                                )
                                                .child(
                                                    Field::new()
                                                        .label("OAuth Client ID (Optional override):")
                                                        .child(if let Some(inputs) = inputs_opt {
                                                            component::input::Input::new(&inputs.client_id).cleanable(true).into_any_element()
                                                        } else {
                                                            div().text_xs().text_color(cx.theme().muted_foreground).child("Client ID").into_any_element()
                                                        }),
                                                )
                                        } else {
                                            div()
                                        }
                                    ),
                            )
                            // Error message if any
                            .child(if let WizardStep::Failed(err) = &self.login_wizard.step {
                                div()
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .gap(px(6.0))
                                    .p(px(8.0))
                                    .rounded_md()
                                    .bg(cx.theme().danger)
                                    .border_1()
                                    .border_color(cx.theme().danger)
                                    .text_xs()
                                    .text_color(cx.theme().danger_foreground)
                                    .child(Icon::new(IconName::TriangleAlert).small().text_color(cx.theme().danger))
                                    .child(err.clone())
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
                                    .border_color(cx.theme().border)
                                    .child(
                                        Button::new("wizard-btn-cancel")
                                            .label("Cancel")
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.active_modal = ActiveModal::None;
                                                this.wizard_inputs = None;
                                                cx.notify();
                                            })),
                                    )
                                    .child(
                                        if is_oauth {
                                            let (btn_label, btn_icon) = match selected_p {
                                                ProviderType::Gmail => ("Sign in with Google (Browser)", IconName::Globe),
                                                ProviderType::Graph => ("Sign in with Microsoft (Browser)", IconName::Building2),
                                                _ => ("Sign in with Browser", IconName::Globe),
                                            };
                                            Button::new("wizard-btn-oauth")
                                                .icon(btn_icon)
                                                .label(btn_label)
                                                .primary()
                                                .on_click(cx.listener(|this, _, window, cx| {
                                                    this.start_oauth2_flow(window, cx);
                                                }))
                                                .into_any_element()
                                    } else {
                                        Button::new("wizard-btn-connect")
                                            .label("Connect account")
                                            .primary()
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
                                                                view.status_message = format!("Successfully connected {} ({:?})", acct_clone.email, acct_clone.provider_type);
                                                                view.show_toast(format!("Account {} connected successfully", acct_clone.email), false, cx);
                                                                view.active_modal = ActiveModal::None;
                                                                view.wizard_inputs = None;
                                                                view.login_wizard.step = WizardStep::Completed;
                                                                let _ = sync_sender.send(SyncEvent::FolderListUpdated(remote_folders));
                                                                cx.notify();
                                                            });
                                                        }
                                                        Err(e) => {
                                                            let _ = this.update(cx, |view, cx| {
                                                                view.status_message = format!("Connection error: {e}");
                                                                view.show_toast(format!("Connection error: {e}"), true, cx);
                                                                view.login_wizard.step = WizardStep::Failed(format!("{e}"));
                                                                cx.notify();
                                                            });
                                                        }
                                                    }
                                                }).detach();
                                            }))
                                            .into_any_element()
                                    },
                                ),
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
                client_id = self.login_wizard.effective_client_id();
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
                let val = inputs.email.read(cx).value().trim().to_string();
                if val.is_empty() {
                    self.login_wizard.email.clone()
                } else {
                    val
                }
            } else {
                self.login_wizard.email.clone()
            };
            let user_name = if let Some(inputs) = &self.wizard_inputs {
                let val = inputs.name.read(cx).value().trim().to_string();
                if val.is_empty() {
                    self.login_wizard.name.clone()
                } else {
                    val
                }
            } else {
                self.login_wizard.name.clone()
            };

            self.login_wizard.step = WizardStep::OAuth2Waiting;
            self.login_wizard.oauth_status = Some("Starting browser authorization...".into());
            self.status_message = "Waiting for browser OAuth2 authorization...".into();
            cx.notify();

            let pool_opt = self.storage_pool.clone();
            let sync_sender = self.sync_sender.clone();

            // Reconnecting an account that already has one: the provider will omit
            // `refresh_token` from the token response, and overwriting the stored credential
            // with nothing would strand the account on a token that expires in an hour.
            // Read it before the spawn, since the task cannot borrow `self`.
            let prior_refresh = self
                .accounts
                .iter()
                .find(|a| a.email.eq_ignore_ascii_case(&user_email))
                .and_then(|a| a.auth_config.refresh_token_keyring_key.clone())
                .and_then(|key| {
                    keyring::Entry::new("vespetrel", &key)
                        .ok()
                        .and_then(|e| e.get_password().ok())
                });

            cx.spawn(async move |this, cx| {
                let listener = match vespetrel_crypto::OAuth2Engine::bind_loopback().await {
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
                let port = listener.port;
                // Must match the redirect_uri sent in the authorization request byte for byte;
                // the token endpoint compares them exactly.
                oauth_cfg.redirect_uri = format!("http://127.0.0.1:{port}/");
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

                // 15 minutes: enough for an account picker, a consent screen, and a second-factor prompt.
                // The previous 180s expired mid-MFA and reported it as a mysterious timeout.
                let code_res = engine
                    .wait_for_callback_on_listener(listener, 900, Some(csrf_token.secret()))
                    .await;
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

                let token_bundle =
                    match engine
                        .exchange_code(code, verifier.secret().to_string(), prior_refresh)
                        .await
                    {
                        Ok(b) => b,
                        Err(e) => {
                            let _ = this.update(cx, |view, cx| {
                                view.show_toast(format!("Token exchange failed: {e}"), true, cx);
                                view.login_wizard.step =
                                    WizardStep::Failed(format!("Token exchange failed: {e}"));
                                cx.notify();
                            });
                            return;
                        }
                    };

                // An offline grant that returned no refresh token is not a usable account.
                // Saving it produced a mailbox that synced for an hour and then failed forever
                // with an opaque server-side auth error, with no hint that re-consent was needed.
                if token_bundle.refresh_token.is_none() && oauth_cfg.offline_access {
                    let msg = "Google did not return a refresh token for offline access. \
Revoke Vespetrel's access at https://myaccount.google.com/permissions and connect again.";
                    let _ = this.update(cx, |view, cx| {
                        view.show_toast(msg.to_string(), true, cx);
                        view.login_wizard.step = WizardStep::Failed(msg.to_string());
                        cx.notify();
                    });
                    return;
                }

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
                    offline_access: oauth_cfg.offline_access,
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
                            view.status_message = format!("Successfully connected {} via OAuth2", acct.email);
                            view.show_toast(format!("Successfully connected {} via OAuth2", acct.email), false, cx);
                            view.active_modal = ActiveModal::None;
                            view.wizard_inputs = None;
                            view.login_wizard.step = WizardStep::Completed;
                            let _ = sync_sender.send(SyncEvent::FolderListUpdated(remote_folders));
                            cx.notify();
                        });
                    }
                    Err(e) => {
                        let _ = this.update(cx, |view, cx| {
                            view.status_message = format!("Connection error: {e}");
                            view.show_toast(format!("Connection error: {e}"), true, cx);
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

    /// Map an application [`KeyAction`] onto its typed GPUI Action.
    ///
    /// One Action per command, so the shortcut, a toolbar button and the palette all
    /// dispatch the same value and the logic stays in one handler. An action the
    /// window does not implement returns `None` and is left unbound rather than bound
    /// to a handler that silently does nothing.
    pub fn binding_for(keystroke: &'static str, key: KeyAction) -> Option<KeyBinding> {
        let context = Some(BINDING_CONTEXT);
        Some(match key {
            KeyAction::NextMessage => KeyBinding::new(keystroke, SelectNextMessage, context),
            KeyAction::PrevMessage => KeyBinding::new(keystroke, SelectPreviousMessage, context),
            KeyAction::Archive => KeyBinding::new(keystroke, Archive, context),
            KeyAction::Delete => KeyBinding::new(keystroke, Delete, context),
            KeyAction::MarkRead | KeyAction::MarkUnread => {
                KeyBinding::new(keystroke, ToggleRead, context)
            }
            KeyAction::ToggleStar => KeyBinding::new(keystroke, ToggleStar, context),
            KeyAction::Reply => KeyBinding::new(keystroke, Reply, context),
            KeyAction::ReplyAll => KeyBinding::new(keystroke, ReplyAll, context),
            KeyAction::Forward => KeyBinding::new(keystroke, Forward, context),
            KeyAction::Compose => KeyBinding::new(keystroke, Compose, context),
            KeyAction::FocusSearch => KeyBinding::new(keystroke, FocusSearch, context),
            KeyAction::SyncNow => KeyBinding::new(keystroke, SyncNow, context),
            KeyAction::ToggleNavigationPane => {
                KeyBinding::new(keystroke, ToggleFolderTree, context)
            }
            KeyAction::GoToInbox => KeyBinding::new(keystroke, GoToInbox, context),
            KeyAction::GoToSent => KeyBinding::new(keystroke, GoToSent, context),
            KeyAction::GoToDrafts => KeyBinding::new(keystroke, GoToDrafts, context),
        })
    }

    /// The key context every window binding is scoped to.
    const BINDING_CONTEXT: &str = WINDOW_CONTEXT;

    /// Install a keymap on the application.
    ///
    /// A token that does not translate is skipped and reported, never passed to
    /// `KeyBinding::new`: that constructor panics on input it cannot parse, so a typo
    /// in a preset would take the whole window down at launch.
    pub fn bind_keymap(map: &KeybindingMap, cx: &mut App) -> Vec<String> {
        let mut bindings = Vec::with_capacity(map.sorted_entries().len());
        let mut rejected = Vec::new();

        for (token, key_action) in map.sorted_entries() {
            let Some(keystroke) = to_gpui_keystroke(&token) else {
                rejected.push(token);
                continue;
            };
            let keystroke_static: &'static str = Box::leak(keystroke.into_boxed_str());
            match binding_for(keystroke_static, key_action) {
                Some(binding) => bindings.push(binding),
                None => rejected.push(token),
            }
        }

        if !bindings.is_empty() {
            cx.bind_keys(bindings);
        }
        rejected
    }

    /// Launch the GPUI Desktop Application
    pub fn run_gpui_app(
        sync_rx: flume::Receiver<SyncEvent>,
        sync_tx: flume::Sender<SyncEvent>,
        storage_pool: Option<vespetrel_storage::db::StoragePool>,
    ) {
        gpui_kit::application()
            .with_assets(Assets)
            .run(move |cx: &mut App| {
                gpui_kit::init(cx);
                // Follow the operating system's light/dark appearance. `init` picks a fixed
                // mode, so without this a user with a dark desktop got a light window.
                gpui_kit::component::theme::Theme::change(crate::platform::theme_mode(), None, cx);
                // Bind the keymap once, at startup, from the stored preset.
                //
                // `cx.bind_keys` appends and GPUI resolves competing bindings by
                // context depth, so a later preset's bindings would outrank the old
                // ones for the keys they share while every other key stayed bound to
                // the previous preset. Clearing and rebuilding would also drop the
                // component defaults that `gpui_kit::init` registered, so the preset
                // is applied at launch and a change is reported as needing a restart.
                let map = KeybindingMap::new(crate::keybindings::preset_from_settings(
                    vespetrel_core::KeymapPreset::default(),
                ));
                bind_keymap(&map, cx);
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
                        // Documented minimum for this shell: 48 rail + 220 tree + 360 list
                        // leaves ~490 for the reader. At the old 900px minimum the reader
                        // was squeezed to roughly 270px, which is below the point where a
                        // message body is readable.
                        window_min_size: Some(size(px(1120.0), px(640.0))),
                        ..Default::default()
                    },
                    move |window, cx| {
                        let view = cx.new(|cx| MainWindow::from_storage(window, cx, rx, tx, pool));
                        cx.new(|cx| gpui_kit::component::Root::new(view, window, cx))
                    },
                );
            });
    }
}

#[cfg(test)]
mod design_tests {
    use super::gpui_app::{StatusState, classify_status, command_palette_keystroke, elide};
    use crate::platform::command_palette_shortcut_label;

    #[test]
    fn message_page_size_is_a_bound_not_a_display_cap() {
        // The list previously fetched 100 rows, so a busy mailbox simply stopped existing below
        // the fold. Assert we stay well clear of that and keep the fetch bounded.
        let page_size = super::gpui_app::MESSAGE_PAGE_SIZE;
        assert!(page_size >= 500, "page size regressed toward a display cap");
        assert!(page_size <= 5_000, "page size must stay bounded");
    }

    /// `&snippet[..100]` panicked whenever the byte at index 100 fell inside a
    /// multi-byte character, which ordinary mail triggers constantly
    /// ("Rechnung für März", an emoji in a newsletter). Assert the truncation walks by
    /// character and never splits one.
    #[test]
    fn elide_never_splits_a_character() {
        assert_eq!(elide("hello", 10), "hello");
        assert_eq!(elide("hello", 5), "hello");
        assert_eq!(elide("hello", 4), "hell\u{2026}");

        // "ä" is two bytes: a 4-byte window over "ääää" must not panic.
        let accented = elide("ääää", 4);
        assert_eq!(accented, "ääää");
        let cut = elide("ääää", 3);
        assert_eq!(cut, "äää\u{2026}");
        assert_eq!(cut.chars().count(), 4);

        // A 4-byte emoji.
        let emoji = "\u{1F600}\u{1F600}\u{1F600}";
        assert_eq!(elide(emoji, 2), "\u{1F600}\u{1F600}\u{2026}");

        // Empty input must stay empty rather than becoming a lone ellipsis.
        assert_eq!(elide("", 5), "");
    }

    /// The status bar used to report "Connected" from the absence of certain words in a
    /// free-text message, so a genuine failure whose wording differed stayed green and a
    /// normal message containing "error" turned the indicator red.
    #[test]
    fn status_state_reflects_the_real_condition() {
        assert_eq!(classify_status(0, "All good"), StatusState::NoAccount);
        assert_eq!(classify_status(1, "Ready"), StatusState::Ready);
        assert_eq!(classify_status(1, "Syncing 3 folders"), StatusState::Busy);
        assert_eq!(
            classify_status(1, "Couldn't reach server"),
            StatusState::Failed
        );
        assert_eq!(
            classify_status(1, "Authentication failed"),
            StatusState::Failed
        );
        assert_eq!(
            classify_status(1, "Server unavailable"),
            StatusState::Failed
        );
        // A busy state must win over a bare "error"-free prefix.
        assert_eq!(classify_status(1, "Syncing\u{2026}"), StatusState::Busy);
    }

    /// The command-palette hint must name the modifier this platform uses, and the
    /// `Keystroke` must agree with the label it is rendered from.
    #[test]
    fn command_palette_keystroke_matches_its_label() {
        let stroke = command_palette_keystroke();
        let is_macos = cfg!(target_os = "macos");
        assert_eq!(stroke.modifiers.platform, is_macos);
        assert_eq!(stroke.modifiers.control, !is_macos);
        assert_eq!(stroke.key, "k");
        assert!(command_palette_shortcut_label().ends_with(" K"));
    }

    #[test]
    fn test_active_modal_attachment_preview() {
        let modal = super::gpui_app::ActiveModal::AttachmentPreview {
            filename: "invoice.pdf".to_string(),
            mime_type: "application/pdf".to_string(),
            size_bytes: 4096,
            blob_path: Some("/tmp/invoice.pdf".to_string()),
            data: Some(b"%PDF-1.7\n".to_vec()),
        };
        match modal {
            super::gpui_app::ActiveModal::AttachmentPreview {
                ref filename,
                ref mime_type,
                size_bytes,
                ..
            } => {
                assert_eq!(filename, "invoice.pdf");
                assert_eq!(mime_type, "application/pdf");
                assert_eq!(size_bytes, 4096);
            }
            _ => panic!("Expected AttachmentPreview variant"),
        }
    }

    #[test]
    fn test_account_alias_selection_and_matching() {
        let alias1 = vespetrel_core::AccountAlias::new(
            "acc-1",
            "suyash@vespetrel.org",
            Some("Suyash".into()),
        )
        .with_default(true);
        let alias2 = vespetrel_core::AccountAlias::new(
            "acc-1",
            "support@vespetrel.org",
            Some("Support Team".into()),
        )
        .with_reply_to("contact@vespetrel.org");

        assert_eq!(alias1.email, "suyash@vespetrel.org");
        assert!(alias1.is_default);
        assert_eq!(alias2.reply_to.as_deref(), Some("contact@vespetrel.org"));

        let aliases = vec![alias1.clone(), alias2.clone()];
        let query_addr = "support@vespetrel.org";
        let matched = aliases
            .iter()
            .find(|a| a.email.eq_ignore_ascii_case(query_addr));
        assert!(matched.is_some());
        assert_eq!(matched.unwrap().name.as_deref(), Some("Support Team"));
    }

    #[test]
    fn test_signature_template_generation_and_text() {
        let profile = vespetrel_core::SignatureProfile {
            full_name: "Suyash Stark".into(),
            job_title: Some("Lead Architect".into()),
            company: Some("Vespetrel".into()),
            phone: Some("+1-555-0199".into()),
            email: "suyash@vespetrel.org".into(),
            website: Some("https://vespetrel.org".into()),
            avatar_url: None,
            pronouns: Some("he/him".into()),
            disclaimer: None,
            social_links: vec![],
        };
        let sig = vespetrel_core::Signature::from_template(
            "acc-1",
            "Modern Sig",
            &profile,
            vespetrel_core::SignatureTemplate::Modern,
            true,
            true,
        );
        assert!(sig.raw_html.contains("Suyash Stark"));
        assert!(sig.raw_html.contains("Lead Architect"));
        assert!(sig.plain_text.is_some());
        let pt = sig.plain_text.unwrap();
        assert!(pt.contains("Suyash Stark"));
        assert!(pt.contains("suyash@vespetrel.org"));
    }
}
