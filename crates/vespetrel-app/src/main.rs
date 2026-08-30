use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("info".parse().unwrap()))
        .init();

    println!("Vespetrel - Pure Rust gpui Mail Client v0.1.0");
    println!("Storage: SQLite WAL + FTS5 | Engine: Tokio Sync | Render: ammonia+lol_html");

    // Headless quick-start check: verify storage schema without UI
    let app = vespetrel_app::app::VespetrelApp::new("./vespetrel.db");
    app.init_storage().await?;
    println!("✓ Storage schema OK ({} PRAGMAs applied)", vespetrel_storage::db::PRAGMAS.len());

    // Demonstrate sync coordinator + tokio bridge
    let (coordinator, mut rx) = vespetrel_engine::SyncCoordinator::create();
    println!("✓ SyncCoordinator created - event channel ready");

    // Spawn a dummy sync event producer to show bridge (would be real ImapProvider in prod)
    tokio::spawn(async move {
        // Simulate incoming sync events
        use vespetrel_core::provider::SyncEvent;
        use vespetrel_core::MessageSummary;
        use chrono::Utc;
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        let msgs = vec![MessageSummary {
            id: uuid::Uuid::new_v4().to_string(),
            thread_id: None,
            subject: Some("Welcome to Vespetrel".into()),
            from_address: "welcome@vespetrel.example".into(),
            from_name: Some("Vespetrel Team".into()),
            snippet: Some("Your Rust-native Thunderbird-parity client is ready.".into()),
            sent_at: Utc::now(),
            is_read: false,
            is_flagged: false,
            has_attachments: false,
        }];
        let _ = coordinator.event_sender().send(SyncEvent::MessagesInserted(msgs));
    });

    // Headless consume one event to prove tokio->GPUI pattern works
    if let Some(ev) = rx.recv().await {
        println!("✓ SyncEvent received: {ev:?}");
    }

    #[cfg(feature = "gpui")]
    {
        println!("Starting GPUI window (feature `gpui` enabled via wry) ...");
        println!("(gpui window stub - add gpui git deps to Cargo.toml to enable App::run)");
    }

    #[cfg(not(feature = "gpui"))]
    {
        println!("Tip: build with --features gpui to launch GPU window (requires enabling gpui git deps in Cargo.toml).");
        println!("Headless check passed. Exiting.");
    }

    Ok(())
}
