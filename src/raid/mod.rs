use serde::{Deserialize, Serialize};

pub use event::Event;

pub mod event;

pub struct Raid {
    pub id: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Room {
    Maiden,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Skill {
    Hitpoints,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SkillLevel {
    pub skill: Skill,
    pub base: i32,
    pub current: i32,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EquipmentSlot {
    Head,
    Cape,
    Amulet,
    Ammo,
    Weapon,
    Torso,
    Shield,
    Legs,
    Gloves,
    Boots,
    Ring,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Item {
    pub id: i32,
    pub name: String,
    pub quantity: i32,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MaidenCrabSpawn {
    Seventies,
    Fifties,
    Thirties,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MaidenCrabPosition {
    N1,
    N2,
    N3,
    N4Inner,
    N4Outer,
    S1,
    S2,
    S3,
    S4Inner,
    S4Outer,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MaidenCrab {
    spawn: MaidenCrabSpawn,
    position: MaidenCrabPosition,
    scuffed: bool,
}
