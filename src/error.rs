use crate::data_repository;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub enum Error {
    Environment(&'static str),
    InvalidField(String),
    DataRepository(data_repository::Error),
    Sql(sqlx::Error),
}

impl From<data_repository::Error> for Error {
    fn from(e: data_repository::Error) -> Self {
        Self::DataRepository(e)
    }
}

impl From<sqlx::Error> for Error {
    fn from(e: sqlx::Error) -> Self {
        Self::Sql(e)
    }
}
