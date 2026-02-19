use color_eyre::eyre::Result;

#[allow(dead_code)]
mod app;
#[allow(dead_code)]
mod config;
#[allow(dead_code)]
mod domain;
#[allow(dead_code)]
mod infrastructure;
#[allow(dead_code)]
mod input;
#[allow(dead_code)]
mod logging;
#[allow(dead_code)]
mod markdown;
#[allow(dead_code)]
mod ui;

#[tokio::main]
async fn main() -> Result<()> {
    logging::install_panic_handler()?;

    // For now, use simple stderr logging until full app startup is implemented.
    // The full init_logging() with file output will be used when the app loop is built.
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    tracing::info!("Discordinator starting up");

    Ok(())
}
