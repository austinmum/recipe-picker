use std::net::SocketAddr;

use anyhow::{Context, Result};
use recipe_picker::{config::Config, db::Db, web};

#[tokio::main]
async fn main() -> Result<()> {
    // Secrets (API keys) may live in a .env file next to config.toml.
    let _ = dotenvy::dotenv();

    let root = std::env::current_dir()?;
    let mut config = Config::load(&root)?;
    // Lets you point at a different data folder without editing config.toml.
    if let Some(dir) = recipe_picker::config::env_var("RECIPE_PICKER_DATA_DIR") {
        config.data_dir = dir.into();
    }
    if let Some(port) = recipe_picker::config::env_var("RECIPE_PICKER_PORT").and_then(|p| p.parse().ok()) {
        config.port = port;
    }
    let data_dir = if config.data_dir.is_absolute() { config.data_dir.clone() } else { root.join(&config.data_dir) };
    std::fs::create_dir_all(data_dir.join("images")).with_context(|| format!("creating {}", data_dir.display()))?;

    let db = Db::open(&data_dir.join("recipes.db"))?;
    let app = web::router(web::AppState::new(db, root, data_dir.clone(), config.clone()));

    // 127.0.0.1 = only reachable from this computer.
    let addr = SocketAddr::from(([127, 0, 0, 1], config.port));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("port {} is busy - is the app already running? (change `port` in config.toml)", config.port))?;
    println!("Recipe Picker is running: open http://localhost:{}", config.port);
    println!("Data folder: {}", data_dir.display());
    println!("Press Ctrl+C to stop.");
    axum::serve(listener, app).await?;
    Ok(())
}
