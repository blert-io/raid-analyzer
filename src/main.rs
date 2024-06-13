#![warn(clippy::pedantic)]
#![allow(clippy::cast_possible_truncation)]

use challenge::Challenge;
use std::env;
use uuid::Uuid;

use data_repository::{DataRepository, FilesystemBackend, S3Backend};
use error::{Error, Result};

mod analysis;
mod analyzers;
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

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();

    let uuid = std::env::args()
        .nth(1)
        .map(|s| Uuid::parse_str(&s).unwrap())
        .expect("expected UUID as first argument");

    let repository = initialize_data_repository().await?;
    let database_pool = sqlx::postgres::PgPoolOptions::new()
        .connect(&var("BLERT_DATABASE_URI")?)
        .await?;

    let mut analysis_engine = analysis::Engine::load_from_directory("./programs").await?;
    analysis_engine.start(8);

    let challenge = Challenge::load(&database_pool, &repository, uuid).await?;

    analysis_engine
        .run_program("analysis_test", analysis::Level::Basic, challenge)
        .await?;

    Ok(())
}

async fn initialize_data_repository() -> Result<DataRepository> {
    use data_repository::Backend;

    let uri = var("BLERT_DATA_REPOSITORY")?;

    let backend: Box<dyn Backend + Sync + 'static> = match uri.split_once("://") {
        Some(("file", path)) => Box::new(FilesystemBackend::new(std::path::Path::new(path))),
        Some(("s3", bucket)) => {
            let endpoint = var("BLERT_S3_ENDPOINT")?;
            Box::new(S3Backend::init(&endpoint, bucket).await)
        }
        Some((_, _)) | None => return Err(Error::Environment("BLERT_DATA_REPOSITORY")),
    };

    Ok(DataRepository::new(backend))
}
