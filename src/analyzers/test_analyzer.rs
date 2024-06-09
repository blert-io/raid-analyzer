use serde::{Deserialize, Serialize};

use crate::analysis::{Analyzer, Context};
use crate::error::Result;

/// An analyzer that simply returns a configured value.
#[derive(Debug)]
pub struct TestAnalyzer {
    value: u32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    value: u32,
}

impl TestAnalyzer {
    pub fn new(config: &Config) -> Self {
        Self {
            value: config.value,
        }
    }
}

impl Analyzer for TestAnalyzer {
    type Output = u32;

    fn name(&self) -> &str {
        "TestAnalyzer"
    }

    fn analyze(&self, _: &Context) -> Result<Self::Output> {
        Ok(self.value)
    }
}
