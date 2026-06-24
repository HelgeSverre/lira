//! Lira Playground Server
//!
//! Web server for the Lira Playground - compiles and runs Lira code in the browser.

mod handlers;
mod protocol;
mod vm_thread;

use axum::{
    routing::{get, post},
    Router,
};
use std::net::SocketAddr;
use tower_http::{
    cors::{Any, CorsLayer},
    services::{ServeDir, ServeFile},
};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() {
    // Initialize tracing
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "lira_playground=debug,tower_http=debug".into()),
        )
        .init();

    // Build CORS layer
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    // Static file serving for frontend
    // Path is relative to where the binary is run from (project root)
    let frontend_dir = std::env::var("FRONTEND_DIR")
        .unwrap_or_else(|_| "lira-playground/frontend/dist".to_string());

    // SPA fallback - serve index.html for routes that don't match files
    let index_path = format!("{}/index.html", frontend_dir);
    let serve_dir = ServeDir::new(&frontend_dir).not_found_service(ServeFile::new(&index_path));

    // Build router
    let app = Router::new()
        // API routes (higher priority)
        .route("/health", get(handlers::health))
        .route("/api/compile", post(handlers::compile))
        .route("/api/run", post(handlers::run))
        .route("/api/check", post(handlers::check))
        .route("/api/step", post(handlers::step))
        .route("/ws", get(handlers::websocket))
        .layer(cors)
        // Serve frontend static files with SPA fallback
        .fallback_service(serve_dir);

    // Get port from environment or use default
    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(3001);

    let addr = SocketAddr::from(([0, 0, 0, 0], port));

    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(listener) => listener,
        Err(e) => {
            tracing::error!("Failed to bind to {}: {}", addr, e);
            tracing::info!("Hint: Try setting PORT environment variable to a different port");
            std::process::exit(1);
        }
    };

    tracing::info!("Lira Playground server listening on http://{}", addr);
    tracing::info!("Serving frontend from: {}", frontend_dir);
    axum::serve(listener, app).await.unwrap();
}
