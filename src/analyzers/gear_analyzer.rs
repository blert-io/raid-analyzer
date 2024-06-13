use std::collections::HashMap;
use std::sync::Arc;

use crate::analysis::{Analyzer, Context};
use crate::error::{Error, Result};
use crate::item::{EquipmentSlot, Item};
use crate::{blert, item};

/// A `GearAnalyzer` determines what gear each player has in each stage of a challenge.
pub struct GearAnalyzer {}

impl GearAnalyzer {
    pub fn new() -> Self {
        Self {}
    }
}

#[derive(Debug)]
struct GearInfo {
    items_by_stage: HashMap<blert::Stage, HashMap<i32, Arc<Item>>>,
    has_void: bool,
}

#[derive(Debug)]
pub struct PlayerGear {
    players: HashMap<String, GearInfo>,
}

impl PlayerGear {
    /// Returns gear information for the specified player.
    pub fn player<'a>(&'a self, username: &str) -> Option<Player<'a>> {
        self.players.get(username).map(|gear| Player { gear })
    }
}

/// Holds information about the gear a player owns during a challenge.
/// Gear ownership is split by stage, as players may trade items between stages.
#[derive(Debug)]
pub struct Player<'a> {
    gear: &'a GearInfo,
}

impl<'a> Player<'a> {
    /// Returns whether the player has an item with the given ID during the specified stage.
    pub fn has(&self, stage: blert::Stage, item_id: i32) -> bool {
        self.gear
            .items_by_stage
            .get(&stage)
            .is_some_and(|gear| gear.contains_key(&item_id))
    }

    /// Returns whether the player has an item with any of the given IDs during the specified stage.
    pub fn has_any(&self, stage: blert::Stage, item_ids: &[i32]) -> bool {
        self.gear
            .items_by_stage
            .get(&stage)
            .is_some_and(|gear| item_ids.iter().any(|id| gear.contains_key(id)))
    }

    /// Returns whether the player has an item with the given ID during any stage of the challenge.
    pub fn has_in_challenge(&self, item_id: i32) -> bool {
        self.gear
            .items_by_stage
            .values()
            .any(|gear| gear.contains_key(&item_id))
    }

    /// Returns whether the player has an item with any of the given IDs during any stage of the challenge.
    pub fn has_any_in_challenge(&self, item_ids: &[i32]) -> bool {
        self.gear
            .items_by_stage
            .values()
            .any(|gear| item_ids.iter().any(|id| gear.contains_key(id)))
    }

    /// Returns whether the player has Void gear of the specified style.
    /// As Void is untradeable, specifying a stage is unnecessary.
    pub fn has_void(&self, style: item::VoidStyle) -> bool {
        let items = match style {
            item::VoidStyle::Mage => vec![item::Id::VOID_MAGE_HELM, item::Id::VOID_MAGE_HELM_OR],
            item::VoidStyle::Ranged => {
                vec![item::Id::VOID_RANGER_HELM, item::Id::VOID_RANGER_HELM_OR]
            }
            item::VoidStyle::Melee => vec![item::Id::VOID_MELEE_HELM, item::Id::VOID_MELEE_HELM_OR],
            item::VoidStyle::Any => return self.gear.has_void,
        };

        self.has_any_in_challenge(&items)
    }
}

impl Analyzer for GearAnalyzer {
    type Output = PlayerGear;

    fn name(&self) -> &str {
        "GearAnalyzer"
    }

    fn analyze(&self, context: &Context) -> Result<Self::Output> {
        let mut players = HashMap::new();

        let challenge = context.challenge();

        for player in challenge.party() {
            let mut items_by_stage = HashMap::new();
            let mut has_void = false;

            for stage in challenge.stage_infos() {
                let mut gear = HashMap::new();

                let state = stage.player_state(player).ok_or(Error::IncompleteData)?;
                state.iter().for_each(|s| {
                    EquipmentSlot::iter()
                        .filter_map(|slot| {
                            s.equipped_item(slot)
                                .and_then(|item| context.item_registry().get(item.id()))
                        })
                        .for_each(|item| {
                            gear.insert(item.id, item.clone());
                            has_void |= item::is_void(item.id);
                        });
                });

                items_by_stage.insert(stage.stage(), gear);
            }

            players.insert(
                player.clone(),
                GearInfo {
                    items_by_stage,
                    has_void,
                },
            );
        }

        Ok(PlayerGear { players })
    }
}
