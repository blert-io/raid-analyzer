use std::collections::HashMap;

use futures::future::{self, FutureExt};
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

    data: blert::ChallengeData,
    stages: Vec<StageInfo>,
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
            repository
                .load_stage_events(uuid, stage)
                .map(|res| res.map_err(Error::from).and_then(StageInfo::new))
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
    pub fn stages(&self) -> impl Iterator<Item = blert::Stage> + '_ {
        self.stages.iter().map(|info| info.stage)
    }

    /// Returns all stage data for the challenge.
    pub fn stage_infos(&self) -> &[StageInfo] {
        self.stages.as_slice()
    }

    // Gets the data for a specific stage of the challenge, if it exists.
    pub fn stage_info(&self, stage: blert::Stage) -> Option<&StageInfo> {
        self.stages.iter().find(|&info| info.stage == stage)
    }
}

#[derive(Debug)]
pub struct StageInfo {
    stage: blert::Stage,
    events_by_tick: Vec<Vec<blert::Event>>,
    total_events: usize,
    player_state: HashMap<String, Vec<Option<PlayerState>>>,
}

fn is_player_event(event: &blert::Event) -> bool {
    matches!(
        event.r#type(),
        blert::event::Type::PlayerAttack
            | blert::event::Type::PlayerDeath
            | blert::event::Type::PlayerUpdate
    )
}

impl StageInfo {
    fn new(stage_data: blert::ChallengeEvents) -> Result<Self> {
        let stage = stage_data.stage();
        let mut events = stage_data.events;
        events.sort_by(|a, b| a.tick.cmp(&b.tick));
        let last_tick = events.last().map_or(0, |e| e.tick);

        let mut events_by_tick = vec![Vec::new(); last_tick as usize + 1];
        let mut total_events = 0;

        for event in events {
            events_by_tick[event.tick as usize].push(event);
            total_events += 1;
        }

        let player_state = Self::build_player_state(&stage_data.party_names, &events_by_tick)?;

        Ok(Self {
            stage,
            events_by_tick,
            total_events,
            player_state,
        })
    }

    fn build_player_state(
        party: &[String],
        events_by_tick: &Vec<Vec<blert::Event>>,
    ) -> Result<HashMap<String, Vec<Option<PlayerState>>>> {
        let mut player_state = HashMap::new();

        for (index, username) in party.iter().enumerate() {
            let mut state_by_tick = Vec::with_capacity(events_by_tick.len());
            state_by_tick.resize_with(events_by_tick.len(), Default::default);
            let mut last_known_state: Option<&PlayerState> = None;

            for (tick, events) in events_by_tick.iter().enumerate() {
                let mut state_this_tick = match last_known_state {
                    Some(s) => s.next_tick(),
                    None => PlayerState {
                        attack_state: AttackState::Idle,
                        death_state: DeathState::Alive,
                        position: blert::Coords { x: 0, y: 0 },
                        stats: PlayerStats::default(),
                        prayers: PrayerSet::empty(),
                        equipment: Default::default(),
                    },
                };

                events
                    .iter()
                    .filter_map(|e| match (is_player_event(e), &e.player) {
                        (true, Some(player)) if player.party_index as usize == index => {
                            Some((e, player))
                        }
                        (true, None) => {
                            eprintln!("Player event without player data: {e:?}");
                            None
                        }
                        _ => None,
                    })
                    .try_for_each(|(event, player)| match event.r#type() {
                        blert::event::Type::PlayerAttack => {
                            state_this_tick.attack_state = match &event.player_attack {
                                Some(atk) => AttackState::Attacked {
                                    attack: atk.r#type(),
                                    target_id: atk.target.as_ref().map(|npc| npc.room_id),
                                },
                                None => AttackState::Attacked {
                                    attack: blert::PlayerAttack::Unknown,
                                    target_id: None,
                                },
                            };
                            Ok(())
                        }
                        blert::event::Type::PlayerDeath => {
                            state_this_tick.death_state = DeathState::JustDied;
                            Ok(())
                        }
                        blert::event::Type::PlayerUpdate => {
                            if state_this_tick.attack_state  == AttackState::Idle && player.off_cooldown_tick > tick as u32 {
                                state_this_tick.attack_state =
                                    AttackState::OnCooldown(player.off_cooldown_tick - tick as u32);
                            }

                            state_this_tick.position = blert::Coords {
                                x: event.x_coord,
                                y: event.y_coord,
                            };
                            state_this_tick.apply_stats(player);
                            state_this_tick.prayers = player.active_prayers().into();

                            player
                                .equipment_deltas
                                .iter()
                                .map(ItemDelta::try_from)
                                .try_for_each(|delta| match delta {
                                    Ok(delta) => {
                                        state_this_tick.apply_equipment_delta(delta);
                                        Ok(())
                                    }
                                    Err(e) => {
                                        eprintln!("Error parsing item delta: {e}");
                                        Err(Error::InvalidField(format!("PlayerUpdateEvent({username}:{tick}): equipment_deltas")))
                                    }
                                })
                        }
                        _ => unreachable!(),
                    })?;

                state_by_tick[tick] = Some(state_this_tick);
                last_known_state = state_by_tick[tick].as_ref();
            }

            player_state.insert(username.clone(), state_by_tick);
        }

        Ok(player_state)
    }

    pub fn events_for_tick(&self, tick: u16) -> &[blert::Event] {
        self.events_by_tick[tick as usize].as_slice()
    }

    pub fn total_events(&self) -> usize {
        self.total_events
    }

    pub fn player_state(&self, username: &str) -> Option<&[Option<PlayerState>]> {
        self.player_state.get(username).map(Vec::as_slice)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum AttackState {
    Attacked {
        attack: blert::PlayerAttack,
        target_id: Option<u64>,
    },
    OnCooldown(u32),
    Idle,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DeathState {
    Alive,
    JustDied,
    Dead,
}

#[derive(Debug, Clone, Default)]
pub struct PlayerStats {
    attack: Option<SkillLevel>,
    defence: Option<SkillLevel>,
    strength: Option<SkillLevel>,
    hitpoints: Option<SkillLevel>,
    ranged: Option<SkillLevel>,
    prayer: Option<SkillLevel>,
    magic: Option<SkillLevel>,
}

#[derive(Debug, Clone)]
pub struct PlayerState {
    pub attack_state: AttackState,
    pub death_state: DeathState,
    pub position: blert::Coords,
    pub stats: PlayerStats,
    pub prayers: PrayerSet,
    equipment: [Option<Item>; 11],
}

impl PlayerState {
    pub fn equipped_item(&self, slot: EquipmentSlot) -> Option<&Item> {
        self.equipment.get(slot as usize).and_then(Option::as_ref)
    }

    fn next_tick(&self) -> Self {
        Self {
            attack_state: match self.attack_state {
                AttackState::OnCooldown(1) => AttackState::Idle,
                AttackState::OnCooldown(ticks) => AttackState::OnCooldown(ticks - 1),
                _ => AttackState::Idle,
            },
            death_state: match self.death_state {
                DeathState::JustDied => DeathState::Dead,
                _ => DeathState::Alive,
            },
            position: self.position.clone(),
            stats: PlayerStats::default(),
            prayers: self.prayers,
            equipment: self.equipment.clone(),
        }
    }

    fn apply_equipment_delta(&mut self, delta: ItemDelta) {
        match delta {
            ItemDelta::Add(slot, id, quantity) => {
                let index = slot as usize;

                match self.equipment.get_mut(index).and_then(Option::as_mut) {
                    Some(item) if item.id == id => {
                        item.quantity += quantity;
                    }
                    Some(_) | None => {
                        self.equipment[index] = Some(Item { id, quantity });
                    }
                }
            }
            ItemDelta::Remove(slot, id, quantity) => {
                let index = slot as usize;

                match self.equipment.get_mut(index).and_then(Option::as_mut) {
                    Some(item) if item.id == id => {
                        if item.quantity <= quantity {
                            self.equipment[index] = None;
                        } else {
                            item.quantity -= quantity;
                        }
                    }
                    Some(_) | None => {
                        self.equipment[index] = None;
                    }
                }
            }
        }
    }

    fn apply_stats(&mut self, player: &blert::event::Player) {
        if let Some(raw) = player.attack {
            self.stats.attack = Some(raw.into());
        }
        if let Some(raw) = player.defence {
            self.stats.defence = Some(raw.into());
        }
        if let Some(raw) = player.strength {
            self.stats.strength = Some(raw.into());
        }
        if let Some(raw) = player.hitpoints {
            self.stats.hitpoints = Some(raw.into());
        }
        if let Some(raw) = player.ranged {
            self.stats.ranged = Some(raw.into());
        }
        if let Some(raw) = player.prayer {
            self.stats.prayer = Some(raw.into());
        }
        if let Some(raw) = player.magic {
            self.stats.magic = Some(raw.into());
        }
    }
}

#[derive(Debug, Clone)]
pub struct Item {
    pub id: i32,
    pub quantity: i32,
}

#[derive(Debug, Clone)]
pub struct SkillLevel {
    pub base: i16,
    pub current: i16,
}

impl SkillLevel {
    pub fn from_raw(raw: u32) -> Self {
        Self {
            base: raw as i16,
            current: (raw >> 16) as i16,
        }
    }
}

impl From<u32> for SkillLevel {
    fn from(raw: u32) -> Self {
        SkillLevel::from_raw(raw)
    }
}

/// Slots in which a player can equip items.
#[derive(Debug, Clone, PartialEq, Eq)]
#[repr(usize)]
pub enum EquipmentSlot {
    Head = blert::event::player::EquipmentSlot::Head as usize,
    Cape = blert::event::player::EquipmentSlot::Cape as usize,
    Amulet = blert::event::player::EquipmentSlot::Amulet as usize,
    Ammo = blert::event::player::EquipmentSlot::Ammo as usize,
    Weapon = blert::event::player::EquipmentSlot::Weapon as usize,
    Torso = blert::event::player::EquipmentSlot::Torso as usize,
    Shield = blert::event::player::EquipmentSlot::Shield as usize,
    Legs = blert::event::player::EquipmentSlot::Legs as usize,
    Gloves = blert::event::player::EquipmentSlot::Gloves as usize,
    Boots = blert::event::player::EquipmentSlot::Boots as usize,
    Ring = blert::event::player::EquipmentSlot::Ring as usize,
}

impl From<blert::event::player::EquipmentSlot> for EquipmentSlot {
    fn from(slot: blert::event::player::EquipmentSlot) -> Self {
        match slot {
            blert::event::player::EquipmentSlot::Head => EquipmentSlot::Head,
            blert::event::player::EquipmentSlot::Cape => EquipmentSlot::Cape,
            blert::event::player::EquipmentSlot::Amulet => EquipmentSlot::Amulet,
            blert::event::player::EquipmentSlot::Ammo => EquipmentSlot::Ammo,
            blert::event::player::EquipmentSlot::Weapon => EquipmentSlot::Weapon,
            blert::event::player::EquipmentSlot::Torso => EquipmentSlot::Torso,
            blert::event::player::EquipmentSlot::Shield => EquipmentSlot::Shield,
            blert::event::player::EquipmentSlot::Legs => EquipmentSlot::Legs,
            blert::event::player::EquipmentSlot::Gloves => EquipmentSlot::Gloves,
            blert::event::player::EquipmentSlot::Boots => EquipmentSlot::Boots,
            blert::event::player::EquipmentSlot::Ring => EquipmentSlot::Ring,
        }
    }
}

impl TryFrom<u64> for EquipmentSlot {
    type Error = u64;

    fn try_from(value: u64) -> std::result::Result<Self, Self::Error> {
        let value32 = i32::try_from(value).map_err(|_| value)?;
        blert::event::player::EquipmentSlot::try_from(value32)
            .map(Self::from)
            .map_err(|_| value)
    }
}

/// An `ItemDelta` represents a change in the quantity of an item in some
/// container, such as a player's inventory or equipment.
#[derive(Debug, PartialEq, Eq)]
enum ItemDelta {
    Add(EquipmentSlot, i32, i32),
    Remove(EquipmentSlot, i32, i32),
}

impl ItemDelta {
    const QUANTITY_MASK: u64 = 0x0000_0000_7fff_ffff;
    const ADDED_BIT: u64 = 1 << 31;
    const ID_SHIFT: u64 = 32;
    const ID_MASK: u64 = 0xffff;
    const SLOT_SHIFT: u64 = 48;
    const SLOT_MASK: u64 = 0x1f;

    /// Parses an item delta from its packed numeric representation.
    pub fn parse(raw_delta: u64) -> std::result::Result<Self, &'static str> {
        let slot = (raw_delta >> Self::SLOT_SHIFT & Self::SLOT_MASK)
            .try_into()
            .map_err(|_| "Invalid slot")?;
        let id = (raw_delta >> Self::ID_SHIFT & Self::ID_MASK) as i32;
        let quantity = (raw_delta & Self::QUANTITY_MASK) as i32;

        if raw_delta & Self::ADDED_BIT != 0 {
            Ok(Self::Add(slot, id, quantity))
        } else {
            Ok(Self::Remove(slot, id, quantity))
        }
    }
}

impl TryFrom<u64> for ItemDelta {
    type Error = &'static str;

    fn try_from(value: u64) -> std::result::Result<Self, Self::Error> {
        ItemDelta::parse(value)
    }
}

impl TryFrom<&u64> for ItemDelta {
    type Error = &'static str;

    fn try_from(value: &u64) -> std::result::Result<Self, Self::Error> {
        ItemDelta::parse(*value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(u64)]
pub enum Prayer {
    ThickSkin = 0,
    BurstOfStrength = 1,
    ClarityOfThought = 2,
    SharpEye = 3,
    MysticWill = 4,
    RockSkin = 5,
    SuperhumanStrength = 6,
    ImprovedReflexes = 7,
    RapidRestore = 8,
    RapidHeal = 9,
    ProtectItem = 10,
    HawkEye = 11,
    MysticLore = 12,
    SteelSkin = 13,
    UltimateStrength = 14,
    IncredibleReflexes = 15,
    ProtectFromMagic = 16,
    ProtectFromMissiles = 17,
    ProtectFromMelee = 18,
    EagleEye = 19,
    MysticMight = 20,
    Retribution = 21,
    Redemption = 22,
    Smite = 23,
    Preserve = 24,
    Chivalry = 25,
    Piety = 26,
    Rigour = 27,
    Augury = 28,
}

/// Represents a set of prayers that are currently active.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PrayerSet {
    prayers: u64,
}

impl PrayerSet {
    fn from_raw(raw: u64) -> Self {
        Self { prayers: raw }
    }

    pub fn empty() -> Self {
        Self { prayers: 0 }
    }

    pub fn is_active(&self, prayer: Prayer) -> bool {
        self.prayers & (1 << prayer as u64) != 0
    }
}

impl From<u64> for PrayerSet {
    fn from(raw: u64) -> Self {
        PrayerSet::from_raw(raw)
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

    #[test]
    fn item_delta_from_raw() {
        use super::{EquipmentSlot, ItemDelta};

        assert_eq!(
            ItemDelta::parse(0x0000_0000_0000_000f).unwrap(),
            ItemDelta::Remove(EquipmentSlot::Head, 0, 15),
        );
        assert_eq!(
            ItemDelta::parse(0x0000_0000_8000_000f).unwrap(),
            ItemDelta::Add(EquipmentSlot::Head, 0, 15),
        );
        assert_eq!(
            ItemDelta::parse(0x0003_2bd6_8000_718d).unwrap(),
            ItemDelta::Add(EquipmentSlot::Ammo, 11222, 29069),
        );
        assert_eq!(
            ItemDelta::parse(0x0003_2bd6_0000_0001).unwrap(),
            ItemDelta::Remove(EquipmentSlot::Ammo, 11222, 1),
        );
    }
}
