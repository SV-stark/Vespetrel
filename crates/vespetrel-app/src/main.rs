use tracing_subscriber::EnvFilter;

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

#[tokio::main]

async fn main() -> anyhow::Result<()> {
    // Safe logger init (ignores error if already initialized)
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::from_default_env()
                .add_directive("vespetrel=info".parse().unwrap_or_default()),
        )
        .try_init()
        .ok();

    let args: Vec<String> = std::env::args().collect();
    if args.contains(&"--help".to_string()) || args.contains(&"-h".to_string()) {
        println!("Vespetrel - Pure Rust gpui Mail Client v0.1.0");
        println!("\nUsage: vespetrel [OPTIONS]");
        println!("\nOptions:");
        println!("  --db <PATH>    Path to SQLite database (default: ./vespetrel.db)");
        println!("  --memory       Use in-memory SQLite database");
        println!("  --version      Print version information");
        println!("  --help         Print this help message");
        return Ok(());
    }

    if args.contains(&"--version".to_string()) || args.contains(&"-v".to_string()) {
        println!("vespetrel 0.1.0 (pure rust mail client)");
        return Ok(());
    }

    let db_path = if args.contains(&"--memory".to_string()) {
        ":memory:".to_string()
    } else if let Some(pos) = args.iter().position(|a| a == "--db") {
        args.get(pos + 1)
            .cloned()
            .unwrap_or_else(|| "./vespetrel.db".to_string())
    } else {
        "./vespetrel.db".to_string()
    };

    println!("Vespetrel - Pure Rust gpui Mail Client v0.1.0");
    println!("Storage: SQLite WAL + FTS5 | Engine: Tokio Sync | Render: ammonia+lol_html");

    // Initialize platform hardening (High-DPI, OS Theme)
    vespetrel_app::platform::init_platform();

    // Initialize storage schema

    let app = vespetrel_app::app::VespetrelApp::new(&db_path);
    if let Err(e) = app.init_storage().await {
        eprintln!("Failed to initialize database storage: {e}");
        return Err(e);
    }
    println!(
        "✓ Storage schema OK ({} PRAGMAs applied, target: {})",
        vespetrel_storage::db::PRAGMAS.len(),
        db_path
    );

    // Demonstrate sync coordinator + tokio bridge
    let (coordinator, mut rx) = vespetrel_engine::SyncCoordinator::create();
    println!("✓ SyncCoordinator created - event channel ready");

    // Spawn a sync event producer to show bridge
    tokio::spawn(async move {
        use chrono::Utc;
        use vespetrel_core::MessageSummary;
        use vespetrel_core::provider::SyncEvent;
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
        let _ = coordinator
            .event_sender()
            .send(SyncEvent::MessagesInserted(msgs));
    });

    // Consume one event to prove tokio->GPUI pattern works
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
        println!(
            "Tip: build with --features gpui to launch GPU window (requires enabling gpui git deps in Cargo.toml)."
        );
        println!("Startup check passed successfully.");
    }

    Ok(())
}
