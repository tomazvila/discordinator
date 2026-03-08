use std::time::Duration;

use color_eyre::eyre::Result;
use crossterm::event::{self, Event, KeyCode};

use discordinator::app::App;
use discordinator::config::AppConfig;
use discordinator::infrastructure::keyring::KeyringStore;
use discordinator::ui::login::{LoginMethod, LoginScreen, LoginState, LoginStatus};

type Term = ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>>;

const GATEWAY_URL: &str = "wss://gateway.discord.gg/?v=10&encoding=json";

#[tokio::main]
async fn main() -> Result<()> {
    discordinator::logging::install_panic_handler()?;
    let dirs = discordinator::config::AppDirs::new()?;
    let config = discordinator::config::load_or_create_config(&dirs.config_file())?;
    let _log_guard = discordinator::logging::init_logging(&dirs.log_dir())?;

    tracing::info!("Discordinator starting up");

    let mut terminal = App::setup_terminal()?;
    let result = run(&mut terminal, config).await;
    App::restore_terminal(&mut terminal)?;

    result
}

async fn run(terminal: &mut Term, config: AppConfig) -> Result<()> {
    let keyring = KeyringStore;
    let env_getter = |key: &str| -> Option<String> { std::env::var(key).ok() };
    let stored = discordinator::auth::retrieve_token(&config.auth, &keyring, &env_getter)?;

    if stored.is_none() && !login_loop(terminal, &config, &keyring).await? {
        return Ok(());
    }

    // Main event loop
    let mut app = App::new(config);

    loop {
        if app.dirty {
            terminal.draw(|f| {
                discordinator::ui::layout::render(f.area(), f.buffer_mut(), &app.state);
            })?;
            app.dirty = false;
        }

        if event::poll(Duration::from_millis(16))? {
            match event::read()? {
                Event::Key(key) => app.dirty = app.handle_terminal_event(key),
                Event::Resize(_, _) => app.dirty = true,
                _ => {}
            }
        }

        if app.should_quit {
            break;
        }
    }

    Ok(())
}

/// Run the login UI. Returns `Ok(true)` on successful login, `Ok(false)` if cancelled.
async fn login_loop(
    terminal: &mut Term,
    config: &AppConfig,
    keyring: &KeyringStore,
) -> Result<bool> {
    let mut state = LoginState::default();

    loop {
        terminal.draw(|f| {
            f.render_widget(LoginScreen::new(&state), f.area());
        })?;

        if !event::poll(Duration::from_millis(16))? {
            continue;
        }

        let Event::Key(key) = event::read()? else {
            continue;
        };

        match key.code {
            KeyCode::Esc => return Ok(false),
            KeyCode::Tab => state.next_field(),
            KeyCode::Char('1') => state.set_method(LoginMethod::Token),
            KeyCode::Char('2') => state.set_method(LoginMethod::EmailPassword),
            KeyCode::Char('3') => state.set_method(LoginMethod::QrCode),
            KeyCode::Enter => {
                if state.can_submit()
                    && handle_login_submit(&mut state, terminal, config, keyring).await?
                {
                    return Ok(true);
                }
            }
            KeyCode::Char(c) => state.type_char(c),
            KeyCode::Backspace => state.backspace(),
            _ => {}
        }
    }
}

/// Handle Enter in the login form. Returns `Ok(true)` if login succeeded.
async fn handle_login_submit(
    state: &mut LoginState,
    terminal: &mut Term,
    config: &AppConfig,
    keyring: &KeyringStore,
) -> Result<bool> {
    if state.method == LoginMethod::Token {
        let token = state.token_input.clone();
        state.status = LoginStatus::Validating;
        terminal.draw(|f| f.render_widget(LoginScreen::new(state), f.area()))?;

        match discordinator::auth::validate_token_via_gateway(
            &token,
            GATEWAY_URL,
            &config.discord,
        )
        .await
        {
            Ok(true) => {
                discordinator::auth::store_token(keyring, &token)?;
                Ok(true)
            }
            Ok(false) => {
                state.status = LoginStatus::Error("Invalid token".to_string());
                Ok(false)
            }
            Err(e) => {
                state.status = LoginStatus::Error(format!("Validation failed: {e}"));
                Ok(false)
            }
        }
    } else {
        state.status = LoginStatus::Error("Not yet implemented".to_string());
        Ok(false)
    }
}
