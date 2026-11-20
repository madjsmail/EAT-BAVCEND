use std::process::exit;

use log::{error, info};
use tokio::net::TcpListener;
use tokio::signal;

use rust_api::config::database::{Database, DatabaseTrait};
use rust_api::config::parameter;
use rust_api::{cli, routes};

#[tokio::main]
async fn main() {
    log4rs::init_file("log4rs.yaml", Default::default()).expect("Failed to init logger");
    info!("Logger initialized");

    if let Err(e) = parameter::init() {
        error!("Parameter init failed: {}", e);
        exit(1);
    }
    info!("Environment variable loaded");

    let db_connection = Database::init().await.unwrap_or_else(|e| {
        error!("Failed to initialize database: {}", e);
        exit(1);
    });
    info!("Database connected");

    if let Err(e) = sqlx::migrate!("./migrations")
        .run(db_connection.get_pool())
        .await
    {
        error!("Migrations failed: {}", e);
        exit(1);
    }

    // `cargo run -- bootstrap ...` creates the first organization + admin.
    if std::env::args().nth(1).as_deref() == Some("bootstrap") {
        if let Err(e) = cli::bootstrap(&db_connection, std::env::args().skip(2).collect()).await {
            error!("Bootstrap failed: {}", e);
            exit(1);
        }
        return;
    }

    let app_url = parameter::get("APP_URL").unwrap_or_else(|e| {
        error!("{}", e);
        exit(1);
    });
    let app_port = parameter::get("APP_PORT").unwrap_or_else(|e| {
        error!("{}", e);
        exit(1);
    });
    let host = format!("{}:{}", app_url, app_port);
    info!("Server starting at {}", host);

    let listener = TcpListener::bind(&host).await.unwrap_or_else(|e| {
        error!("Error binding address {}: {}", host, e);
        exit(1);
    });

    axum::serve(
        listener,
        routes::root::routes(std::sync::Arc::new(db_connection)),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await
    .unwrap_or_else(|e| {
        error!("Server error: {}", e);
    });

    info!("Server stopped");
}

async fn shutdown_signal() {
    signal::ctrl_c().await.unwrap_or_else(|_| {
        error!("Failed to install CTRL+C handler");
        exit(1);
    });
}
