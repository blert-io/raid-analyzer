use std::any::Any;
use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::sync::{Arc, RwLock};
use std::time::Instant;

use futures::future::{self, TryFutureExt};
use serde::{Deserialize, Serialize};
use tokio::fs;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::analyzers::init_analyzer;
use crate::challenge::Challenge;
use crate::error::{Error, Result};
use crate::item;

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

/// An analysis `Context` provides information about the active analysis program run.
pub struct Context {
    challenge: Arc<Challenge>,
    item_registry: Arc<item::Registry>,
    level: Level,
    completed_analyzers: Arc<RwLock<HashMap<String, Box<dyn RunnableAnalyzer>>>>,
}

impl Context {
    fn new(
        challenge: Arc<Challenge>,
        item_registry: Arc<item::Registry>,
        level: Level,
        completed_analyzers: Arc<RwLock<HashMap<String, Box<dyn RunnableAnalyzer>>>>,
    ) -> Self {
        Self {
            challenge,
            item_registry,
            level,
            completed_analyzers,
        }
    }

    /// Returns the configured analysis level.
    pub fn level(&self) -> Level {
        self.level
    }

    /// Returns the challenge being analyzed.
    pub fn challenge(&self) -> &Challenge {
        &self.challenge
    }

    /// Returns a registry of all known game items.
    pub fn item_registry(&self) -> &item::Registry {
        &self.item_registry
    }

    /// Returns the output of a dependency of the current analyzer.
    /// If the dependency is optional, may return `None`.
    pub fn get_dependency_output<A: Analyzer + 'static>(&self) -> Option<Arc<A::Output>> {
        self.completed_analyzers
            .read()
            .unwrap()
            .values()
            .find_map(|a| {
                a.as_any()
                    .downcast_ref::<AnalyzerRun<A>>()
                    .and_then(|a| a.output.clone())
            })
    }
}

pub trait Analyzer {
    /// Output produced by the analyzer to be consumed by other analyzers.
    type Output;

    /// Returns a globally unique name for the analyzer implementation.
    fn name(&self) -> &str;

    fn analyze(&self, context: &Context) -> Result<Self::Output>;
}

/// A specific instantiation of an `Analyzer` run within an analysis program.
pub trait RunnableAnalyzer: Send + Sync {
    fn name(&self) -> &str;
    fn run(&mut self, context: &Context) -> Result<()>;
    fn as_any(&self) -> &dyn Any;
}

#[derive(Debug)]
struct AnalyzerRun<A: Analyzer> {
    analyzer_name: String,
    analyzer: A,
    output: Option<Arc<A::Output>>,
}

impl<A> RunnableAnalyzer for AnalyzerRun<A>
where
    A: Analyzer + Send + Sync + 'static,
    <A as Analyzer>::Output: Send + Sync,
{
    fn name(&self) -> &str {
        self.analyzer_name.as_str()
    }

    fn run(&mut self, context: &Context) -> Result<()> {
        let output = self.analyzer.analyze(context)?;
        self.output = Some(Arc::new(output));
        Ok(())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Wraps an instance of an `Analyzer` in a form runnable by the engine.
pub fn wrap_analyzer<A>(name: String, analyzer: A) -> Box<dyn RunnableAnalyzer>
where
    A: Analyzer + Send + Sync + 'static,
    <A as Analyzer>::Output: Send + Sync,
{
    Box::new(AnalyzerRun {
        analyzer_name: name,
        analyzer,
        output: None,
    })
}

struct WorkerRunRequest {
    analyzer: Box<dyn RunnableAnalyzer>,
    context: Context,
    notify_tx: mpsc::Sender<WorkerRunResponse>,
}

struct WorkerRunResponse {
    analyzer: Box<dyn RunnableAnalyzer>,
    result: Result<()>,
}

struct ProgramRun {
    program: Arc<ProgramConfig>,
    run_number: u32,
    level: Level,
    analyzers_to_run: u32,
    dispatch_tx: async_channel::Sender<WorkerRunRequest>,
    notify_tx: mpsc::Sender<WorkerRunResponse>,
    notify_rx: mpsc::Receiver<WorkerRunResponse>,
    blocked: BTreeMap<String, Box<dyn RunnableAnalyzer>>,
    pending: BTreeMap<String, Box<dyn RunnableAnalyzer>>,
    completed: Arc<RwLock<HashMap<String, Box<dyn RunnableAnalyzer>>>>,
    challenge: Arc<Challenge>,
    item_registry: Arc<item::Registry>,
}

impl ProgramRun {
    fn new(
        program: Arc<ProgramConfig>,
        run_number: u32,
        level: Level,
        dispatch_tx: async_channel::Sender<WorkerRunRequest>,
        challenge: Challenge,
        item_registry: Arc<item::Registry>,
    ) -> Self {
        let (notify_tx, notify_rx) = mpsc::channel(8);
        let analyzers_to_run = program.analyzers.len() as u32;

        Self {
            program,
            run_number,
            level,
            analyzers_to_run,
            dispatch_tx,
            notify_tx,
            notify_rx,
            blocked: BTreeMap::new(),
            pending: BTreeMap::new(),
            completed: Arc::new(RwLock::new(HashMap::new())),
            challenge: Arc::new(challenge),
            item_registry,
        }
    }

    fn program_name(&self) -> &str {
        &self.program.program.name
    }

    async fn run(&mut self) -> Result<()> {
        self.initialize_analyzers()?;
        self.schedule_all_pending().await?;

        while self.analyzers_to_run > 0 {
            let response = self.notify_rx.recv().await.ok_or(Error::IncompleteData)?;
            if let Err(e) = response.result {
                log::error!(r#"Analyzer "{}" failed: {e:?}"#, response.analyzer.name());
                return Err(e);
            }

            self.handle_completed(response.analyzer);
            self.schedule_all_pending().await?;
            self.analyzers_to_run -= 1;
        }

        Ok(())
    }

    fn initialize_analyzers(&mut self) -> Result<()> {
        self.program
            .analyzers
            .iter()
            .try_for_each(|(name, definition)| {
                let analyzer =
                    init_analyzer(name, &definition.implementation, definition.config.clone())?;
                self.blocked.insert(name.clone(), analyzer);
                Ok::<(), Error>(())
            })?;
        self.unblock_analyzers();
        Ok(())
    }

    fn unblock_analyzers(&mut self) {
        let completed = self.completed.read().unwrap();

        let blocked = std::mem::take(&mut self.blocked);
        self.blocked = blocked
            .into_iter()
            .filter_map(|(name, analyzer)| {
                let runnable = match self.program.analyzers[&name].dependencies.as_ref() {
                    Some(dependencies) => dependencies.iter().all(|d| completed.contains_key(d)),
                    None => true,
                };

                if runnable {
                    log::debug!(r#"Unblocked analyzer "{name}""#);
                    self.pending.insert(name, analyzer);
                    None
                } else {
                    Some((name, analyzer))
                }
            })
            .collect();
    }

    async fn schedule_all_pending(&mut self) -> Result<()> {
        let pending = std::mem::take(&mut self.pending);

        future::try_join_all(pending.into_values().map(|analyzer| {
            let request = WorkerRunRequest {
                analyzer,
                context: Context::new(
                    self.challenge.clone(),
                    self.item_registry.clone(),
                    self.level,
                    self.completed.clone(),
                ),
                notify_tx: self.notify_tx.clone(),
            };

            log::debug!(r#"Scheduled analyzer "{}" to run"#, request.analyzer.name());
            self.dispatch_tx
                .send(request)
                .map_err(|_| Error::FailedPrecondition("Worker channel closed".into()))
        }))
        .await?;

        Ok(())
    }

    fn handle_completed(&mut self, analyzer: Box<dyn RunnableAnalyzer>) {
        self.completed
            .write()
            .unwrap()
            .insert(analyzer.name().to_string(), analyzer);
        self.unblock_analyzers();
    }
}

impl std::fmt::Debug for ProgramRun {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.debug_struct("ProgramRun")
            .field("program", &self.program)
            .field("run_number", &self.run_number)
            .field("level", &self.level)
            .field("analyzers_to_run", &self.analyzers_to_run)
            .field("notify_tx", &self.notify_tx)
            .field("notify_rx", &self.notify_rx)
            .field("dispatch_tx", &self.dispatch_tx)
            .field("blocked", &self.blocked.len())
            .field("pending", &self.pending.len())
            .field(
                "completed",
                &self.completed.try_read().map(|r| r.len()).unwrap_or(0),
            )
            .field("challenge", &self.challenge)
            .field("item_registry", &self.item_registry)
            .finish()
    }
}

pub struct Engine {
    programs: HashMap<String, Arc<ProgramConfig>>,
    workers: Vec<JoinHandle<()>>,
    dispatch_tx: Option<async_channel::Sender<WorkerRunRequest>>,
    num_programs_run: u32,
    item_registry: Arc<item::Registry>,
}

impl Engine {
    /// Loads analysis programs defined in TOML files from the directory at `path`.
    pub async fn load_from_directory(
        path: impl AsRef<Path>,
        item_registry: item::Registry,
    ) -> Result<Self> {
        let mut programs = HashMap::new();
        let mut dir = fs::read_dir(path).await?;

        while let Some(entry) = dir.next_entry().await? {
            let path = entry.path();
            if !path.is_file() || path.extension().map_or(true, |ext| ext != "toml") {
                continue;
            }

            let config = fs::read(path).await?;
            let config = String::from_utf8(config).map_err(|_| Error::IncompleteData)?;
            let program: ProgramConfig =
                toml::from_str(&config).map_err(|_| Error::IncompleteData)?;

            programs.insert(program.program.name.clone(), Arc::new(program));
        }

        Ok(Self {
            programs,
            workers: Vec::new(),
            dispatch_tx: None,
            num_programs_run: 0,
            item_registry: Arc::new(item_registry),
        })
    }

    /// Begins running the analysis engine with the specified number of workers.
    pub fn start(&mut self, worker_count: u32) {
        let (dispatch_tx, dispatch_rx) = async_channel::unbounded();

        self.dispatch_tx = Some(dispatch_tx);
        for id in 0..worker_count {
            self.workers.push(Worker::spawn(id, dispatch_rx.clone()));
        }
    }

    /// Runs an analysis program on a challenge, at the specified level.
    ///
    /// [`start`](#method.start) must have been called before this method, or it will fail.
    pub fn run_program(&mut self, program: &str, level: Level, challenge: Challenge) -> Result<()> {
        let Some(program) = self.programs.get(program) else {
            return Err(Error::InvalidArgument);
        };

        let dispatch_tx = match &self.dispatch_tx {
            Some(tx) => tx.clone(),
            None => return Err(Error::FailedPrecondition("Engine not started".into())),
        };

        log::info!(
            "Running program {} on challenge {}",
            program.program.name,
            challenge.uuid(),
        );

        self.num_programs_run += 1;
        let run_number = self.num_programs_run;

        let mut program_run = ProgramRun::new(
            program.clone(),
            run_number,
            level,
            dispatch_tx,
            challenge,
            self.item_registry.clone(),
        );

        tokio::spawn(async move {
            let run_start = Instant::now();

            match program_run.run().await {
                Ok(()) => {
                    log::debug!(
                        r#"Program "{}" completed in {:?}"#,
                        program_run.program_name(),
                        run_start.elapsed(),
                    );
                }
                Err(e) => {
                    log::error!(
                        r#"Program "{}" failed in {:?}: {e:?}"#,
                        program_run.program_name(),
                        run_start.elapsed()
                    );
                }
            }
        });

        Ok(())
    }
}

struct Worker {
    id: u32,
    dispatch_rx: async_channel::Receiver<WorkerRunRequest>,
}

impl Worker {
    fn spawn(id: u32, dispatch_rx: async_channel::Receiver<WorkerRunRequest>) -> JoinHandle<()> {
        let worker = Self { id, dispatch_rx };
        tokio::spawn(worker.run())
    }

    async fn run(self) {
        loop {
            let Ok(mut request) = self.dispatch_rx.recv().await else {
                break;
            };

            log::debug!(
                r#"Worker {} running analyzer "{}""#,
                self.id,
                request.analyzer.name(),
            );
            let start = Instant::now();

            let result = request.analyzer.run(&request.context);

            log::debug!(
                r#"Worker {} completed analyzer "{}" in {:?}"#,
                self.id,
                request.analyzer.name(),
                start.elapsed(),
            );

            request
                .notify_tx
                .send(WorkerRunResponse {
                    analyzer: request.analyzer,
                    result,
                })
                .await
                .unwrap();
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct ProgramConfig {
    program: ProgramDefinition,
    analyzers: HashMap<String, AnalyzerDefinition>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ProgramDefinition {
    name: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct AnalyzerDefinition {
    implementation: String,
    dependencies: Option<Vec<String>>,
    config: Option<toml::Value>,
}
