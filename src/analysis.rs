use std::any::Any;

use crate::challenge::Challenge;
use crate::error::Result;

#[derive(Debug, Clone, Copy)]
pub enum Level {
    /// Base level of analysis run on every recorded challenge. Prioritizes
    /// speed and simplicity.
    Basic,

    /// Analysis targeting players who are new to the content and focused on
    /// learning basic skills and mechanics.
    Learner,

    /// Analysis targeting players who run the content regularly, but do not
    /// use advanced strategies or techniques.
    Casual,

    /// Advanced analysis based on the optimal strategies and techniques for
    /// the content.
    MaxEff,
}

#[derive(Debug)]
pub struct Context {
    level: Level,

    challenge: Challenge,
}

impl Context {
    pub fn new(level: Level, challenge: Challenge) -> Self {
        Self { level, challenge }
    }

    pub fn level(&self) -> Level {
        self.level
    }

    pub fn challenge(&self) -> &Challenge {
        &self.challenge
    }
}

pub trait Analyzer {
    type Output;

    /// Returns a globally unique name for the analyzer.
    fn name(&self) -> &str;

    fn analyze(&self, context: &Context) -> Result<Self::Output>;
}

struct ResolvedAnalyzer<A: Analyzer> {
    analyzer: A,
    output: Option<A::Output>,
    context: Context,
    dependencies: Vec<Box<dyn Any>>,
}
