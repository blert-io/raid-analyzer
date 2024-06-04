pub struct AnalysisContext {}

pub trait Analyzer {
    type Output;

    fn analyze(&self, context: &AnalysisContext);
}
