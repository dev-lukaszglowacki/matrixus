//! Matrix Desktop Client Entrypoint

use tracing::info;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("matrix_desktop=info".parse()?))
        .init();

    info!("Starting Matrix Linux Desktop Client");
    let app = matrix_desktop::MatrixDesktopApp::new();

    if app.try_auto_login().await {
        info!("Resumed previous session successfully.");
    } else {
        info!("No stored session found. Ready for login.");
    }

    #[cfg(feature = "gui")]
    {
        info!("Initializing GTK4 / Libadwaita user interface");
        // libadwaita::init()?;
        // Application launch logic
    }

    info!("Matrix client initialized successfully.");
    Ok(())
}
