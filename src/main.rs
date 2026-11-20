use std::process::exit;
use std::sync::Arc;
use log::{error, info};
use tokio::net::TcpListener;
use tokio::signal;
use crate::config::parameter;
use crate::config::database::{Database, DatabaseTrait};

mod config;
mod routes;
mod dto;
mod error;
mod response;
mod entity;
mod repository;
mod state;
mod service;
mod middleware;
mod handler;

#[tokio::main]
async fn main() {
    log4rs::init_file("log4rs.yaml", Default::default())
        .expect("Failed to init logger");

    info!("Logger initialized");

    if let Err(e) = parameter::init() {
        error!("Parameter init failed: {}", e);
        exit(1);
    }

    info!("Environment variable loaded");

    let db_connection = Database::init()
        .await
        .unwrap_or_else(|e| {
            error!("Failed to initialize database: {}", e);
            exit(1);
        });

    info!("Database connected");

    let app_url = parameter::get("APP_URL")
        .unwrap_or_else(|e| {
            error!("{}", e);
            exit(1);
        });

    let app_port = parameter::get("APP_PORT")
        .unwrap_or_else(|e| {
            error!("{}", e);
            exit(1);
        });

    let host = format!("{}:{}", app_url, app_port);

    info!("Server starting at {}", host);

    let listener = TcpListener::bind(&host)
        .await
        .unwrap_or_else(|e| {
            error!("Error binding address {}: {}", host, e);
            exit(1);
        });

    let app = routes::root::routes(Arc::new(db_connection));

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .unwrap_or_else(|e| {
            error!("Server error: {}", e);
        });

    info!("Server stopped");
}


async fn shutdown_signal() {
    signal::ctrl_c()
        .await
        .unwrap_or_else(|_| {
            error!("Failed to install CTRL+C handler");
            exit(1);
        });
}
