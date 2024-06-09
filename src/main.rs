#![warn(clippy::pedantic)]
#![allow(clippy::cast_possible_truncation)]

use challenge::Challenge;
use std::env;
use uuid::Uuid;

use data_repository::{DataRepository, FilesystemBackend};
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

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();

    let uuid = std::env::args()
        .nth(1)
        .map(|s| Uuid::parse_str(&s).unwrap())
        .expect("expected UUID as first argument");

    let repository = initialize_data_repository()?;
    let database_pool = sqlx::postgres::PgPoolOptions::new()
        .connect(&env::var("BLERT_DATABASE_URI").expect("BLERT_DATABASE_URI not set"))
        .await?;

    let mut analysis_engine = analysis::Engine::load_from_directory("./programs").await?;
    analysis_engine.start(8);

    let challenge = Challenge::load(&database_pool, &repository, uuid).await?;

    analysis_engine
        .run_program("analysis_test", analysis::Level::Basic, challenge)
        .await?;

    Ok(())
}

fn initialize_data_repository() -> Result<data_repository::DataRepository> {
    let uri = env::var("BLERT_DATA_REPOSITORY")
        .map_err(|_| Error::Environment("BLERT_DATA_REPOSITORY"))?;

    let backend = match uri.split_once("://") {
        Some(("file", path)) => FilesystemBackend::new(std::path::Path::new(path)),
        Some(("s3", _)) => unimplemented!(),
        Some((_, _)) | None => return Err(Error::Environment("BLERT_DATA_REPOSITORY")),
    };

    Ok(DataRepository::new(Box::new(backend)))
}
