use std::env;

use data_repository::{DataRepository, FilesystemBackend};

mod analyzers;
mod data_repository;

mod blert {
    include!(concat!(env!("OUT_DIR"), "/blert.rs"));
}

#[tokio::main]
async fn main() {
    let uuid = std::env::args()
        .nth(1)
        .expect("expected URI as first argument");

    let repository = initialize_data_repository().expect("Failed to initialize data repository");

    let challenge_data = repository
        .load_challenge(&uuid)
        .await
        .expect("Failed to load challenge");

    println!("Loaded challenge data for {}", challenge_data.challenge_id);
    match challenge_data.stage_data {
        Some(blert::challenge_data::StageData::TobRooms(rooms)) => {
            println!("  Type: Theatre of Blood");
            if let Some(maiden) = rooms.maiden {
                let events = repository
                    .load_stage_events(&uuid, maiden.stage())
                    .await
                    .unwrap();
                println!("  Maiden: yes ({})", events.events.len());
            }
            if let Some(bloat) = rooms.bloat {
                let events = repository
                    .load_stage_events(&uuid, bloat.stage())
                    .await
                    .unwrap();
                println!("  Bloat: yes ({})", events.events.len());
            }
            if let Some(nylocas) = rooms.nylocas {
                let events = repository
                    .load_stage_events(&uuid, nylocas.stage())
                    .await
                    .unwrap();
                println!("  Nylocas: yes ({})", events.events.len());
            }
            if let Some(sotetseg) = rooms.sotetseg {
                let events = repository
                    .load_stage_events(&uuid, sotetseg.stage())
                    .await
                    .unwrap();
                println!("  Sotetseg: yes ({})", events.events.len());
            }
            if let Some(xarpus) = rooms.xarpus {
                let events = repository
                    .load_stage_events(&uuid, xarpus.stage())
                    .await
                    .unwrap();
                println!("  Xarpus: yes ({})", events.events.len());
            }
            if let Some(verzik) = rooms.verzik {
                let events = repository
                    .load_stage_events(&uuid, verzik.stage())
                    .await
                    .unwrap();
                println!("  Verzik: yes ({})", events.events.len());
            }
        }
        Some(blert::challenge_data::StageData::Colosseum(_)) => println!("  Type: Colosseum"),
        None => todo!(),
    }
}

fn initialize_data_repository() -> Result<data_repository::DataRepository, ()> {
    let uri = env::var("BLERT_DATA_REPOSITORY").expect("BLERT_DATA_REPOSITORY not set");

    let backend = match uri.split_once("://") {
        Some(("file", path)) => FilesystemBackend::new(std::path::Path::new(path)),
        Some(("s3", _)) => unimplemented!(),
        Some((_, _)) => panic!("Invalid data repository URI"),
        None => panic!("Invalid data repository URI"),
    };

    Ok(DataRepository::new(Box::new(backend)))
}
