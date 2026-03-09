use std::path::PathBuf;
use std::time::Duration;

use color_eyre::eyre::{eyre, Result};
use crossterm::event::{self, Event, KeyCode};
use futures_util::StreamExt;
use secrecy::{ExposeSecret, SecretString};
use tokio::sync::mpsc;

use discordinator::app::App;
use discordinator::config::{AppConfig, AppDirs};
use discordinator::domain::event::GatewayEvent;
use discordinator::domain::types::{
    BackgroundResult, ConnectionState, DbRequest, GatewayCommand, HttpRequest,
};
use discordinator::infrastructure::discord_properties;
use discordinator::infrastructure::gateway::GatewayManager;
use discordinator::infrastructure::http_client::HttpActor;
use discordinator::infrastructure::keyring::KeyringStore;
use discordinator::infrastructure::science::ScienceTracker;
use discordinator::input::mode::InputMode;
use discordinator::ui::login::{LoginMethod, LoginScreen, LoginState, LoginStatus};

type Term = ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>>;

const DISCORD_API_BASE: &str = "https://discord.com/api/v10";

#[tokio::main]
async fn main() -> Result<()> {
    // Handle --check-properties flag before any TUI setup
    if std::env::args().any(|a| a == "--check-properties") {
        return check_properties_cli().await;
    }

    discordinator::logging::install_panic_handler()?;
    let dirs = AppDirs::new()?;
    let config = discordinator::config::load_or_create_config(&dirs.config_file())?;
    let _log_guard = discordinator::logging::init_logging(&dirs.log_dir())?;

    tracing::info!("Discordinator starting up");

    let mut terminal = App::setup_terminal()?;
    let result = run(&mut terminal, config).await;
    App::restore_terminal(&mut terminal)?;

    result
}

/// CLI mode: fetch current Discord properties and compare against config defaults.
async fn check_properties_cli() -> Result<()> {
    let config = discordinator::config::DiscordConfig::default();
    println!("Fetching current Discord client properties...");

    match discord_properties::fetch_discord_properties(&config.browser_user_agent).await {
        Ok(props) => {
            println!("\n  Current Discord properties:");
            println!("    Build number:     {}", props.client_build_number);
            println!("    Capabilities:     {}", props.capabilities);
            println!("    Browser version:  {}", props.browser_version);
            println!("\n  Compiled-in defaults:");
            println!("    Build number:     {}", config.client_build_number);
            println!("    Capabilities:     {}", config.capabilities);
            println!("    Browser version:  {}", config.browser_version);

            let mut stale = false;
            if props.client_build_number != config.client_build_number {
                println!(
                    "\n  WARNING: Build number is stale ({} vs {})",
                    config.client_build_number, props.client_build_number
                );
                stale = true;
            }
            if props.capabilities != config.capabilities {
                println!(
                    "\n  WARNING: Capabilities bitmask is stale ({} vs {})",
                    config.capabilities, props.capabilities
                );
                stale = true;
            }
            if !stale {
                println!("\n  All properties are up to date.");
            }
            println!("\n  Note: Auto-fetch at startup will use the live values regardless.");
        }
        Err(e) => {
            println!("  Failed to fetch properties: {e}");
            println!("  The app will fall back to compiled-in defaults.");
        }
    }
    Ok(())
}

/// What `run_app` decided when it returned.
enum RunResult {
    /// User pressed Ctrl+Q — exit the application.
    Quit,
    /// Connection timed out or user pressed Esc — go back to login.
    ReturnToLogin,
}

async fn run(terminal: &mut Term, mut config: AppConfig) -> Result<()> {
    // Auto-fetch Discord client properties (build number, capabilities, browser version)
    let dirs = AppDirs::new()?;
    let cache_path = dirs.cache_dir.join("discord_properties.json");
    match fetch_or_load_properties(&cache_path, &config.discord.browser_user_agent).await {
        Some(props) => {
            tracing::info!(
                build_number = props.client_build_number,
                capabilities = props.capabilities,
                browser_version = %props.browser_version,
                "Using auto-fetched Discord properties"
            );
            config.discord.apply_fetched_properties(&props);
        }
        None => {
            tracing::warn!("Could not fetch Discord properties, using config defaults");
        }
    }

    let keyring = KeyringStore;
    let env_getter = |key: &str| -> Option<String> { std::env::var(key).ok() };

    let mut token = match discordinator::auth::retrieve_token(&config.auth, &keyring, &env_getter)?
    {
        Some(t) => t,
        None => match login_loop(terminal, &config, &keyring).await? {
            Some(t) => SecretString::from(t),
            None => return Ok(()),
        },
    };

    loop {
        match run_app(terminal, config.clone(), token.clone()).await? {
            RunResult::Quit => return Ok(()),
            RunResult::ReturnToLogin => match login_loop(terminal, &config, &keyring).await? {
                Some(t) => token = SecretString::from(t),
                None => return Ok(()),
            },
        }
    }
}

// === Async Event Loop ===

#[allow(clippy::too_many_lines)]
async fn run_app(terminal: &mut Term, config: AppConfig, token: SecretString) -> Result<RunResult> {
    let dirs = AppDirs::new()?;

    // Create channels
    let (gateway_tx, mut gateway_rx) = mpsc::channel::<GatewayEvent>(256);
    let (gw_cmd_tx, gw_cmd_rx) = mpsc::channel::<GatewayCommand>(64);
    let (http_tx, http_rx) = mpsc::channel::<HttpRequest>(64);
    let (db_tx, db_rx) = mpsc::channel::<DbRequest>(64);
    let (bg_tx, mut bg_rx) = mpsc::channel::<BackgroundResult>(64);

    // Spawn gateway manager
    let gw_token = token.clone();
    let gw_config = config.discord.clone();
    tokio::spawn(async move {
        let mut manager = GatewayManager::new(gw_token, gw_config, gateway_tx, gw_cmd_rx);
        if let Err(e) = manager.run().await {
            tracing::error!("Gateway manager error: {}", e);
        }
    });

    // Spawn HTTP actor
    let http_bg_tx = bg_tx.clone();
    let http_actor = HttpActor::new(&config.discord, token.expose_secret(), http_rx, http_bg_tx)?;
    tokio::spawn(async move { http_actor.run().await });

    // Spawn DB worker on a dedicated thread (rusqlite::Connection is !Send)
    let db_path = dirs.data_dir.join("messages.db");
    spawn_db_worker(db_path, db_rx, bg_tx);

    // App state
    let mut app = App::new(config);
    app.state.connection = ConnectionState::Connecting;

    // Async terminal event stream
    let mut reader = crossterm::event::EventStream::new();

    // Render tick (from config, default 60fps = 16ms)
    let render_ms = if app.state.config.general.render_fps > 0 {
        1000 / u64::from(app.state.config.general.render_fps)
    } else {
        16
    };
    let mut render_interval = tokio::time::interval(Duration::from_millis(render_ms));
    render_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    // Science/telemetry tracker for anti-detection
    let mut science = ScienceTracker::new();
    let mut science_interval = tokio::time::interval_at(
        tokio::time::Instant::now() + Duration::from_secs(30),
        Duration::from_secs(30),
    );
    science_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    // Periodic re-subscription for all open panes (prevents Discord from dropping
    // idle lazy guild subscriptions for non-focused panes)
    let mut resub_interval = tokio::time::interval(Duration::from_secs(30));
    resub_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    // Connection timeout: if we don't reach Connected within 30s, bail out
    let connect_deadline = tokio::time::sleep(Duration::from_secs(30));
    tokio::pin!(connect_deadline);
    let mut connected = false;

    // Idle presence timer: after 5 minutes of no input, transition to idle
    let idle_timeout = Duration::from_secs(300);
    let idle_timer = tokio::time::sleep(idle_timeout);
    tokio::pin!(idle_timer);
    let mut is_idle = false;

    // Load session on startup if configured
    if app.state.config.session.restore_on_start {
        let _ = db_tx.try_send(DbRequest::LoadSession {
            name: "default".to_string(),
        });
    }

    loop {
        tokio::select! {
            biased;

            Some(event) = gateway_rx.recv() => {
                app.dirty |= discordinator::event_handler::handle_gateway_event(
                    event, &mut app.state, &db_tx,
                );
                if !connected && matches!(app.state.connection, ConnectionState::Connected { .. }) {
                    connected = true;
                    // Send startup requests that mimic real client behavior after READY
                    let _ = http_tx.try_send(HttpRequest::StartupRequests);
                    science.track_app_opened();
                }
            }

            Some(result) = bg_rx.recv() => {
                app.dirty |= discordinator::event_handler::handle_background_result(
                    result, &mut app.state,
                );

                // After session restore, fetch messages for all panes that have channels
                if app.state.session_just_restored {
                    app.state.session_just_restored = false;
                    let pane_ids = app.state.pane_manager.root.leaves_in_order();
                    for pid in pane_ids {
                        if let Some(pane) = app.state.pane_manager.root.find(pid) {
                            if let Some(ch_id) = pane.channel_id {
                                let _ = db_tx.try_send(DbRequest::FetchMessages {
                                    channel_id: ch_id,
                                    before_timestamp: None,
                                    limit: 50,
                                });
                                let _ = http_tx.try_send(HttpRequest::FetchMessages {
                                    channel_id: ch_id,
                                    before: None,
                                    limit: 50,
                                });
                            }
                        }
                    }
                    subscribe_all_panes(&app.state, &gw_cmd_tx);
                }
            }

            maybe_event = reader.next() => {
                if let Some(Ok(event)) = maybe_event {
                    match event {
                        Event::Key(key) => {
                            // Allow Esc to return to login during connection phase
                            if !connected && key.code == KeyCode::Esc {
                                return Ok(RunResult::ReturnToLogin);
                            }
                            app.dirty |= handle_key_with_dispatch(
                                &mut app, key, &http_tx, &db_tx, &gw_cmd_tx,
                                &mut science,
                            );
                            // Reset idle timer on any key input
                            idle_timer.as_mut().reset(tokio::time::Instant::now() + idle_timeout);
                            if is_idle {
                                is_idle = false;
                                let _ = gw_cmd_tx.try_send(GatewayCommand::UpdatePresence {
                                    status: "online".to_string(),
                                    afk: false,
                                });
                            }
                        }
                        Event::Resize(_, _) => app.dirty = true,
                        _ => {}
                    }
                }
            }

            () = &mut connect_deadline, if !connected => {
                return Ok(RunResult::ReturnToLogin);
            }

            () = &mut idle_timer, if !is_idle => {
                is_idle = true;
                let _ = gw_cmd_tx.try_send(GatewayCommand::UpdatePresence {
                    status: "idle".to_string(),
                    afk: true,
                });
            }

            _ = science_interval.tick() => {
                if let Some(batch) = science.drain_batch() {
                    let _ = http_tx.try_send(HttpRequest::ScienceBatch { payload: batch });
                }
            }

            _ = resub_interval.tick() => {
                subscribe_all_panes(&app.state, &gw_cmd_tx);
            }

            _ = render_interval.tick() => {
                if app.dirty {
                    terminal.draw(|f| {
                        discordinator::ui::layout::render(
                            f.area(), f.buffer_mut(), &app.state,
                        );
                    })?;
                    app.dirty = false;
                }
            }
        }

        if app.should_quit {
            // Save session before quitting
            if app.state.config.session.auto_save {
                if let Ok(json) = app.state.pane_manager.to_session_json() {
                    let _ = db_tx.try_send(DbRequest::SaveSession {
                        name: "default".to_string(),
                        layout_json: json,
                    });
                }
            }
            return Ok(RunResult::Quit);
        }
    }
}

/// Handle a key event and dispatch side effects (HTTP/DB requests).
fn handle_key_with_dispatch(
    app: &mut App,
    key: crossterm::event::KeyEvent,
    http_tx: &mpsc::Sender<HttpRequest>,
    db_tx: &mpsc::Sender<DbRequest>,
    gw_cmd_tx: &mpsc::Sender<GatewayCommand>,
    science: &mut ScienceTracker,
) -> bool {
    // Capture pre-action state for dispatch
    let pre_channel = app.state.focused_pane().channel_id;
    let pre_confirming = app.state.focused_pane().confirming_delete;

    // For insert mode Enter: capture message data before apply_action clears it
    let send_info = if app.state.input_mode == InputMode::Insert && key.code == KeyCode::Enter {
        let pane = app.state.focused_pane();
        pane.channel_id.and_then(|ch_id| {
            let content = pane.input.content.clone();
            if content.is_empty() {
                return None;
            }
            let reply_to = pane.input.reply_to;
            let editing = pane.input.editing;
            Some((ch_id, content, reply_to, editing))
        })
    } else {
        None
    };

    let dirty = app.handle_terminal_event(key);

    // === Dispatch side effects ===

    // SendMessage / EditMessage
    if let Some((channel_id, content, reply_to, editing)) = send_info {
        if let Some(msg_id) = editing {
            let _ = http_tx.try_send(HttpRequest::EditMessage {
                channel_id,
                message_id: msg_id,
                content,
            });
        } else {
            let _ = http_tx.try_send(HttpRequest::SendMessage {
                channel_id,
                content,
                nonce: generate_nonce(),
                reply_to,
            });
        }
    }

    // ConfirmDelete → dispatch HTTP delete
    if let Some(msg_id) = pre_confirming {
        if app.state.focused_pane().confirming_delete.is_none() {
            if let Some(ch_id) = pre_channel {
                let _ = http_tx.try_send(HttpRequest::DeleteMessage {
                    channel_id: ch_id,
                    message_id: msg_id,
                });
            }
        }
    }

    // SwitchChannel → fetch messages + subscribe to gateway events + ack latest message
    let post_channel = app.state.focused_pane().channel_id;
    if post_channel != pre_channel {
        if let Some(ch_id) = post_channel {
            let _ = db_tx.try_send(DbRequest::FetchMessages {
                channel_id: ch_id,
                before_timestamp: None,
                limit: 50,
            });
            let _ = http_tx.try_send(HttpRequest::FetchMessages {
                channel_id: ch_id,
                before: None,
                limit: 50,
            });
            // Ack the latest message in the channel (read state)
            if let Some(last_msg) = app.state.cache.last_message_id(ch_id) {
                let _ = http_tx.try_send(HttpRequest::AckMessage {
                    channel_id: ch_id,
                    message_id: last_msg,
                });
            }
            // Track channel opened for science/telemetry
            let guild_id = app
                .state
                .focused_pane()
                .guild_id
                .map(twilight_model::id::Id::get);
            science.track_channel_opened(ch_id.get(), guild_id);
        }

        // Re-subscribe ALL open panes' guild/channels via op 14, not just the focused one.
        // This ensures non-focused panes keep receiving MESSAGE_CREATE events.
        subscribe_all_panes(&app.state, gw_cmd_tx);
    }

    dirty
}

/// Send op 14 (Lazy Request) for all unique guild/channel pairs across all open panes.
/// This keeps subscriptions active for every visible channel, not just the focused one.
fn subscribe_all_panes(
    state: &discordinator::app::AppState,
    gw_cmd_tx: &mpsc::Sender<GatewayCommand>,
) {
    let subs = state.pane_manager.root.active_guild_channels();
    for (guild_id, channels) in subs {
        let _ = gw_cmd_tx.try_send(GatewayCommand::Subscribe { guild_id, channels });
    }
}

/// Generate a Discord Snowflake-format nonce.
/// Discord epoch is 2015-01-01. Format: `(timestamp_ms - discord_epoch) << 22 | increment`.
/// The low 22 bits contain a random value to mimic real client worker/increment fields.
fn generate_nonce() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    const DISCORD_EPOCH: u128 = 1_420_070_400_000;
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let low_bits = u128::from(rand::random::<u32>() & 0x003F_FFFF);
    let snowflake = (now_ms.saturating_sub(DISCORD_EPOCH)) << 22 | low_bits;
    snowflake.to_string()
}

// === DB Worker ===

/// Spawn a dedicated OS thread for `SQLite` operations.
/// Uses `blocking_recv` / `blocking_send` since `rusqlite::Connection` is !Send.
fn spawn_db_worker(
    db_path: PathBuf,
    mut rx: mpsc::Receiver<DbRequest>,
    bg_tx: mpsc::Sender<BackgroundResult>,
) {
    std::thread::spawn(move || {
        let mut conn = match discordinator::infrastructure::db::open(&db_path) {
            Ok(c) => c,
            Err(e) => {
                tracing::error!("Failed to open database: {}", e);
                return;
            }
        };

        while let Some(request) = rx.blocking_recv() {
            handle_db_request(&mut conn, request, &bg_tx);
        }

        tracing::info!("DB worker shutting down");
    });
}

fn handle_db_request(
    conn: &mut rusqlite::Connection,
    request: DbRequest,
    bg_tx: &mpsc::Sender<BackgroundResult>,
) {
    use discordinator::infrastructure::db;

    let result = match request {
        DbRequest::InsertMessage(msg) => db::insert_message(conn, &msg).map(|()| None),
        DbRequest::InsertMessages(msgs) => db::insert_messages(conn, &msgs).map(|()| None),
        DbRequest::UpdateMessage {
            id,
            content,
            edited_timestamp,
        } => db::update_message(conn, id, &content, &edited_timestamp).map(|_| None),
        DbRequest::DeleteMessage(id) => db::delete_message(conn, id).map(|_| None),
        DbRequest::FetchMessages {
            channel_id,
            before_timestamp,
            limit,
        } => db::fetch_messages(conn, channel_id, before_timestamp.as_deref(), limit).map(|msgs| {
            Some(BackgroundResult::CachedMessages {
                channel_id,
                messages: msgs,
            })
        }),
        DbRequest::SaveSession { name, layout_json } => {
            db::save_session(conn, &name, &layout_json).map(|()| None)
        }
        DbRequest::LoadSession { name } => db::load_session(conn, &name)
            .map(|layout_json| Some(BackgroundResult::SessionLoaded { name, layout_json })),
    };

    match result {
        Ok(Some(bg_result)) => {
            let _ = bg_tx.blocking_send(bg_result);
        }
        Ok(None) => {}
        Err(e) => {
            let _ = bg_tx.blocking_send(BackgroundResult::DbError {
                operation: "db_request".to_string(),
                error: e.to_string(),
            });
        }
    }
}

// === Login Flow ===

/// Run the login UI. Returns `Some(token)` on success, `None` if cancelled.
async fn login_loop(
    terminal: &mut Term,
    config: &AppConfig,
    keyring: &KeyringStore,
) -> Result<Option<String>> {
    let mut state = LoginState::default();
    let mut qr_result_rx: Option<mpsc::Receiver<Result<String>>> = None;
    let mut qr_lines: Option<Vec<String>> = None;

    loop {
        terminal.draw(|f| {
            let mut screen = LoginScreen::new(&state);
            if let Some(ref lines) = qr_lines {
                screen = screen.with_qr_lines(lines);
            }
            f.render_widget(screen, f.area());
        })?;

        // Check QR auth result if active
        if let Some(ref mut rx) = qr_result_rx {
            match rx.try_recv() {
                Ok(Ok(token)) => {
                    discordinator::auth::store_token(keyring, &token)?;
                    return Ok(Some(token));
                }
                Ok(Err(e)) => {
                    state.status = LoginStatus::Error(format!("QR auth failed: {e}"));
                    qr_result_rx = None;
                    qr_lines = None;
                }
                Err(mpsc::error::TryRecvError::Empty) => {} // Still waiting
                Err(mpsc::error::TryRecvError::Disconnected) => {
                    state.status = LoginStatus::Error("QR auth connection lost".to_string());
                    qr_result_rx = None;
                    qr_lines = None;
                }
            }
        }

        if !event::poll(Duration::from_millis(16))? {
            continue;
        }

        let Event::Key(key) = event::read()? else {
            continue;
        };

        match key.code {
            KeyCode::Esc => return Ok(None),
            KeyCode::Tab => state.next_field(),
            KeyCode::F(1) => {
                state.set_method(LoginMethod::Token);
                qr_result_rx = None;
                qr_lines = None;
            }
            KeyCode::F(2) => {
                state.set_method(LoginMethod::QrCode);
                if qr_result_rx.is_none() {
                    match start_qr_auth(config) {
                        Ok((rx, lines)) => {
                            qr_result_rx = Some(rx);
                            qr_lines = Some(lines);
                            state.status = LoginStatus::Validating;
                        }
                        Err(e) => {
                            state.status = LoginStatus::Error(format!("QR init failed: {e}"));
                        }
                    }
                }
            }
            KeyCode::Enter => {
                if state.can_submit() {
                    if let Some(token) =
                        handle_login_submit(&mut state, terminal, config, keyring).await?
                    {
                        return Ok(Some(token));
                    }
                }
            }
            KeyCode::Char(c) => state.type_char(c),
            KeyCode::Backspace => state.backspace(),
            _ => {}
        }
    }
}

/// Handle Enter in the login form. Returns `Some(token)` if login succeeded.
async fn handle_login_submit(
    state: &mut LoginState,
    terminal: &mut Term,
    config: &AppConfig,
    keyring: &KeyringStore,
) -> Result<Option<String>> {
    match state.method {
        LoginMethod::Token => {
            let token = state.token_input.clone();
            state.status = LoginStatus::Validating;
            terminal.draw(|f| f.render_widget(LoginScreen::new(state), f.area()))?;

            match discordinator::auth::validate_token_via_gateway(
                &token,
                "wss://gateway.discord.gg/?v=10&encoding=json",
                &config.discord,
            )
            .await
            {
                Ok(true) => {
                    discordinator::auth::store_token(keyring, &token)?;
                    Ok(Some(token))
                }
                Ok(false) => {
                    state.status = LoginStatus::Error("Invalid token".to_string());
                    Ok(None)
                }
                Err(e) => {
                    state.status = LoginStatus::Error(format!("Validation failed: {e}"));
                    Ok(None)
                }
            }
        }

        LoginMethod::EmailPassword | LoginMethod::QrCode => {
            // QR auth is handled asynchronously via background task
            Ok(None)
        }
    }
}

// === QR Code Authentication ===

/// Start QR code authentication. Returns a receiver for the result and QR lines for display.
fn start_qr_auth(config: &AppConfig) -> Result<(mpsc::Receiver<Result<String>>, Vec<String>)> {
    let session = discordinator::auth::QrAuthSession::new()?;
    let qr_lines = session.generate_qr_lines()?;

    let (tx, rx) = mpsc::channel(1);
    let discord_config = config.discord.clone();

    tokio::spawn(async move {
        let result = run_qr_auth_flow(session, &discord_config).await;
        let _ = tx.send(result).await;
    });

    Ok((rx, qr_lines))
}

/// Run the QR auth WebSocket protocol flow.
async fn run_qr_auth_flow(
    session: discordinator::auth::QrAuthSession,
    config: &discordinator::config::DiscordConfig,
) -> Result<String> {
    use discordinator::auth::{
        build_qr_auth_heartbeat, build_qr_auth_init, build_qr_auth_nonce_proof,
        parse_qr_auth_message, QrAuthMessage,
    };
    use futures_util::SinkExt;
    use tokio_tungstenite::tungstenite::Message;

    let ws_request = discordinator::infrastructure::anti_detection::build_ws_request(
        "wss://remote-auth-gateway.discord.gg/?v=2",
        config,
    )
    .map_err(|e| eyre!("Failed to build QR auth WS request: {e}"))?;
    let (ws_stream, _) = tokio::time::timeout(
        Duration::from_secs(10),
        tokio_tungstenite::connect_async(ws_request),
    )
    .await
    .map_err(|_| eyre!("QR auth gateway connection timed out"))?
    .map_err(|e| eyre!("QR auth gateway connection failed: {e}"))?;

    let (mut write, mut read) = ws_stream.split();
    let mut heartbeat_interval = Duration::from_secs(41);

    // Read hello
    let hello = tokio::time::timeout(Duration::from_secs(10), read.next())
        .await
        .map_err(|_| eyre!("Timeout waiting for QR hello"))?
        .ok_or_else(|| eyre!("QR connection closed"))?
        .map_err(|e| eyre!("QR WebSocket error: {e}"))?;

    let hello_text = hello
        .into_text()
        .map_err(|e| eyre!("QR hello not text: {e}"))?;
    let hello_json: serde_json::Value = serde_json::from_str(&hello_text)?;

    if let QrAuthMessage::Hello {
        heartbeat_interval: hi,
        ..
    } = parse_qr_auth_message(&hello_json)
    {
        heartbeat_interval = Duration::from_millis(hi);
    }

    // Send init
    let init = build_qr_auth_init(&session.encoded_public_key());
    write
        .send(Message::Text(init.to_string().into()))
        .await
        .map_err(|e| eyre!("Failed to send QR init: {e}"))?;

    // Protocol loop
    let heartbeat_sleep = tokio::time::sleep(heartbeat_interval);
    tokio::pin!(heartbeat_sleep);

    loop {
        tokio::select! {
            biased;

            msg = read.next() => {
                let msg = msg
                    .ok_or_else(|| eyre!("QR connection closed"))?
                    .map_err(|e| eyre!("QR WebSocket error: {e}"))?;

                let text = msg.into_text().map_err(|e| eyre!("QR msg not text: {e}"))?;
                let json: serde_json::Value = serde_json::from_str(&text)?;
                let qr_msg = parse_qr_auth_message(&json);

                match qr_msg {
                    QrAuthMessage::NonceProof { encrypted_nonce } => {
                        let proof = session.compute_nonce_proof(&encrypted_nonce)?;
                        let proof_msg = build_qr_auth_nonce_proof(&proof);
                        write.send(Message::Text(proof_msg.to_string().into())).await
                            .map_err(|e| eyre!("Failed to send nonce proof: {e}"))?;
                    }
                    QrAuthMessage::PendingRemoteInit { .. } => {
                        tracing::info!("QR code scanned, waiting for confirmation");
                    }
                    QrAuthMessage::PendingTicket { encrypted_user_payload } => {
                        let _payload = session.decrypt_payload(&encrypted_user_payload)?;
                        tracing::info!("QR auth user confirmed");
                    }
                    QrAuthMessage::PendingLogin { ticket } => {
                        let encrypted_token = exchange_qr_ticket(&ticket, config).await?;
                        let token = session.decrypt_payload(&encrypted_token)?;
                        return Ok(token);
                    }
                    QrAuthMessage::Cancel => {
                        return Err(eyre!("QR auth cancelled"));
                    }
                    _ => {}
                }
            }

            () = &mut heartbeat_sleep => {
                let hb = build_qr_auth_heartbeat();
                write.send(Message::Text(hb.to_string().into())).await
                    .map_err(|e| eyre!("Failed to send QR heartbeat: {e}"))?;
                heartbeat_sleep.as_mut().reset(
                    tokio::time::Instant::now() + heartbeat_interval,
                );
            }
        }
    }
}

/// Exchange a QR auth ticket for an encrypted token via Discord API.
async fn exchange_qr_ticket(
    ticket: &str,
    config: &discordinator::config::DiscordConfig,
) -> Result<String> {
    let client = discordinator::auth::build_auth_client(config)?;
    let response = client
        .post(format!("{DISCORD_API_BASE}/users/@me/remote-auth/login"))
        .json(&serde_json::json!({ "ticket": ticket }))
        .send()
        .await
        .map_err(|e| eyre!("QR ticket exchange failed: {e}"))?;

    let body: serde_json::Value = response
        .json()
        .await
        .map_err(|e| eyre!("Failed to parse QR ticket response: {e}"))?;

    body["encrypted_token"]
        .as_str()
        .map(String::from)
        .ok_or_else(|| eyre!("No encrypted_token in QR response"))
}

/// Try to load cached Discord properties, or fetch fresh ones.
async fn fetch_or_load_properties(
    cache_path: &std::path::Path,
    user_agent: &str,
) -> Option<discord_properties::DiscordProperties> {
    // Try cache first
    if let Some(cached) = discord_properties::load_cached(cache_path) {
        return Some(cached);
    }

    // Fetch fresh
    match discord_properties::fetch_discord_properties(user_agent).await {
        Ok(props) => {
            if let Err(e) = discord_properties::save_cached(cache_path, &props) {
                tracing::warn!("Failed to cache Discord properties: {e}");
            }
            Some(props)
        }
        Err(e) => {
            tracing::warn!("Failed to fetch Discord properties: {e}");
            None
        }
    }
}
