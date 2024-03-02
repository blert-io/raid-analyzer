use crate::raid::Raid;

pub struct AnalysisContext {
    raid: Raid,
}

pub trait Analyzer {
    type Output;

    fn analyze(&self, context: &AnalysisContext);
}
