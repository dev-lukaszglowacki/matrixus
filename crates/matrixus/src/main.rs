//! Matrixus entrypoint

use std::sync::Arc;

use tracing::info;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::from_default_env().add_directive("matrixus=info".parse()?),
        )
        .init();

    info!("Starting Matrixus");
    let app = Arc::new(matrixus::MatrixusApp::new());

    if app.try_auto_login().await {
        info!("Resumed previous session successfully.");
    } else {
        info!("No stored session found. Ready for login.");
    }

    #[cfg(feature = "gui")]
    {
        info!("Initializing GTK4 / Libadwaita user interface");
        // Blocks until the window is closed. Tokio runtime stays alive for the process.
        let exit_code = matrixus::ui::gtk_app::run(app);
        info!("GTK application exited with code: {:?}", exit_code);
        // glib::ExitCode converts to u8; map to process exit status.
        let code: u8 = exit_code.into();
        std::process::exit(i32::from(code));
    }

    #[cfg(not(feature = "gui"))]
    {
        info!(
            "Matrix client initialized successfully (built without `gui` feature — no window)."
        );
        info!("Rebuild with: cargo run -p matrixus --features gui");
        Ok(())
    }
}

