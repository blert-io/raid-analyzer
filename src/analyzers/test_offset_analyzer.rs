use crate::analysis::{Analyzer, Context};
use crate::error::Result;

use super::test_analyzer::TestAnalyzer;

/// Offsets the result of a previously run `TestAnalyzer` by a configured amount,
/// or simply returns the configured amount if no `TestAnalyzer` has been run.
#[derive(Debug)]
pub struct TestOffsetAnalyzer {
    offset: u32,
}

#[derive(Debug, serde::Deserialize)]
pub struct Config {
    offset: u32,
}

impl TestOffsetAnalyzer {
    pub fn new(config: &Config) -> Self {
        Self {
            offset: config.offset,
        }
    }
}

impl Analyzer for TestOffsetAnalyzer {
    type Output = u32;

    fn name(&self) -> &str {
        "TestOffsetAnalyzer"
    }

    fn analyze(&self, context: &Context) -> Result<Self::Output> {
        let value = context
            .get_dependency_output::<TestAnalyzer>()
            .map_or_else(|| self.offset, |v| *v + self.offset);
        log::debug!("TestOffsetAnalyzer output {value}");
        Ok(value)
    }
}
