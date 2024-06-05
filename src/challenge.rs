use futures::future;
use uuid::Uuid;

use crate::{
    blert,
    data_repository::DataRepository,
    error::{Error, Result},
};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Status {
    InProgress = 0,
    Completed = 1,
    Reset = 2,
    Wiped = 3,
}

impl std::fmt::Display for Status {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Status::InProgress => write!(f, "In Progress"),
            Status::Completed => write!(f, "Completed"),
            Status::Wiped => write!(f, "Wiped"),
            Status::Reset => write!(f, "Reset"),
        }
    }
}

impl TryFrom<i16> for Status {
    type Error = Error;

    fn try_from(value: i16) -> Result<Self> {
        match value {
            0 => Ok(Status::InProgress),
            1 => Ok(Status::Completed),
            2 => Ok(Status::Wiped),
            3 => Ok(Status::Reset),
            _ => Err(Error::InvalidField("status".to_string())),
        }
    }
}

#[derive(Debug)]
pub struct Challenge {
    uuid: Uuid,
    status: Status,
    stage: blert::Stage,
    party: Vec<String>,

    initial_stage: blert::Stage,
    data: blert::ChallengeData,
    stages: Vec<blert::ChallengeEvents>,
}

impl Challenge {
    /// Loads information about the challenge identified by `uuid` from both the database and a
    /// Blert data repository.
    pub async fn load(
        pool: &sqlx::PgPool,
        repository: &DataRepository,
        uuid: Uuid,
    ) -> Result<Self> {
        let challenge = sqlx::query!("SELECT * FROM challenges WHERE uuid = $1", uuid)
            .fetch_one(pool)
            .await?;

        let challenge_players = sqlx::query!(
            "
            SELECT username
            FROM challenge_players
            WHERE challenge_id = $1
            ORDER BY orb ASC
            ",
            challenge.id,
        )
        .fetch_all(pool)
        .await?;

        let challenge_data = repository.load_challenge(uuid).await?;

        let first_stage = match blert::Challenge::try_from(i32::from(challenge.r#type)) {
            Ok(blert::Challenge::Tob) => blert::Stage::TobMaiden as i16,
            Ok(blert::Challenge::Colosseum) => blert::Stage::ColosseumWave1 as i16,
            Ok(_) => unimplemented!(),
            Err(_) => return Err(Error::InvalidField("type".to_string())),
        };

        let challenge_stage = challenge
            .stage
            .ok_or(Error::InvalidField("stage".to_string()))
            .and_then(|s| {
                blert::Stage::try_from(i32::from(s))
                    .map_err(|_| Error::InvalidField("stage".to_string()))
            })?;

        let stages = future::try_join_all((first_stage..=challenge_stage as i16).map(|stage| {
            let stage =
                blert::Stage::try_from(i32::from(stage)).expect("Stage is within the valid range");
            repository.load_stage_events(uuid, stage)
        }))
        .await?;

        Ok(Challenge {
            uuid,
            status: challenge
                .status
                .ok_or(Error::InvalidField("status".to_string()))
                .and_then(Status::try_from)?,
            stage: challenge_stage,
            party: challenge_players.into_iter().map(|p| p.username).collect(),
            initial_stage: blert::Stage::try_from(i32::from(first_stage)).expect("Stage is valid"),
            data: challenge_data,
            stages,
        })
    }

    /// Returns the ID of the challenge.
    pub fn uuid(&self) -> Uuid {
        self.uuid
    }

    /// Returns the status of the challenge.
    pub fn status(&self) -> Status {
        self.status
    }

    /// Returns the list of players in the challenge, in orb order.
    pub fn party(&self) -> &[String] {
        self.party.as_slice()
    }

    /// Returns an iterator over the stages of the challenge.
    pub fn stages(&self) -> impl Iterator<Item = blert::Stage> {
        (self.initial_stage as i32..=self.stage as i32)
            .map(|s| blert::Stage::try_from(s).expect("Challenge stages are valid"))
    }

    /// Gets the events for a specific stage of the challenge, if they exist.
    pub fn stage_events(&self, stage: blert::Stage) -> Option<&[blert::Event]> {
        let index = stage as usize - self.initial_stage as usize;
        self.stages.get(index).map(|s| s.events.as_slice())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn status_from_i16_valid() {
        use super::Status;
        use std::convert::TryFrom;

        assert_eq!(Status::try_from(0).unwrap(), Status::InProgress);
        assert_eq!(Status::try_from(1).unwrap(), Status::Completed);
        assert_eq!(Status::try_from(2).unwrap(), Status::Wiped);
        assert_eq!(Status::try_from(3).unwrap(), Status::Reset);
    }

    #[test]
    fn status_from_i16_invalid() {
        use super::Status;
        use std::convert::TryFrom;

        assert!(Status::try_from(-1).is_err());
        assert!(Status::try_from(4).is_err());
        assert!(Status::try_from(100).is_err());
        assert!(Status::try_from(i16::MAX).is_err());
        assert!(Status::try_from(i16::MIN).is_err());
    }
}
