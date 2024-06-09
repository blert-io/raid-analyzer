use crate::data_repository;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub enum Error {
    Environment(&'static str),
    InvalidField(String),
    IncompleteData,
    InvalidArgument,
    NotRunning,
    DataRepository(data_repository::Error),
    Io(std::io::Error),
    Sql(sqlx::Error),
    Config(String),
}

impl From<data_repository::Error> for Error {
    fn from(e: data_repository::Error) -> Self {
        Self::DataRepository(e)
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<sqlx::Error> for Error {
    fn from(e: sqlx::Error) -> Self {
        Self::Sql(e)
    }
}

impl From<toml::de::Error> for Error {
    fn from(e: toml::de::Error) -> Self {
        Self::Config(e.message().to_owned())
    }
}
