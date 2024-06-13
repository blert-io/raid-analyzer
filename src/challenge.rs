use std::{collections::HashMap, sync::Arc};

use futures::future::{self, FutureExt};
use uuid::Uuid;

use crate::{
    blert,
    data_repository::DataRepository,
    error::{Error, Result},
    item::{self, EquipmentSlot},
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
    r#type: blert::Challenge,
    mode: blert::ChallengeMode,
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

        let r#type = blert::Challenge::try_from(i32::from(challenge.r#type))
            .map_err(|_| Error::InvalidField("type".to_string()))?;

        let mode = challenge
            .mode
            .ok_or_else(|| Error::InvalidField("mode".to_string()))
            .map(i32::from)?;
        let mode = blert::ChallengeMode::try_from(mode)
            .map_err(|_| Error::InvalidField("mode".to_string()))?;

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
            repository.load_stage_events(uuid, stage).map(|res| {
                res.map_err(Error::from)
                    .and_then(|s| StageInfo::new(&challenge_data, s))
            })
        }))
        .await?;

        Ok(Challenge {
            uuid,
            r#type,
            mode,
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

    /// Returns the type of the challenge.
    pub fn r#type(&self) -> blert::Challenge {
        self.r#type
    }

    /// Returns the mode of the challenge.
    pub fn mode(&self) -> blert::ChallengeMode {
        self.mode
    }

    /// Returns the status of the challenge.
    pub fn status(&self) -> Status {
        self.status
    }

    /// Returns the number of players in the challenge.
    pub fn scale(&self) -> usize {
        self.party.len()
    }

    /// Returns the list of players in the challenge, in orb order.
    pub fn party(&self) -> &[String] {
        self.party.as_slice()
    }

    pub fn stage(&self) -> blert::Stage {
        self.stage
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

fn is_player_event(event: &blert::Event) -> bool {
    matches!(
        event.r#type(),
        blert::event::Type::PlayerAttack
            | blert::event::Type::PlayerDeath
            | blert::event::Type::PlayerUpdate
    )
}

#[derive(Debug)]
struct StageEvents {
    total_ticks: u32,
    all: Vec<blert::Event>,
    tick_indices: Vec<i32>,
    by_type: HashMap<blert::event::Type, Vec<usize>>,
}

impl StageEvents {
    pub fn for_tick(&self, tick: u32) -> &[blert::Event] {
        let start_index = self.tick_indices[tick as usize];
        if start_index < 0 {
            return &[];
        }
        let start_index = start_index as usize;

        let end_index = if tick > self.total_ticks {
            self.all.len()
        } else {
            self.tick_indices[tick as usize + 1] as usize
        };

        &self.all[start_index..end_index]
    }
}

#[derive(Debug)]
pub struct StageInfo {
    stage: blert::Stage,
    events: StageEvents,
    player_state: HashMap<String, Vec<Option<PlayerState>>>,
    npcs: HashMap<u64, Arc<blert::challenge_data::StageNpc>>,
}

impl StageInfo {
    fn new(
        challenge_data: &blert::ChallengeData,
        stage_data: blert::ChallengeEvents,
    ) -> Result<Self> {
        let stage = stage_data.stage();
        let mut events = stage_data.events;
        events.sort_by(|a, b| a.tick.cmp(&b.tick));
        let last_tick = events.last().map_or(0, |e| e.tick);

        let mut events = StageEvents {
            total_ticks: last_tick,
            all: events,
            tick_indices: vec![-1; last_tick as usize + 1],
            by_type: HashMap::new(),
        };

        let mut previous_tick = -1;

        for (i, event) in events.all.iter().enumerate() {
            if event.tick as i32 != previous_tick {
                events.tick_indices[event.tick as usize] = i as i32;
                previous_tick = event.tick as i32;
            }

            events.by_type.entry(event.r#type()).or_default().push(i);
        }

        // Pull the raw NPC data for the stage from the proto and convert it to a map of room IDs
        // to NPCs.
        let npcs = challenge_data
            .stage_data
            .as_ref()
            .and_then(|data| match data {
                blert::challenge_data::StageData::TobRooms(rooms) => match stage {
                    blert::Stage::TobMaiden => rooms.maiden.as_ref().map(|r| &r.npcs),
                    blert::Stage::TobBloat => rooms.bloat.as_ref().map(|r| &r.npcs),
                    blert::Stage::TobNylocas => rooms.nylocas.as_ref().map(|r| &r.npcs),
                    blert::Stage::TobSotetseg => rooms.sotetseg.as_ref().map(|r| &r.npcs),
                    blert::Stage::TobXarpus => rooms.xarpus.as_ref().map(|r| &r.npcs),
                    blert::Stage::TobVerzik => rooms.verzik.as_ref().map(|r| &r.npcs),
                    _ => None,
                },
                blert::challenge_data::StageData::Colosseum(colo) => colo
                    .waves
                    .get(stage as usize - blert::Stage::ColosseumWave1 as usize)
                    .map(|wave| &wave.npcs),
            });
        let npcs = npcs
            .map(|npcs| {
                npcs.iter()
                    .map(|npc| (npc.room_id, Arc::new(npc.clone())))
                    .collect::<HashMap<_, _>>()
            })
            .unwrap_or_default();

        let player_state = Self::build_player_state(&stage_data.party_names, &events, &npcs)?;

        Ok(Self {
            stage,
            events,
            player_state,
            npcs,
        })
    }

    fn build_player_state(
        party: &[String],
        events: &StageEvents,
        npcs: &HashMap<u64, Arc<blert::challenge_data::StageNpc>>,
    ) -> Result<HashMap<String, Vec<Option<PlayerState>>>> {
        let mut player_state = HashMap::new();

        for (index, username) in party.iter().enumerate() {
            let mut state_by_tick = Vec::with_capacity(events.total_ticks as usize);
            state_by_tick.resize_with(events.total_ticks as usize, Default::default);
            let mut last_known_state: Option<&PlayerState> = None;

            for tick in 0..events.total_ticks {
                let mut state_this_tick = match last_known_state {
                    Some(s) => s.next_tick(),
                    None => PlayerState {
                        tick,
                        attack_state: AttackState::Idle,
                        death_state: DeathState::Alive,
                        position: blert::Coords { x: 0, y: 0 },
                        stats: PlayerStats::default(),
                        prayers: PrayerSet::empty(),
                        equipment: Default::default(),
                    },
                };

                events
                    .for_tick(tick)
                    .iter()
                    .filter_map(|e| match (is_player_event(e), &e.player) {
                        (true, Some(player)) if player.party_index as usize == index => {
                            Some((e, player))
                        }
                        (true, None) => {
                            log::error!("Player event without player data: {e:?}");
                            None
                        }
                        _ => None,
                    })
                    .try_for_each(|(event, player)| match event.r#type() {
                        blert::event::Type::PlayerAttack => {
                            state_this_tick.attack_state = match &event.player_attack {
                                Some(atk) => AttackState::Attacked(PlayerAttacked {
                                    attack: atk.r#type(),
                                    target: atk.target.as_ref().and_then(|npc| npcs.get(&npc.room_id)).cloned(),
                                }),
                                None => AttackState::Attacked(PlayerAttacked {
                                    attack: blert::PlayerAttack::Unknown,
                                    target: None,
                                }),
                            };
                            Ok(())
                        }
                        blert::event::Type::PlayerDeath => {
                            state_this_tick.death_state = DeathState::JustDied;
                            Ok(())
                        }
                        blert::event::Type::PlayerUpdate => {
                            if state_this_tick.attack_state  == AttackState::Idle && player.off_cooldown_tick > tick {
                                state_this_tick.attack_state =
                                    AttackState::OnCooldown(player.off_cooldown_tick - tick);
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
                                        log::error!("Error parsing item delta: {e}");
                                        Err(Error::InvalidField(format!("PlayerUpdateEvent({username}:{tick}): equipment_deltas")))
                                    }
                                })
                        }
                        _ => unreachable!(),
                    })?;

                state_by_tick[tick as usize] = Some(state_this_tick);
                last_known_state = state_by_tick[tick as usize].as_ref();
            }

            player_state.insert(username.clone(), state_by_tick);
        }

        Ok(player_state)
    }

    /// Returns the challenge stage whose data is contained.
    pub fn stage(&self) -> blert::Stage {
        self.stage
    }

    /// Returns an iterator over every event in the stage.
    pub fn all_events(&self) -> impl Iterator<Item = &blert::Event> {
        self.events.all.iter()
    }

    /// Returns the total number of recorded events in the stage.
    pub fn total_events(&self) -> usize {
        self.events.all.len()
    }

    /// Returns an iterator over all events with the specified type.
    pub fn events_for_type(
        &self,
        event_type: blert::event::Type,
    ) -> impl Iterator<Item = &blert::Event> {
        self.events
            .by_type
            .get(&event_type)
            .into_iter()
            .flat_map(move |indices| indices.iter().map(|&i| &self.events.all[i]))
    }

    /// Returns information about a specific player in the stage.
    pub fn player_state(&self, username: &str) -> Option<PlayerStates> {
        self.player_state
            .get(username)
            .map(|states| PlayerStates { states })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlayerAttacked {
    pub attack: blert::PlayerAttack,
    pub target: Option<Arc<blert::challenge_data::StageNpc>>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AttackState {
    Attacked(PlayerAttacked),
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
pub struct ItemQuantity(i32, i32);

impl ItemQuantity {
    pub fn id(&self) -> i32 {
        self.0
    }

    pub fn quantity(&self) -> i32 {
        self.1
    }
}

#[derive(Debug, Clone, Copy)]
pub struct PlayerStates<'a> {
    states: &'a [Option<PlayerState>],
}

impl PlayerStates<'_> {
    /// Returns an iterator over every known player state. As player state may be missing for some
    /// ticks, the ticks of the iterator may not be sequential.
    pub fn iter(&self) -> impl Iterator<Item = &PlayerState> {
        self.states.iter().flatten()
    }

    /// Returns every attack done by the player with their attack ticks.
    pub fn attacks(&self) -> impl Iterator<Item = (u32, &PlayerAttacked)> {
        self.iter().filter_map(|state| match &state.attack_state {
            AttackState::Attacked(a) => Some((state.tick, a)),
            _ => None,
        })
    }

    /// Returns the player state for a specific tick, if it exists.
    pub fn get_tick(&self, tick: usize) -> Option<&PlayerState> {
        self.states.get(tick).and_then(Option::as_ref)
    }
}

#[derive(Debug, Clone)]
pub struct PlayerState {
    pub tick: u32,
    pub attack_state: AttackState,
    pub death_state: DeathState,
    pub position: blert::Coords,
    pub stats: PlayerStats,
    pub prayers: PrayerSet,
    equipment: [Option<ItemQuantity>; 11],
}

impl PlayerState {
    pub fn equipped_item(&self, slot: EquipmentSlot) -> Option<&ItemQuantity> {
        self.equipment.get(slot as usize).and_then(Option::as_ref)
    }

    pub fn equipment_stats(&self, registry: &item::Registry) -> item::Stats {
        self.equipment
            .iter()
            .fold(item::Stats::default(), |mut acc, item| {
                let Some(stats) = item
                    .as_ref()
                    .and_then(|item| registry.get(item.0))
                    .and_then(|item| item.stats.as_ref())
                else {
                    return acc;
                };

                acc.stab_attack += stats.stab_attack;
                acc.slash_attack += stats.slash_attack;
                acc.crush_attack += stats.crush_attack;
                acc.magic_attack += stats.magic_attack;
                acc.ranged_attack += stats.ranged_attack;
                acc.stab_defence += stats.stab_defence;
                acc.slash_defence += stats.slash_defence;
                acc.crush_defence += stats.crush_defence;
                acc.magic_defence += stats.magic_defence;
                acc.ranged_defence += stats.ranged_defence;
                acc.melee_strength += stats.melee_strength;
                acc.ranged_strength += stats.ranged_strength;
                acc.magic_damage += stats.magic_damage;
                acc.prayer += stats.prayer;
                acc.attack_speed += stats.attack_speed;

                acc
            })
    }

    fn next_tick(&self) -> Self {
        Self {
            tick: self.tick + 1,
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
                    Some(item) if item.0 == id => {
                        item.1 += quantity;
                    }
                    Some(_) | None => {
                        self.equipment[index] = Some(ItemQuantity(id, quantity));
                    }
                }
            }
            ItemDelta::Remove(slot, id, quantity) => {
                let index = slot as usize;

                match self.equipment.get_mut(index).and_then(Option::as_mut) {
                    Some(item) if item.0 == id => {
                        if item.1 <= quantity {
                            self.equipment[index] = None;
                        } else {
                            item.1 -= quantity;
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

/// An `ItemDelta` represents a change in the quantity of an item in some
/// container, such as a player's inventory or equipment.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
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

    pub fn is_active(self, prayer: Prayer) -> bool {
        self.prayers & (1 << prayer as u64) != 0
    }
}

impl From<u64> for PrayerSet {
    fn from(raw: u64) -> Self {
        PrayerSet::from_raw(raw)
    }
}

pub trait PlayerAttackExt {
    fn is_barrage(&self) -> bool;
    fn is_chin(&self) -> bool;
}

impl PlayerAttackExt for blert::PlayerAttack {
    fn is_barrage(&self) -> bool {
        matches!(
            self,
            blert::PlayerAttack::UnknownBarrage
                | blert::PlayerAttack::KodaiBarrage
                | blert::PlayerAttack::NmStaffBarrage
                | blert::PlayerAttack::SangBarrage
                | blert::PlayerAttack::SceptreBarrage
                | blert::PlayerAttack::ShadowBarrage
                | blert::PlayerAttack::SotdBarrage
                | blert::PlayerAttack::ToxicTridentBarrage
                | blert::PlayerAttack::ToxicStaffBarrage
                | blert::PlayerAttack::TridentBarrage
        )
    }

    fn is_chin(&self) -> bool {
        matches!(
            self,
            blert::PlayerAttack::ChinBlack
                | blert::PlayerAttack::ChinGrey
                | blert::PlayerAttack::ChinRed
        )
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
