#![warn(clippy::pedantic)]
#![allow(clippy::cast_possible_truncation)]

use axum::Router;
use std::{
    env,
    sync::{Arc, Mutex},
};
use tokio::net::TcpListener;

use data_repository::{DataRepository, FilesystemBackend, S3Backend};
use error::{Error, Result};

mod analysis;
mod analyzers;
mod api;
mod challenge;
mod data_repository;
mod error;

mod blert {
    #![allow(clippy::all)]
    include!(concat!(env!("OUT_DIR"), "/blert.rs"));
}

fn var(name: &'static str) -> Result<String> {
    env::var(name).map_err(|_| Error::Environment(name))
}

pub struct AppState {
    pub analysis_engine: Mutex<analysis::Engine>,
    pub data_repository: DataRepository,
    pub database_pool: sqlx::PgPool,
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();

    let repository = initialize_data_repository().await?;
    let database_pool = sqlx::postgres::PgPoolOptions::new()
        .connect(&var("BLERT_DATABASE_URI")?)
        .await?;

    let mut analysis_engine = analysis::Engine::load_from_directory("./programs").await?;
    analysis_engine.start(8);

    let state = Arc::new(AppState {
        analysis_engine: Mutex::new(analysis_engine),
        data_repository: repository,
        database_pool,
    });

    let port = match env::var("PORT") {
        Ok(port) => port.parse().expect("Invalid port number"),
        Err(_) => 3033,
    };

    let app = Router::new()
        .route("/analyze", axum::routing::post(api::analyze))
        .with_state(state);
    let listener = TcpListener::bind(("127.0.0.1", port))
        .await
        .expect("Failed to bind port");

    log::info!("Server listening on port {port}");
    axum::serve(listener, app).await.expect("Server failed");

    Ok(())
}

async fn initialize_data_repository() -> Result<DataRepository> {
    use data_repository::Backend;

    let uri = var("BLERT_DATA_REPOSITORY")?;

    let backend: Box<dyn Backend + Sync + Send + 'static> = match uri.split_once("://") {
        Some(("file", path)) => Box::new(FilesystemBackend::new(std::path::Path::new(path))),
        Some(("s3", bucket)) => {
            let endpoint = var("BLERT_S3_ENDPOINT")?;
            Box::new(S3Backend::init(&endpoint, bucket).await)
        }
        Some((_, _)) | None => return Err(Error::Environment("BLERT_DATA_REPOSITORY")),
    };

    Ok(DataRepository::new(backend))
}
