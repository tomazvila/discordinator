use color_eyre::eyre::Result;
use std::path::Path;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::prelude::*;
use tracing_subscriber::EnvFilter;

/// Maximum size per log file in bytes (10 MB).
const MAX_LOG_FILE_SIZE: u64 = 10 * 1024 * 1024;

/// Maximum number of log files to keep.
const MAX_LOG_FILES: usize = 5;

/// Initialize the tracing subscriber with file logging and env filter.
///
/// Returns a `WorkerGuard` that must be held alive for the duration of the program.
/// When dropped, it flushes any remaining log output.
pub fn init_logging(log_dir: &Path) -> Result<WorkerGuard> {
    std::fs::create_dir_all(log_dir)?;

    // Rotate old logs before starting
    rotate_logs(log_dir);

    let log_file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_dir.join("discordinator.log"))?;

    let (non_blocking, guard) = tracing_appender::non_blocking(log_file);

    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    let file_layer = tracing_subscriber::fmt::layer()
        .with_writer(non_blocking)
        .with_ansi(false)
        .with_target(true)
        .with_thread_ids(true);

    tracing_subscriber::registry()
        .with(env_filter)
        .with(file_layer)
        .init();

    Ok(guard)
}

/// Rotate log files: rename discordinator.log -> discordinator.1.log, etc.
/// Delete files beyond `MAX_LOG_FILES`.
fn rotate_logs(log_dir: &Path) {
    let current_log = log_dir.join("discordinator.log");
    if !current_log.exists() {
        return;
    }

    // Only rotate if the file is large enough
    if let Ok(metadata) = std::fs::metadata(&current_log) {
        if metadata.len() < MAX_LOG_FILE_SIZE {
            return;
        }
    }

    // Delete the oldest log file if it exists
    let oldest = log_dir.join(format!("discordinator.{MAX_LOG_FILES}.log"));
    let _ = std::fs::remove_file(oldest);

    // Shift existing rotated files
    for i in (1..MAX_LOG_FILES).rev() {
        let from = log_dir.join(format!("discordinator.{i}.log"));
        let to = log_dir.join(format!("discordinator.{}.log", i + 1));
        let _ = std::fs::rename(from, to);
    }

    // Rotate current -> .1
    let rotated = log_dir.join("discordinator.1.log");
    let _ = std::fs::rename(&current_log, rotated);
}

/// Install color-eyre with a custom panic handler that restores terminal state.
pub fn install_panic_handler() -> Result<()> {
    let (panic_hook, eyre_hook) = color_eyre::config::HookBuilder::default()
        .panic_section("This is a bug in Discordinator. Please report it.")
        .into_hooks();

    eyre_hook.install()?;

    std::panic::set_hook(Box::new(move |panic_info| {
        // Attempt to restore terminal state before printing error
        let _ = crossterm::terminal::disable_raw_mode();
        let _ = crossterm::execute!(
            std::io::stderr(),
            crossterm::terminal::LeaveAlternateScreen,
            crossterm::cursor::Show
        );

        // Now print the panic report
        let report = panic_hook.panic_report(panic_info);
        eprintln!("{report}");
    }));

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_file_is_created() {
        let dir = tempfile::tempdir().unwrap();
        let log_dir = dir.path().join("logs");

        // We can't call init_logging in tests because it installs a global subscriber.
        // Instead, test the file creation logic directly.
        std::fs::create_dir_all(&log_dir).unwrap();
        let log_path = log_dir.join("discordinator.log");
        std::fs::write(&log_path, "test log entry\n").unwrap();

        assert!(log_path.exists());
        let content = std::fs::read_to_string(&log_path).unwrap();
        assert!(content.contains("test log entry"));
    }

    #[test]
    fn log_rotation_skips_small_files() {
        let dir = tempfile::tempdir().unwrap();
        let log_dir = dir.path();
        let log_path = log_dir.join("discordinator.log");

        // Write a small file (under MAX_LOG_FILE_SIZE)
        std::fs::write(&log_path, "small content").unwrap();

        rotate_logs(log_dir);

        // File should NOT be rotated
        assert!(log_path.exists());
        assert!(!log_dir.join("discordinator.1.log").exists());
    }

    #[test]
    fn log_rotation_rotates_large_files() {
        let dir = tempfile::tempdir().unwrap();
        let log_dir = dir.path();
        let log_path = log_dir.join("discordinator.log");

        // Write a large file (over MAX_LOG_FILE_SIZE)
        let large_content = "x".repeat(MAX_LOG_FILE_SIZE as usize + 1);
        std::fs::write(&log_path, &large_content).unwrap();

        rotate_logs(log_dir);

        // Original file should be gone, .1.log should exist
        assert!(!log_path.exists());
        assert!(log_dir.join("discordinator.1.log").exists());

        let rotated_content = std::fs::read_to_string(log_dir.join("discordinator.1.log")).unwrap();
        assert_eq!(rotated_content.len(), MAX_LOG_FILE_SIZE as usize + 1);
    }

    #[test]
    fn log_rotation_shifts_existing_files() {
        let dir = tempfile::tempdir().unwrap();
        let log_dir = dir.path();

        // Create existing rotated files
        std::fs::write(log_dir.join("discordinator.1.log"), "old1").unwrap();
        std::fs::write(log_dir.join("discordinator.2.log"), "old2").unwrap();

        // Create a large current log
        let large_content = "x".repeat(MAX_LOG_FILE_SIZE as usize + 1);
        std::fs::write(log_dir.join("discordinator.log"), &large_content).unwrap();

        rotate_logs(log_dir);

        // .1 should now be the rotated current file
        // .2 should be the old .1
        // .3 should be the old .2
        assert!(!log_dir.join("discordinator.log").exists());
        assert!(log_dir.join("discordinator.1.log").exists());
        assert!(log_dir.join("discordinator.2.log").exists());
        assert!(log_dir.join("discordinator.3.log").exists());

        let content2 = std::fs::read_to_string(log_dir.join("discordinator.2.log")).unwrap();
        assert_eq!(content2, "old1");
        let content3 = std::fs::read_to_string(log_dir.join("discordinator.3.log")).unwrap();
        assert_eq!(content3, "old2");
    }

    #[test]
    fn log_rotation_deletes_oldest_beyond_max() {
        let dir = tempfile::tempdir().unwrap();
        let log_dir = dir.path();

        // Create MAX_LOG_FILES rotated files
        for i in 1..=MAX_LOG_FILES {
            std::fs::write(
                log_dir.join(format!("discordinator.{}.log", i)),
                format!("old{}", i),
            )
            .unwrap();
        }

        // Create a large current log
        let large_content = "x".repeat(MAX_LOG_FILE_SIZE as usize + 1);
        std::fs::write(log_dir.join("discordinator.log"), &large_content).unwrap();

        rotate_logs(log_dir);

        // .MAX_LOG_FILES+1 should NOT exist (deleted)
        assert!(!log_dir
            .join(format!("discordinator.{}.log", MAX_LOG_FILES + 1))
            .exists());
        // .MAX_LOG_FILES should exist (was .MAX_LOG_FILES-1)
        assert!(log_dir
            .join(format!("discordinator.{}.log", MAX_LOG_FILES))
            .exists());
    }

    #[test]
    fn log_rotation_handles_nonexistent_file() {
        let dir = tempfile::tempdir().unwrap();
        // No log file exists - should not panic
        rotate_logs(dir.path());
    }

    #[test]
    fn log_directory_creation() {
        let dir = tempfile::tempdir().unwrap();
        let log_dir = dir.path().join("nested").join("logs");

        std::fs::create_dir_all(&log_dir).unwrap();
        assert!(log_dir.exists());
    }
}
