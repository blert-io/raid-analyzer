use crate::blert;

pub struct Id {}

impl Id {
    pub const MAIDEN_MATOMENOS_ENTRY: u32 = 10820;
    pub const MAIDEN_MATOMENOS_REGULAR: u32 = 8366;
    pub const MAIDEN_MATOMENOS_HARD: u32 = 10828;
}

#[allow(clippy::module_name_repetitions)]
pub trait NpcExt {
    /// Returns whether the NPC is a red crab at Maiden.
    fn is_maiden_matomenos(&self) -> bool;
}

impl NpcExt for blert::event::Npc {
    fn is_maiden_matomenos(&self) -> bool {
        self.id == Id::MAIDEN_MATOMENOS_ENTRY
            || self.id == Id::MAIDEN_MATOMENOS_REGULAR
            || self.id == Id::MAIDEN_MATOMENOS_HARD
    }
}

impl NpcExt for blert::challenge_data::StageNpc {
    fn is_maiden_matomenos(&self) -> bool {
        self.spawn_npc_id == Id::MAIDEN_MATOMENOS_ENTRY
            || self.spawn_npc_id == Id::MAIDEN_MATOMENOS_REGULAR
            || self.spawn_npc_id == Id::MAIDEN_MATOMENOS_HARD
    }
}
