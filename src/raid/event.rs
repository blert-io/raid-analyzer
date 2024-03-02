use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::{EquipmentSlot, Item, MaidenCrab, Room, SkillLevel};

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BaseEvent {
    pub raid_id: String,
    pub tick: i32,
    pub x_coord: i32,
    pub y_coord: i32,
    pub room: Room,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Player {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hitpoints: Option<SkillLevel>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub equipment: Option<HashMap<EquipmentSlot, Item>>,
    pub off_cooldown_tick: i32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PlayerEvent {
    #[serde(flatten)]
    pub base: BaseEvent,
    pub player: Player,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PlayerAttack {
    BgsSmack,
    BgsSpec,
    Blowpipe,
    Bowfa,
    ChallySwipe,
    ChallySpec,
    ChinBlack,
    ChinGrey,
    ChinRed,
    ClawScratch,
    ClawSpec,
    DawnSpec,
    DinhsSpec,
    Fang,
    HammerBop,
    HammerSpec,
    HamJoint,
    KodaiBarrage,
    KodaiBash,
    Saeldor,
    Sang,
    SangBarrage,
    SceptreBarrage,
    Scythe,
    ScytheUncharged,
    Shadow,
    ShadowBarrage,
    SotdBarrage,
    StaffOfLightBarrage,
    StaffOfLightSwipe,
    Swift,
    TentWhip,
    ToxicTrident,
    ToxicTridentBarrage,
    ToxicStaffBarrage,
    ToxicStaffSwipe,
    Trident,
    TridentBarrage,
    TwistedBow,
    Zcb,
    UnknownBow,
    UnknownBarrage,
    UnknownPoweredStaff,
    Unknown,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Attack {
    #[serde(rename = "type")]
    attack_type: PlayerAttack,
    weapon: Item,
    #[serde(skip_serializing_if = "Option::is_none")]
    target: Option<NpcIds>,
    distance_to_target: i32,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NpcIds {
    pub id: i32,
    room_id: f64,
}

impl NpcIds {
    pub fn room_id(&self) -> u64 {
        self.room_id as u64
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Npc {
    Basic {
        #[serde(flatten)]
        ids: NpcIds,
        hitpoints: SkillLevel,
    },
    MaidenCrab {
        #[serde(flatten)]
        ids: NpcIds,
        hitpoints: SkillLevel,
        #[serde(rename = "maidenCrab")]
        crab: MaidenCrab,
    },
}

#[derive(Debug, Serialize, Deserialize)]
pub struct NpcEvent {
    #[serde(flatten)]
    pub base: BaseEvent,
    pub npc: Npc,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Coords {
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE", tag = "type")]
pub enum Event {
    PlayerUpdate(PlayerEvent),
    PlayerDeath(PlayerEvent),
    PlayerAttack {
        #[serde(flatten)]
        base: BaseEvent,
        player: Player,
        attack: Attack,
    },
    NpcSpawn(NpcEvent),
    NpcUpdate(NpcEvent),
    NpcDeath(NpcEvent),
    NpcAttack {
        #[serde(flatten)]
        base: BaseEvent,
        npc: NpcIds,
    },
    #[serde(rename_all = "camelCase")]
    MaidenBloodSplats {
        #[serde(flatten)]
        base: BaseEvent,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        maiden_blood_splats: Vec<Coords>,
    },
    MaidenCrabLeak(NpcEvent),
}
