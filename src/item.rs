use std::collections::HashSet;
use std::fs;
use std::sync::{Arc, OnceLock};
use std::{collections::HashMap, path::Path};

use serde::{Deserialize, Serialize};

use crate::blert;
use crate::error::{Error, Result};

#[derive(Debug, Serialize, Deserialize)]
pub struct Item {
    pub id: i32,
    pub name: String,
    pub tradeable: bool,
    #[serde(default, deserialize_with = "deserialize_slot_opt")]
    pub slot: Option<EquipmentSlot>,
    pub stats: Option<Stats>,
}

impl Item {
    /// Returns whether the item can be equipped.
    pub fn equipable(&self) -> bool {
        self.slot.is_some()
    }
}

impl std::hash::Hash for Item {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

impl std::cmp::PartialEq for Item {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl std::cmp::Eq for Item {}

#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Stats {
    pub stab_attack: i32,
    pub slash_attack: i32,
    pub crush_attack: i32,
    pub magic_attack: i32,
    pub ranged_attack: i32,
    pub stab_defence: i32,
    pub slash_defence: i32,
    pub crush_defence: i32,
    pub magic_defence: i32,
    pub ranged_defence: i32,
    pub melee_strength: i32,
    pub ranged_strength: i32,
    pub magic_damage: i32,
    pub prayer: i32,
    pub attack_speed: i32,
}

/// Slots in which a player can equip items.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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

impl EquipmentSlot {
    const VALUES: [EquipmentSlot; 11] = [
        EquipmentSlot::Head,
        EquipmentSlot::Cape,
        EquipmentSlot::Amulet,
        EquipmentSlot::Ammo,
        EquipmentSlot::Weapon,
        EquipmentSlot::Torso,
        EquipmentSlot::Shield,
        EquipmentSlot::Legs,
        EquipmentSlot::Gloves,
        EquipmentSlot::Boots,
        EquipmentSlot::Ring,
    ];

    pub fn iter() -> impl Iterator<Item = EquipmentSlot> {
        Self::VALUES.iter().copied()
    }

    fn deserialize_i32<'de, D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        match i32::deserialize(deserializer)? {
            0 => Ok(EquipmentSlot::Head),
            1 => Ok(EquipmentSlot::Cape),
            2 => Ok(EquipmentSlot::Amulet),
            3 => Ok(EquipmentSlot::Ammo),
            4 => Ok(EquipmentSlot::Weapon),
            5 => Ok(EquipmentSlot::Torso),
            6 => Ok(EquipmentSlot::Shield),
            7 => Ok(EquipmentSlot::Legs),
            8 => Ok(EquipmentSlot::Gloves),
            9 => Ok(EquipmentSlot::Boots),
            10 => Ok(EquipmentSlot::Ring),
            _ => Err(serde::de::Error::custom("invalid equipment slot")),
        }
    }
}

#[derive(Debug, Deserialize)]
struct WrappedEquipmentSlot(
    #[serde(deserialize_with = "EquipmentSlot::deserialize_i32")] EquipmentSlot,
);

fn deserialize_slot_opt<'de, D>(
    deserializer: D,
) -> std::result::Result<Option<EquipmentSlot>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Option::<WrappedEquipmentSlot>::deserialize(deserializer).map(|opt| opt.map(|w| w.0))
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

/// An `ItemRegistry` is a collection of in-game item definitions.
#[derive(Debug)]
pub struct Registry {
    items: HashMap<i32, Arc<Item>>,
}

impl Registry {
    /// Reads items into a registry from a JSON file.
    pub fn load_from_file(path: impl AsRef<Path>) -> Result<Self> {
        let reader = fs::File::open(path)?;
        let items: Vec<Item> = serde_json::from_reader(reader).map_err(|e| {
            log::error!("Failed to parse items file: {}", e);
            Error::IncompleteData
        })?;

        Ok(Self {
            items: items
                .into_iter()
                .map(|item| (item.id, Arc::new(item)))
                .collect(),
        })
    }

    /// Looks up an item by its ID.
    pub fn get(&self, id: i32) -> Option<&Arc<Item>> {
        self.items.get(&id)
    }
}

#[derive(Debug, Clone, Copy)]
pub enum VoidStyle {
    Mage,
    Ranged,
    Melee,
    Any,
}

/// Returns `true` if the given item ID belongs to any Void piece.
pub fn is_void(id: i32) -> bool {
    static VOID_ITEMS: OnceLock<HashSet<i32>> = OnceLock::new();
    let items = VOID_ITEMS.get_or_init(|| {
        [
            Id::VOID_KNIGHT_TOP,
            Id::VOID_KNIGHT_ROBE,
            Id::VOID_KNIGHT_GLOVES,
            Id::VOID_MAGE_HELM,
            Id::VOID_RANGER_HELM,
            Id::VOID_MELEE_HELM,
            Id::VOID_KNIGHT_TOP_L,
            Id::ELITE_VOID_TOP_L,
            Id::VOID_KNIGHT_ROBE_L,
            Id::ELITE_VOID_ROBE_L,
            Id::VOID_KNIGHT_MACE_L,
            Id::VOID_KNIGHT_GLOVES_L,
            Id::VOID_MAGE_HELM_L,
            Id::VOID_RANGER_HELM_L,
            Id::VOID_MELEE_HELM_L,
            Id::VOID_KNIGHT_TOP_OR,
            Id::VOID_KNIGHT_ROBE_OR,
            Id::VOID_KNIGHT_GLOVES_OR,
            Id::ELITE_VOID_TOP_OR,
            Id::ELITE_VOID_ROBE_OR,
            Id::VOID_MAGE_HELM_OR,
            Id::VOID_RANGER_HELM_OR,
            Id::VOID_MELEE_HELM_OR,
        ]
        .into_iter()
        .collect()
    });

    items.contains(&id)
}

pub struct Id;

// TODO(frolv): Automatically generate these from the JSON dump as a build step.
impl Id {
    pub const VOID_KNIGHT_TOP: i32 = 8839;
    pub const VOID_KNIGHT_ROBE: i32 = 8840;
    pub const VOID_KNIGHT_GLOVES: i32 = 8842;
    pub const VOID_MAGE_HELM: i32 = 11663;
    pub const VOID_RANGER_HELM: i32 = 11664;
    pub const VOID_MELEE_HELM: i32 = 11665;
    pub const GOBLIN_PAINT_CANNON: i32 = 12727;
    pub const DINHS_BULWARK: i32 = 21015;
    pub const HAM_JOINT: i32 = 23360;
    pub const VOID_KNIGHT_TOP_L: i32 = 24177;
    pub const ELITE_VOID_TOP_L: i32 = 24178;
    pub const VOID_KNIGHT_ROBE_L: i32 = 24179;
    pub const ELITE_VOID_ROBE_L: i32 = 24180;
    pub const VOID_KNIGHT_MACE_L: i32 = 24181;
    pub const VOID_KNIGHT_GLOVES_L: i32 = 24182;
    pub const VOID_MAGE_HELM_L: i32 = 24183;
    pub const VOID_RANGER_HELM_L: i32 = 24184;
    pub const VOID_MELEE_HELM_L: i32 = 24185;
    pub const SWIFT_BLADE: i32 = 24219;
    pub const ZARYTE_VAMBRACES: i32 = 26235;
    pub const VOID_KNIGHT_TOP_OR: i32 = 26463;
    pub const VOID_KNIGHT_ROBE_OR: i32 = 26465;
    pub const VOID_KNIGHT_GLOVES_OR: i32 = 26467;
    pub const ELITE_VOID_TOP_OR: i32 = 26469;
    pub const ELITE_VOID_ROBE_OR: i32 = 26471;
    pub const VOID_MAGE_HELM_OR: i32 = 26473;
    pub const VOID_RANGER_HELM_OR: i32 = 26475;
    pub const VOID_MELEE_HELM_OR: i32 = 26477;
    pub const MASORI_MASK: i32 = 27226;
    pub const MASORI_BODY: i32 = 27229;
    pub const MASORI_CHAPS: i32 = 27232;
    pub const MASORI_MASK_F: i32 = 27235;
    pub const MASORI_BODY_F: i32 = 27238;
    pub const MASORI_CHAPS_F: i32 = 27241;
    pub const DINHS_BLAZING_BULWARK: i32 = 28682;
    pub const DUAL_MACUAHUITL: i32 = 28997;
}
