use crate::analysis::Analyzer;
use crate::error::{Error, Result};

struct GearAnalyzer {}

impl Analyzer for GearAnalyzer {
    type Output = ();

    fn name(&self) -> &str {
        "GearAnalyzer"
    }

    fn analyze(&self, context: &crate::analysis::Context) -> Result<Self::Output> {
        let challenge = context.challenge();

        for stage in challenge.stage_infos() {
            for player in challenge.party() {
                let state = stage.player_state(player).ok_or(Error::IncompleteData)?;
            }
        }

        Ok(())
    }
}
