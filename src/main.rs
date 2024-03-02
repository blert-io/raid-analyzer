use crate::raid::{Event, Room};
use futures::{StreamExt, TryStreamExt};
use mongodb::{bson::doc, options::FindOptions, Client};

mod analyzers;
mod raid;

#[tokio::main]
async fn main() {
    let uri = std::env::args()
        .nth(1)
        .expect("expected URI as first argument");

    let client = Client::with_uri_str(&uri).await.expect("Failed to connect");
    let events = client.database("test").collection::<Event>("roomevents");

    let raid_id = std::env::args()
        .nth(2)
        .expect("expected raid ID as second argument");

    let cursor = events
        .find(
            doc! { "raidId": raid_id, "room": "MAIDEN" },
            FindOptions::builder().sort(doc! { "tick": 1 }).build(),
        )
        .await
        .unwrap();

    let maiden_events: Vec<Event> = cursor.try_collect::<Vec<_>>().await.unwrap();
    let event = maiden_events
        .iter()
        .filter_map(|e| match e {
            Event::NpcUpdate(e) => Some(e),
            _ => None,
        })
        .find(|&e| matches!(e.npc, raid::event::Npc::MaidenCrab { .. }));

    if let Some(e) = event {
        println!("{e:?}");
    }
}
