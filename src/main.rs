use std::process::exit;

use log::{error, info};
use tokio::net::TcpListener;
use tokio::signal;
use tokio_cron_scheduler::{Job, JobScheduler};

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

    // `cargo run -- seed --org-slug acme` loads the default lookups/catalog.
    if std::env::args().nth(1).as_deref() == Some("seed") {
        if let Err(e) =
            rust_api::seed::run(&db_connection, std::env::args().skip(2).collect()).await
        {
            error!("Seed failed: {}", e);
            exit(1);
        }
        return;
    }

    // Daily preventive-maintenance sweep. `run_once` is also public so tests
    // and operators can trigger a sweep without waiting for the cron tick.
    let maintenance_cron =
        std::env::var("MAINTENANCE_CRON").unwrap_or_else(|_| "0 0 6 * * *".to_string());
    let _maintenance_scheduler = match JobScheduler::new().await {
        Ok(scheduler) => {
            let pool = db_connection.get_pool().clone();
            match Job::new_async(maintenance_cron.as_str(), move |_uuid, _lock| {
                let pool = pool.clone();
                Box::pin(async move {
                    if let Err(e) = rust_api::modules::maintenances::run_once(&pool).await {
                        error!("Maintenance scheduler run failed: {e}");
                    }
                })
            }) {
                Ok(job) => {
                    if let Err(e) = scheduler.add(job).await {
                        error!("Failed to register maintenance scheduler: {e}");
                    } else if let Err(e) = scheduler.start().await {
                        error!("Failed to start maintenance scheduler: {e}");
                    } else {
                        info!("Maintenance scheduler started with cron '{maintenance_cron}'");
                    }
                }
                Err(e) => error!("Invalid MAINTENANCE_CRON '{maintenance_cron}': {e}"),
            }
            Some(scheduler)
        }
        Err(e) => {
            error!("Failed to create maintenance scheduler: {e}");
            None
        }
    };

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
