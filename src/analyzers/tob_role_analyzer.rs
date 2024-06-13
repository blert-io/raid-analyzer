use std::{
    cmp::Reverse,
    collections::{HashMap, HashSet},
};

use crate::{
    analysis::Analyzer,
    blert,
    challenge::{Challenge, PlayerAttackExt, PlayerStates, StageInfo},
    error::{Error, Result},
    item,
    npc::NpcExt,
};

use super::gear_analyzer::{self, GearAnalyzer};

/// A well-defined meta role for a player in the Theatre of Blood.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Role {
    Solo,
    DuoMage,
    DuoRanger,
    Mage,
    Ranger,
    Melee,
    MeleeFreeze,
}

impl Role {
    pub fn is_freezer(self) -> bool {
        matches!(self, Role::Mage | Role::MeleeFreeze | Role::DuoMage)
    }
}

/// A role responsibility within a Theatre of Blood room.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubRole {
    MaidenSoloFreezer,
    MaidenNorthFreezer,
    MaidenSouthFreezer,
    MaidenChinner,
    NyloWestMage,
    NyloEastMage,
    NyloWestMelee,
    NyloEastMelee,
}

#[derive(Debug)]
#[allow(dead_code)]
pub struct PlayerRoles(Role, Vec<SubRole>);

#[allow(dead_code)]
impl PlayerRoles {
    pub fn role(&self) -> Role {
        self.0
    }

    pub fn has_sub_role(&self, sub_role: SubRole) -> bool {
        self.1.contains(&sub_role)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MatchCertainty {
    Strong,
    Weak,
    None,
}

type MatchFn =
    fn(Role, blert::ChallengeMode, usize, &PlayerStates, &gear_analyzer::Player) -> MatchCertainty;

#[derive(Debug)]
struct AssignmentContext<'a> {
    /// The raid being analyzed.
    challenge: &'a Challenge,

    /// Roles yet to be assigned.
    roles_to_assign: Vec<Role>,

    /// Players yet to be assigned a role, sorted by the number of roles they could potentially match.
    /// Each tuple consists of (name, number of matching roles).
    unassigned_players: Vec<(&'a String, usize)>,

    /// Players definitively matching a role.
    strong_matches: HashMap<Role, Vec<&'a String>>,

    /// Roles that have potential matches, but are not definitively assigned.
    weak_matches: HashMap<Role, Vec<&'a String>>,

    /// Players who do not match any role due to insufficient information.
    players_not_matching_any_role: Vec<&'a String>,
}

impl AssignmentContext<'_> {
    fn uuid(&self) -> uuid::Uuid {
        self.challenge.uuid()
    }

    fn scale(&self) -> usize {
        self.challenge.scale()
    }
}

#[derive(Debug)]
struct PrimaryRole(String, Role);

/// The `TobRoleAnalyzer` attempts to determine the role of every player within a Theatre of Blood
/// raid.
///
/// Raids which reach the Nylocas can be analyzed with a high degree of accuracy. If a raid ends
/// before Nylocas, the analyzer attempts to assign a role to every player primarily based on their
/// actions at Maiden.
///
/// There are a couple of limitations to this analyzer:
/// - It assumes roles within the current TOB meta, and will fail on non-standard setups.
/// - Likewise, each role is assumed to have specific responsibilities within rooms and alternative
///   strategies are not recognized.
///
/// To simplify downstream usage, the analyzer takes an all-or-nothing approach: if it cannot
/// assign roles to every player, it will fail outright.
pub struct TobRoleAnalyzer {}

impl TobRoleAnalyzer {
    /// The threshold for the number of 4 tick melees a player must have to be considered a meleer
    /// using a 4 tick mainhand weapon. This is set high to avoid false positives from other roles
    /// that may fill ticks with attacks such as claw scratches.
    const MELEE_4T_THRESHOLD: u32 = 12;

    /// Weapons used by meleers in the Nylocas room.
    const NYLO_MELEE_WEAPONS: &'static [i32] = &[
        item::Id::SWIFT_BLADE,
        item::Id::HAM_JOINT,
        item::Id::DUAL_MACUAHUITL,
    ];

    pub fn new() -> Self {
        Self {}
    }

    /// Attempts to assign roles to all players based on room data. If every role is successfully
    /// assigned, returns a map of player names to their roles. Otherwise, returns an error.
    fn determine_roles(
        challenge: &Challenge,
        player_gear: &gear_analyzer::PlayerGear,
    ) -> Result<HashMap<String, PlayerRoles>> {
        let roles_to_assign = match challenge.scale() {
            1 => unreachable!(),
            2 => vec![Role::DuoMage, Role::DuoRanger],
            3 => vec![Role::Mage, Role::Ranger, Role::Melee],
            4 => vec![Role::Mage, Role::MeleeFreeze, Role::Ranger, Role::Melee],
            5 => vec![
                Role::Mage,
                Role::Mage,
                Role::Ranger,
                Role::Melee,
                Role::Melee,
            ],
            _ => return Err(Error::FailedPrecondition("Invalid raid scale".into())),
        };

        let mut ctx = AssignmentContext {
            challenge,
            roles_to_assign,
            unassigned_players: Vec::new(),
            strong_matches: HashMap::new(),
            weak_matches: HashMap::new(),
            players_not_matching_any_role: Vec::new(),
        };

        let mut player_roles = HashMap::new();

        // Do an initial pass counting how many roles each player could potentially match, and sort
        // the party in that order to maximize the chance of successful role assignment.
        // Keep track of the orb to correct the party's order after role assignment.
        Self::find_role_matches(&mut ctx, player_gear)?;

        let mut assigned_roles: Vec<PrimaryRole> = ctx
            .strong_matches
            .iter()
            .flat_map(|(role, players)| {
                players
                    .iter()
                    .map(|&player| PrimaryRole(player.to_owned(), *role))
            })
            .collect();

        // Next, attempt to pigeonhole players who do not match any role into a role based on
        // the raid scale and what roles are left to assign.
        assigned_roles.extend(Self::try_guess_unmatched_roles(&mut ctx, player_gear));

        if ctx.players_not_matching_any_role.len() > 1 {
            log::error!(
                "Cannot assign roles to all players as multiple players do not match any role",
            );
            return Err(Error::IncompleteData);
        }

        assert_eq!(ctx.roles_to_assign.len(), ctx.unassigned_players.len());

        if let Some(roles) = Self::try_assign_roles(
            &mut ctx.roles_to_assign,
            &mut Vec::new(),
            &ctx.unassigned_players,
            &ctx.weak_matches,
        )? {
            assigned_roles.extend(roles);
        } else {
            log::error!("Failed to assign roles to all players");
            return Err(Error::IncompleteData);
        };

        player_roles.extend(assigned_roles.into_iter().map(|PrimaryRole(player, role)| {
            let mut subroles = Vec::new();

            if let Some(maiden_data) = challenge.stage_info(blert::Stage::TobMaiden) {
                let player_state = maiden_data
                    .player_state(&player)
                    .expect("Player state is known to exist");
                subroles.extend(Self::determine_maiden_subroles(
                    challenge,
                    maiden_data,
                    &player_state,
                    role,
                ));
            }
            if let Some(nylo_data) = challenge.stage_info(blert::Stage::TobNylocas) {
                let player_state = nylo_data
                    .player_state(&player)
                    .expect("Player state is known to exist");
                subroles.extend(Self::determine_nylo_subroles(
                    challenge,
                    nylo_data,
                    &player_state,
                    role,
                ));
            }

            (player, PlayerRoles(role, subroles))
        }));

        if player_roles.len() == challenge.scale() {
            Ok(player_roles)
        } else {
            log::error!("Failed to assign roles to all players");
            Err(Error::IncompleteData)
        }
    }

    fn find_role_matches(
        ctx: &mut AssignmentContext,
        player_gear: &gear_analyzer::PlayerGear,
    ) -> Result<()> {
        let (stage_data, match_fn): (&StageInfo, MatchFn) =
            if ctx.challenge.stage() < blert::Stage::TobNylocas {
                log::debug!(
                    "Challenge {}: assigning roles based on Maiden data",
                    ctx.uuid(),
                );
                let maiden_data = ctx
                    .challenge
                    .stage_info(blert::Stage::TobMaiden)
                    .ok_or_else(|| Error::IncompleteData)?;
                (maiden_data, Self::try_match_role_pre_nylo)
            } else {
                log::debug!(
                    "Challenge {}: assigning roles based on Nylocas data",
                    ctx.uuid(),
                );
                let nylo_data = ctx
                    .challenge
                    .stage_info(blert::Stage::TobNylocas)
                    .ok_or_else(|| Error::IncompleteData)?;
                (nylo_data, Self::try_match_role_nylo)
            };

        ctx.challenge.party().iter().try_for_each(|player| {
            let mut player_weak_matches = Vec::new();
            let mut strong_match_index = None;

            let player_state = stage_data
                .player_state(player)
                .ok_or(Error::IncompleteData)?;
            let gear = player_gear.player(player).ok_or(Error::IncompleteData)?;

            for (i, role) in ctx.roles_to_assign.iter().enumerate() {
                match match_fn(
                    *role,
                    ctx.challenge.mode(),
                    ctx.scale(),
                    &player_state,
                    &gear,
                ) {
                    MatchCertainty::Strong => {
                        log::debug!("Definitively matched {player} to {role:?}");
                        ctx.strong_matches.entry(*role).or_default().push(player);
                        strong_match_index = Some(i);
                        break;
                    }
                    MatchCertainty::Weak => {
                        player_weak_matches.push(*role);
                    }
                    MatchCertainty::None => {}
                }
            }

            if let Some(i) = strong_match_index {
                ctx.roles_to_assign.swap_remove(i);
                return Ok::<(), Error>(());
            }

            if player_weak_matches.is_empty() {
                ctx.players_not_matching_any_role.push(player);
            } else {
                for &role in &player_weak_matches {
                    ctx.weak_matches.entry(role).or_default().push(player);
                }
            }

            ctx.unassigned_players
                .push((player, player_weak_matches.len()));
            Ok(())
        })?;

        ctx.unassigned_players
            .sort_by_key(|(_, weak_matches)| Reverse(*weak_matches));

        Ok(())
    }

    fn try_guess_unmatched_roles<'a>(
        ctx: &'a mut AssignmentContext,
        player_gear: &'a gear_analyzer::PlayerGear,
    ) -> Vec<PrimaryRole> {
        let mut assigned_roles = Vec::new();

        if ctx.scale() == 4 && ctx.strong_matches.contains_key(&Role::Mage) {
            // In 4s, if a mage has already been positively matched, an unmatched freezer
            // must be the melee freezer.
            if let Some(players) = ctx.weak_matches.get(&Role::Mage) {
                if players.len() == 1 {
                    let player = players[0];
                    assigned_roles.push(PrimaryRole(player.clone(), Role::MeleeFreeze));
                    ctx.unassigned_players.retain(|(p, _)| *p != player);
                    ctx.roles_to_assign
                        .retain(|role| *role != Role::MeleeFreeze);
                } else {
                    log::warn!(
                        "{}: Multiple weak matches for Mage alongside a strong match",
                        ctx.uuid(),
                    );
                }
            }
        }

        // Freezing roles will always at least weakly match a mage role, so any players without
        // matches at all must either rangers or meleers. If there are two unmatched players, it
        // may be possible to guess their roles.
        if ctx.players_not_matching_any_role.len() != 2 {
            return assigned_roles;
        }

        match ctx.scale() {
            3 | 4 => {
                // In 3s and 4s, one of the unmatched players must be a ranger and the other
                // a melee. Some melee setups will drop Void for other ranged gear, whereas
                // rangers rarely do. Therefore, if one player has Void and the other doesn't,
                // assume the player with Void is the ranger.
                let with_void = ctx
                    .players_not_matching_any_role
                    .iter()
                    .enumerate()
                    .filter_map(|(i, &player)| {
                        player_gear
                            .player(player)
                            .map_or(false, |gear| gear.has_void(item::VoidStyle::Any))
                            .then_some(i)
                    })
                    .collect::<Vec<_>>();

                if with_void.len() != 1 {
                    return assigned_roles;
                }

                let potential_ranger = ctx.players_not_matching_any_role[with_void[0]];
                let potential_melee = ctx.players_not_matching_any_role[1 - with_void[0]];

                let melee_has_non_void_ranged_gear = player_gear
                    .player(potential_melee)
                    .unwrap()
                    .has_any_in_challenge(&[
                        item::Id::ZARYTE_VAMBRACES,
                        item::Id::MASORI_MASK,
                        item::Id::MASORI_BODY,
                        item::Id::MASORI_CHAPS,
                        item::Id::MASORI_MASK_F,
                        item::Id::MASORI_BODY_F,
                        item::Id::MASORI_CHAPS_F,
                    ]);

                if melee_has_non_void_ranged_gear {
                    ctx.unassigned_players
                        .retain(|(p, _)| *p != potential_ranger && *p != potential_melee);
                    ctx.roles_to_assign
                        .retain(|role| *role != Role::Ranger && *role != Role::Melee);
                    ctx.players_not_matching_any_role.clear();
                    assigned_roles.push(PrimaryRole(potential_ranger.to_string(), Role::Ranger));
                    assigned_roles.push(PrimaryRole(potential_melee.to_string(), Role::Melee));
                }
            }
            5 => {
                // In 5s there are two melees, so if a potential ranger has already been
                // positively matched, the two remaining players must be melees.
                if ctx.strong_matches.contains_key(&Role::Ranger)
                    || ctx.weak_matches.contains_key(&Role::Ranger)
                {
                    ctx.players_not_matching_any_role.drain(..).for_each(|p| {
                        assigned_roles.push(PrimaryRole(p.to_string(), Role::Melee));
                        ctx.unassigned_players.retain(|(player, _)| *player != p);
                    });
                    ctx.roles_to_assign.retain(|role| *role != Role::Melee);
                }
            }
            _ => {}
        };

        assigned_roles
    }

    /// Recursively attempts to assign a role to every player based on their weak potential
    /// matches, by giving roles to players and backtracking if not all roles can be assigned.
    ///
    /// The party is assumed to be sorted by the number of weak matches each player has.
    fn try_assign_roles(
        roles_to_assign: &mut [Role],
        roles_assigned: &mut Vec<PrimaryRole>,
        unassigned_players: &[(&String, usize)],
        weak_matches: &HashMap<Role, Vec<&String>>,
    ) -> Result<Option<Vec<PrimaryRole>>> {
        if roles_to_assign.is_empty() {
            return Ok(Some(std::mem::take(roles_assigned)));
        }

        let (player, _) = unassigned_players[roles_assigned.len()];

        if roles_to_assign.len() == 1 {
            // If there's only one role left to assign, assume it belongs to the last player.
            log::debug!("Assigning final role {:?} to {player}", roles_to_assign[0]);
            roles_assigned.push(PrimaryRole(player.to_string(), roles_to_assign[0]));
            return Ok(Some(std::mem::take(roles_assigned)));
        }

        for i in 0..roles_to_assign.len() {
            let role = roles_to_assign[i];

            let player_matches_role = weak_matches
                .get(&role)
                .map_or(false, |players| players.contains(&player));

            if !player_matches_role {
                log::debug!("{player} does not match role {role:?}");
                continue;
            }

            log::debug!("Potentially assigning role {role:?} to {player}");

            roles_assigned.push(PrimaryRole(player.to_string(), role));
            roles_to_assign.swap(0, i);

            match Self::try_assign_roles(
                &mut roles_to_assign[1..],
                roles_assigned,
                unassigned_players,
                weak_matches,
            )? {
                Some(assigned_roles) => return Ok(Some(assigned_roles)),
                None => {
                    log::debug!("Failed to assign role {role:?} to {player}");
                }
            }

            roles_to_assign.swap(i, 0);
            roles_assigned.pop();
        }

        Ok(None)
    }

    fn try_match_role_pre_nylo(
        role: Role,
        mode: blert::ChallengeMode,
        scale: usize,
        player_state: &PlayerStates,
        player_gear: &gear_analyzer::Player,
    ) -> MatchCertainty {
        let mut has_barraged = false;
        let mut has_chinned = false;
        let mut has_dinhs = false;

        player_state
            .attacks()
            .filter(|(_, atk)| atk.target.as_ref().is_some_and(|t| t.is_maiden_matomenos()))
            .for_each(|(_, atk)| {
                if atk.attack.is_barrage() {
                    has_barraged = true;
                } else if atk.attack.is_chin() {
                    has_chinned = true;
                } else if atk.attack == blert::PlayerAttack::DinhsSpec
                    || atk.attack == blert::PlayerAttack::DinhsBash
                {
                    has_dinhs = true;
                }
            });

        let has_melee_weapon = player_gear.has_any_in_challenge(Self::NYLO_MELEE_WEAPONS);
        has_dinhs = has_dinhs
            || player_gear.has_any(
                blert::Stage::TobMaiden,
                &[item::Id::DINHS_BULWARK, item::Id::DINHS_BLAZING_BULWARK],
            );

        let is_hmt = mode == blert::ChallengeMode::TobHard;

        match role {
            Role::DuoMage => {
                if has_barraged || has_melee_weapon {
                    MatchCertainty::Strong
                } else {
                    MatchCertainty::None
                }
            }
            Role::DuoRanger => {
                if has_chinned {
                    MatchCertainty::Strong
                } else if !has_barraged {
                    MatchCertainty::Weak
                } else {
                    MatchCertainty::None
                }
            }
            Role::Mage => {
                if has_barraged {
                    if has_chinned || (scale == 3 && !is_hmt) || scale == 5 {
                        MatchCertainty::Strong
                    } else if !has_melee_weapon {
                        MatchCertainty::Weak
                    } else {
                        MatchCertainty::None
                    }
                } else {
                    MatchCertainty::None
                }
            }
            Role::Ranger => {
                if has_chinned && !has_barraged {
                    MatchCertainty::Weak
                } else {
                    MatchCertainty::None
                }
            }
            Role::Melee => {
                if has_dinhs || (has_melee_weapon && !has_barraged) {
                    MatchCertainty::Strong
                } else if is_hmt && has_melee_weapon && has_barraged {
                    // HMT trios typically have the meleer freeze at Maiden as well.
                    MatchCertainty::Weak
                } else {
                    MatchCertainty::None
                }
            }
            Role::MeleeFreeze => {
                if has_barraged && has_melee_weapon {
                    MatchCertainty::Strong
                } else {
                    MatchCertainty::None
                }
            }
            Role::Solo => MatchCertainty::Strong,
        }
    }

    fn try_match_role_nylo(
        role: Role,
        _mode: blert::ChallengeMode,
        scale: usize,
        player_state: &PlayerStates,
        player_gear: &gear_analyzer::Player,
    ) -> MatchCertainty {
        use blert::PlayerAttack;

        // TOOD(frolv): This currently only counts types of attacks. It could be made much more
        // accurate by considering what Nylos were targeted.

        let mut num_swifts = 0;
        let mut num_pipes = 0;
        let mut num_4t_melees = 0;
        let mut has_barraged = false;
        let mut has_chinned = false;

        player_state.attacks().for_each(|(_, atk)| {
            match atk.attack {
                PlayerAttack::SwiftBlade
                | PlayerAttack::HamJoint
                | PlayerAttack::DualMacuahuitl => {
                    num_swifts += 1;
                }
                PlayerAttack::ClawScratch | PlayerAttack::TentWhip => {
                    num_4t_melees += 1;
                }
                PlayerAttack::Blowpipe | PlayerAttack::BlowpipeSpec => {
                    num_pipes += 1;
                }
                attack if attack.is_barrage() => has_barraged = true,
                attack if attack.is_chin() => has_chinned = true,
                _ => (),
            };
        });

        let has_meleed = num_swifts > 1 || num_4t_melees > Self::MELEE_4T_THRESHOLD;
        let has_paint_cannon =
            player_gear.has(blert::Stage::TobNylocas, item::Id::GOBLIN_PAINT_CANNON);

        match role {
            Role::Solo => MatchCertainty::Strong,
            Role::DuoMage => {
                if has_barraged || has_meleed {
                    MatchCertainty::Strong
                } else {
                    MatchCertainty::None
                }
            }
            Role::DuoRanger => {
                if num_pipes > 30 {
                    MatchCertainty::Strong
                } else if !has_meleed {
                    MatchCertainty::Weak
                } else {
                    MatchCertainty::None
                }
            }
            Role::Mage => {
                if has_barraged {
                    if scale == 4 {
                        MatchCertainty::Weak
                    } else {
                        MatchCertainty::Strong
                    }
                } else {
                    MatchCertainty::None
                }
            }
            Role::MeleeFreeze => {
                if scale == 4 && has_barraged {
                    if has_meleed {
                        MatchCertainty::Strong
                    } else if has_paint_cannon {
                        MatchCertainty::Weak
                    } else {
                        MatchCertainty::None
                    }
                } else {
                    MatchCertainty::None
                }
            }
            Role::Ranger => {
                if has_chinned {
                    MatchCertainty::Strong
                } else if num_pipes > 20 {
                    MatchCertainty::Weak
                } else {
                    MatchCertainty::None
                }
            }
            Role::Melee => {
                if has_meleed || has_paint_cannon {
                    MatchCertainty::Weak
                } else {
                    MatchCertainty::None
                }
            }
        }
    }

    fn determine_maiden_subroles(
        challenge: &Challenge,
        maiden_data: &StageInfo,
        player_state: &PlayerStates,
        role: Role,
    ) -> Vec<SubRole> {
        let mut subroles = Vec::new();

        if challenge.scale() > 2 && role.is_freezer() {
            // Count how many players froze crabs at Maiden.
            let num_freezers = maiden_data
                .events_for_type(blert::event::Type::PlayerAttack)
                .filter_map(|event| {
                    event.player_attack.as_ref().and_then(|attack| {
                        if attack.r#type().is_barrage()
                            && attack
                                .target
                                .as_ref()
                                .is_some_and(NpcExt::is_maiden_matomenos)
                        {
                            event.player.as_ref().map(|p| p.party_index)
                        } else {
                            None
                        }
                    })
                })
                .collect::<HashSet<_>>()
                .len();

            if num_freezers == 1 {
                subroles.push(SubRole::MaidenSoloFreezer);
            } else {
                let (north_freezes, south_freezes) =
                    Self::count_north_and_south_freezes(player_state);

                if north_freezes > south_freezes {
                    subroles.push(SubRole::MaidenNorthFreezer);
                } else {
                    subroles.push(SubRole::MaidenSouthFreezer);
                }
            }
        }

        let has_chinned = player_state.attacks().any(|(_, atk)| {
            atk.attack.is_chin() && atk.target.as_ref().is_some_and(|t| t.is_maiden_matomenos())
        });
        if has_chinned {
            subroles.push(SubRole::MaidenChinner);
        }

        subroles
    }

    fn determine_nylo_subroles(
        challenge: &Challenge,
        _nylo_data: &StageInfo,
        player_state: &PlayerStates,
        role: Role,
    ) -> Vec<SubRole> {
        use blert::challenge_data::stage_npc::Type;
        use blert::event::npc;
        use blert::PlayerAttack;

        if challenge.scale() != 5 {
            // Only 5s Nylo roles are currently supported.
            return Vec::new();
        }

        // All of the nylos that the player has prefired, arbitrarily defined as attacking it within
        // `PREFIRE_TICKS` of it spawning. This value is set relatively high to allow time for melee
        // nylos to walk down the lane.
        let mut nylos_counted = HashSet::new();

        let nylos_prefired = player_state
            .attacks()
            .filter_map(|(tick, atk)| {
                atk.target.as_ref().and_then(|target| match target.r#type {
                    Some(Type::Nylo(ref nylo)) => {
                        const PREFIRE_TICKS: u32 = 9;

                        if nylos_counted.contains(&target.room_id) {
                            return None;
                        }

                        if nylo.spawn_type() == npc::nylo::SpawnType::Split {
                            None
                        } else {
                            match tick.checked_sub(target.spawn_tick) {
                                Some(ticks) if ticks <= PREFIRE_TICKS => {
                                    nylos_counted.insert(target.room_id);
                                    Some((atk.attack, nylo))
                                }
                                Some(_) | None => None,
                            }
                        }
                    }
                    _ => None,
                })
            })
            .collect::<Vec<_>>();

        let mut subroles = Vec::new();

        if role == Role::Mage {
            let mut west_prefires = 0;
            let mut east_prefires = 0;

            for (attack, nylo) in nylos_prefired {
                // The following important mage prefires are counted:
                //
                //   - Wave 11 east barrage.
                //   - Wave 21 west barrage.
                //   - Wave 26 and 27 west/east bigs.
                //
                let consider_nylo = ((nylo.wave == 11 || nylo.wave == 21) && attack.is_barrage())
                    || ((nylo.wave == 26 || nylo.wave == 27) && nylo.big);

                if consider_nylo {
                    match nylo.spawn_type() {
                        npc::nylo::SpawnType::West => west_prefires += 1,
                        npc::nylo::SpawnType::East => east_prefires += 1,
                        _ => (),
                    }
                }
            }

            match west_prefires.cmp(&east_prefires) {
                std::cmp::Ordering::Greater => subroles.push(SubRole::NyloWestMage),
                std::cmp::Ordering::Less => subroles.push(SubRole::NyloEastMage),
                std::cmp::Ordering::Equal => {}
            }
        } else if role == Role::Melee {
            let mut west_prefires = 0;
            let mut east_prefires = 0;

            for (attack, nylo) in nylos_prefired {
                // The following melee "prefires" are counted:
                //
                //   - Wave 12 west/east doubles.
                //
                let consider_nylo = nylo.wave == 12
                    && matches!(attack, PlayerAttack::Scythe | PlayerAttack::ScytheUncharged);

                if consider_nylo {
                    match nylo.spawn_type() {
                        npc::nylo::SpawnType::West => west_prefires += 1,
                        npc::nylo::SpawnType::East => east_prefires += 1,
                        _ => (),
                    }
                }
            }

            match west_prefires.cmp(&east_prefires) {
                std::cmp::Ordering::Greater => subroles.push(SubRole::NyloWestMelee),
                std::cmp::Ordering::Less => subroles.push(SubRole::NyloEastMelee),
                std::cmp::Ordering::Equal => {}
            }
        }

        subroles
    }

    /// Counts how many times a player barraged a north or south Maiden crab.
    fn count_north_and_south_freezes(player_state: &PlayerStates) -> (u32, u32) {
        use blert::challenge_data::stage_npc::Type;
        use blert::event::npc::maiden_crab;

        // On
        player_state.attacks().fold((0, 0), |acc, (tick, atk)| {
            match (atk.attack, atk.target.as_ref()) {
                // Only count freezes occurring within 17 ticks of the crab spawning, as that is
                // how long a scuffed 4 crab takes to walk into Maiden. Any freezes beyond that
                // are considered DPS on the clump.
                (attack, Some(target)) if attack.is_barrage() && tick - target.spawn_tick <= 17 => {
                    if let Some(Type::MaidenCrab(crab)) = &target.r#type {
                        match crab.position() {
                            maiden_crab::Position::S1
                            | maiden_crab::Position::S2
                            | maiden_crab::Position::S3
                            | maiden_crab::Position::S4Inner
                            | maiden_crab::Position::S4Outer => (acc.0, acc.1 + 1),
                            maiden_crab::Position::N1
                            | maiden_crab::Position::N2
                            | maiden_crab::Position::N3
                            | maiden_crab::Position::N4Inner
                            | maiden_crab::Position::N4Outer => (acc.0 + 1, acc.1),
                        }
                    } else {
                        acc
                    }
                }
                _ => acc,
            }
        })
    }
}

impl Analyzer for TobRoleAnalyzer {
    type Output = HashMap<String, PlayerRoles>;

    fn name(&self) -> &str {
        "TobRoleAnalyzer"
    }

    fn analyze(&self, context: &crate::analysis::Context) -> Result<Self::Output> {
        let challenge = context.challenge();
        let blert::Challenge::Tob = challenge.r#type() else {
            return Err(Error::FailedPrecondition(
                "TobRoleAnalyzer requires a TOB challenge".into(),
            ));
        };

        let gear = context
            .get_dependency_output::<GearAnalyzer>()
            .ok_or(Error::Dependency("GearAnalyzer".into()))?;

        if challenge.scale() == 1 {
            let mut roles = HashMap::new();
            roles.insert(
                challenge.party()[0].clone(),
                PlayerRoles(Role::Solo, Vec::new()),
            );
            return Ok(roles);
        }

        Self::determine_roles(challenge, &gear)
    }
}
