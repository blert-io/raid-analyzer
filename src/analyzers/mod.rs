use crate::analysis::{wrap_analyzer, RunnableAnalyzer};
use crate::error::{Error, Result};

pub mod gear_analyzer;
pub mod test_analyzer;
pub mod test_offset_analyzer;
pub mod tob_role_analyzer;

/// Initializes a new instance of the analyzer with the given implementation name based on
/// analyzer-specific configuration options.
pub fn init_analyzer(
    name: &str,
    implementation: &str,
    config: Option<toml::Value>,
) -> Result<Box<dyn RunnableAnalyzer>> {
    match implementation {
        "GearAnalyzer" => Ok(wrap_analyzer(
            name.into(),
            gear_analyzer::GearAnalyzer::new(),
        )),
        "TestAnalyzer" => {
            let config = config
                .ok_or(Error::Config("TestAnalyzer missing config options".into()))?
                .try_into()?;
            Ok(wrap_analyzer(
                name.into(),
                test_analyzer::TestAnalyzer::new(&config),
            ))
        }
        "TestOffsetAnalyzer" => {
            let config = config
                .ok_or(Error::Config(
                    "TestOffsetAnalyzer missing config options".into(),
                ))?
                .try_into()?;
            Ok(wrap_analyzer(
                name.into(),
                test_offset_analyzer::TestOffsetAnalyzer::new(&config),
            ))
        }
        "TobRoleAnalyzer" => Ok(wrap_analyzer(
            name.into(),
            tob_role_analyzer::TobRoleAnalyzer::new(),
        )),
        _ => Err(Error::Config(format!("Unknown analyzer: {name}"))),
    }
}
