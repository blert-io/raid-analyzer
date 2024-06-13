use prost::Message;
use std::{
    fs,
    io::Cursor,
    path::{Path, PathBuf},
};
use uuid::Uuid;

use crate::blert;

pub struct DataRepository {
    backend: Box<dyn Backend + Sync>,
}

impl DataRepository {
    const CHALLENGE_FILE_NAME: &'static str = "challenge";

    pub fn new(backend: Box<dyn Backend + Sync>) -> Self {
        Self { backend }
    }

    pub async fn load_challenge(&self, uuid: Uuid) -> Result<blert::ChallengeData, Error> {
        let data = self
            .backend
            .read_file(Self::relative_path(uuid, Self::CHALLENGE_FILE_NAME))
            .await?;
        blert::ChallengeData::decode(&mut Cursor::new(&data)).map_err(Error::from)
    }

    pub async fn load_stage_events(
        &self,
        uuid: Uuid,
        stage: blert::Stage,
    ) -> Result<blert::ChallengeEvents, Error> {
        let file_name = self.stage_file_name(stage);
        let raw = self
            .backend
            .read_file(Self::relative_path(uuid, file_name))
            .await?;
        blert::ChallengeEvents::decode(&mut Cursor::new(&raw)).map_err(Error::from)
    }

    /// Returns the relative path to a file from the root of the repository.
    fn relative_path(uuid: Uuid, file_name: &str) -> String {
        let uuid = uuid.to_string();
        format!("{}/{}/{}", &uuid[0..2], uuid.replace('-', ""), file_name)
    }

    fn stage_file_name(&self, stage: blert::Stage) -> &str {
        match stage {
            blert::Stage::UnknownStage => todo!(),
            blert::Stage::TobMaiden => "maiden",
            blert::Stage::TobBloat => "bloat",
            blert::Stage::TobNylocas => "nylocas",
            blert::Stage::TobSotetseg => "sotetseg",
            blert::Stage::TobXarpus => "xarpus",
            blert::Stage::TobVerzik => "verzik",
            blert::Stage::CoxTekton => todo!(),
            blert::Stage::CoxCrabs => todo!(),
            blert::Stage::CoxIceDemon => todo!(),
            blert::Stage::CoxShamans => todo!(),
            blert::Stage::CoxVanguards => todo!(),
            blert::Stage::CoxThieving => todo!(),
            blert::Stage::CoxVespula => todo!(),
            blert::Stage::CoxTightrope => todo!(),
            blert::Stage::CoxGuardians => todo!(),
            blert::Stage::CoxVasa => todo!(),
            blert::Stage::CoxMystics => todo!(),
            blert::Stage::CoxMuttadile => todo!(),
            blert::Stage::CoxOlm => todo!(),
            blert::Stage::ToaApmeken => todo!(),
            blert::Stage::ToaBaba => todo!(),
            blert::Stage::ToaScabaras => todo!(),
            blert::Stage::ToaKephri => todo!(),
            blert::Stage::ToaHet => todo!(),
            blert::Stage::ToaAkkha => todo!(),
            blert::Stage::ToaCrondis => todo!(),
            blert::Stage::ToaZebak => todo!(),
            blert::Stage::ToaWardens => todo!(),
            blert::Stage::ColosseumWave1 => "wave-1",
            blert::Stage::ColosseumWave2 => "wave-2",
            blert::Stage::ColosseumWave3 => "wave-3",
            blert::Stage::ColosseumWave4 => "wave-4",
            blert::Stage::ColosseumWave5 => "wave-5",
            blert::Stage::ColosseumWave6 => "wave-6",
            blert::Stage::ColosseumWave7 => "wave-7",
            blert::Stage::ColosseumWave8 => "wave-8",
            blert::Stage::ColosseumWave9 => "wave-9",
            blert::Stage::ColosseumWave10 => "wave-10",
            blert::Stage::ColosseumWave11 => "wave-11",
            blert::Stage::ColosseumWave12 => "wave-12",
        }
    }
}

#[derive(Debug)]
pub enum Error {
    NotFound(String),
    Backend(String),
    Decode(prost::DecodeError),
}

impl From<prost::DecodeError> for Error {
    fn from(err: prost::DecodeError) -> Self {
        Error::Decode(err)
    }
}

#[async_trait::async_trait]
pub trait Backend {
    async fn read_file(&self, relative_path: String) -> Result<Vec<u8>, Error>;
}

#[derive(Debug)]
pub struct FilesystemBackend {
    root: PathBuf,
}

impl FilesystemBackend {
    pub fn new(root: &Path) -> Self {
        Self {
            root: root.to_owned(),
        }
    }
}

#[async_trait::async_trait]
impl Backend for FilesystemBackend {
    async fn read_file(&self, relative_path: String) -> Result<Vec<u8>, Error> {
        let full_path = self.root.join(relative_path);
        fs::read(&full_path).map_err(|_| Error::NotFound(full_path.to_string_lossy().into()))
    }
}

#[derive(Debug)]
pub struct S3Backend {
    bucket: String,
    client: aws_sdk_s3::Client,
}

impl S3Backend {
    pub async fn init(endpoint: &str, bucket: &str) -> Self {
        let config = aws_config::defaults(aws_config::BehaviorVersion::latest())
            .endpoint_url(endpoint)
            .load()
            .await;
        let client = aws_sdk_s3::Client::new(&config);

        Self {
            bucket: bucket.to_owned(),
            client,
        }
    }
}

#[async_trait::async_trait]
impl Backend for S3Backend {
    async fn read_file(&self, relative_path: String) -> Result<Vec<u8>, Error> {
        let object = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(&relative_path)
            .send()
            .await
            .map_err(|_| Error::NotFound(relative_path))?;

        let object = object
            .body
            .collect()
            .await
            .map_err(|e| Error::Backend(e.to_string()))?;
        Ok(object.to_vec())
    }
}
