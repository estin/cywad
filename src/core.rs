use chrono::prelude::{DateTime, Local};
use std::fmt;
use std::fs::File;
use std::io::Read;
use std::str::FromStr;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, RwLock};

#[cfg(feature = "server")]
use cron::Schedule;

use failure::Error;
use slug::slugify;

pub const APP_NAME: Option<&'static str> = option_env!("CARGO_PKG_NAME");
pub const APP_DESCRIPTION: Option<&'static str> = option_env!("CARGO_PKG_DESCRIPTION");
pub const APP_VERSION: Option<&'static str> = option_env!("CARGO_PKG_VERSION");
pub const SCHEDULER_SLEEP: u64 = 60; // each minute

use toml;

#[derive(Debug, Serialize, Deserialize)]
pub struct AppInfo {
    pub name: String,
    pub version: String,
    pub description: String,
    pub server_datetime: DateTime<Local>,
}

impl AppInfo {
    pub fn default() -> Self {
        AppInfo {
            name: APP_NAME.unwrap_or("n/a").to_string(),
            description: APP_DESCRIPTION.unwrap_or("n/a").to_string(),
            version: APP_VERSION.unwrap_or("n/a").to_string(),
            server_datetime: Local::now(),
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub url: String,
    pub name: String,
    pub cron: Option<String>,
    pub retry: Option<Vec<i64>>,
    pub window_width: u32,
    pub window_height: u32,
    pub step_timeout: i64,
    pub step_interval: u32,
    pub steps: Vec<StepConfig>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct StepConfig {
    pub kind: StepKind,
    pub key: Option<String>,
    pub exec: Option<String>,
    pub levels: Option<Vec<LevelConfig>>,
}

#[derive(Debug, Deserialize, PartialEq, Clone)]
#[serde(rename_all = "lowercase")]
pub enum StepKind {
    Wait,
    Exec,
    Value,
    Screenshot,
}

#[derive(Debug, PartialEq, Clone)]
pub enum StepResult {
    Idle,
    InWork,
    Repeat,
    Next,
    Error,
}

#[derive(Debug, Clone)]
pub struct ExecutionContext {
    pub ts_start: i64,
    pub step_result: StepResult,
    pub step_index: usize,
    pub step_max_index: usize,
    pub config: Config,
    pub config_index: usize,
    pub state: SharedState,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LevelConfig {
    pub name: String,
    pub more: Option<f64>,
    pub less: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub enum ResultItemState {
    Idle,
    InQueue,
    InWork,
    Done,
    Err,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ValueItem {
    pub key: String,
    pub value: f64,
    pub level: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ScreenshotItem {
    pub name: String,
    pub uri: String,

    #[serde(skip_serializing)]
    #[serde(skip_deserializing)]
    pub data: Vec<u8>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PreviouResultItemState {
    pub datetime: DateTime<Local>,
    pub values: Vec<ValueItem>,
    pub screenshots: Vec<ScreenshotItem>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ResultItem {
    pub name: String,
    pub slug: String,
    pub datetime: DateTime<Local>,
    pub scheduled: Option<DateTime<Local>>,
    pub values: Vec<ValueItem>,
    pub error: Option<String>,
    pub screenshots: Vec<ScreenshotItem>,
    pub state: ResultItemState,
    pub previous: Option<PreviouResultItemState>,
    pub steps_done: Option<usize>,
    pub steps_total: Option<usize>,
    pub attempt_count: Option<usize>,
}

impl ResultItem {
    pub fn new(name: &str) -> Self {
        ResultItem {
            name: name.to_string(),
            slug: slugify(name.to_string()),
            datetime: Local::now(),
            scheduled: None,
            values: Vec::new(),
            error: None,
            screenshots: Vec::new(),
            state: ResultItemState::Idle,
            previous: None,
            steps_done: None,
            steps_total: None,
            attempt_count: None,
        }
    }

    pub fn is_ok(&self) -> bool {
        self.state == ResultItemState::Done
    }
    pub fn is_err(&self) -> bool {
        self.state == ResultItemState::Err
    }
    pub fn is_in_work(&self) -> bool {
        self.state == ResultItemState::InWork
    }
    pub fn is_idle(&self) -> bool {
        self.state == ResultItemState::Idle
    }
    pub fn is_in_queue(&self) -> bool {
        self.state == ResultItemState::InQueue
    }
    pub fn get_error(&self) -> &str {
        self.error.as_ref().map_or("None", |error| error)
    }
    pub fn get_scheduled(&self) -> String {
        self.scheduled
            .as_ref()
            .map_or("-".to_string(), |scheduled| {
                scheduled.format("%Y-%m-%d %H:%M:%S").to_string()
            })
    }
}

pub fn get_level(value: f64, levels: &[LevelConfig]) -> Option<String> {
    for level in levels {
        if let Some(more_value) = level.more {
            if value >= more_value {
                return Some(level.name.to_owned());
            }
        }
        if let Some(less_value) = level.less {
            if value < less_value {
                return Some(level.name.to_owned());
            }
        }
    }
    None
}

pub fn validate_config(config: &Config) -> Result<(), Error> {
    // validate schedule
    #[cfg(feature = "server")]
    {
        if let Some(ref schedule) = config.cron {
            if let Err(e) = Schedule::from_str(schedule) {
                return Err(format_err!("'cron' field - {}", e));
            }
        }
    }

    // validate each step
    for (i, step) in config.steps.iter().enumerate() {
        match step.kind {
            StepKind::Wait | StepKind::Value | StepKind::Exec => {
                // `exec` field required for wait/value/exec
                if step.exec.is_none() {
                    return Err(format_err!(
                        "'wait/value/exec' step #{} without 'exec' field",
                        i + 1
                    ));
                }
                // `value` step must be with `key` field
                if step.kind == StepKind::Value && step.key.is_none() {
                    return Err(format_err!("'value' step #{} without 'key' field", i + 1));
                }
            }
            _ => {}
        }
    }
    Ok(())
}

pub fn load_config(path: &str) -> Result<Config, Error> {
    let mut file = File::open(path)?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)?;
    let config: Config =
        toml::from_str(&contents).map_err(|e| format_err!("Toml file parsing error: {}", e))?;
    validate_config(&config)?;
    Ok(config)
}

#[derive(Debug)]
pub struct State {
    pub configs: Vec<Config>,
    pub results: Vec<ResultItem>,
    pub tx: Option<Mutex<Sender<usize>>>,
    pub tx_vec: Option<Vec<Mutex<Sender<ResultItem>>>>,
}

impl State {
    pub fn broadcast(&mut self, index: usize) {
        self.tx_vec = {
            let tx_vec = match self.tx_vec {
                Some(ref mut vec) => vec,
                None => return,
            };
            let mut new_tx_vec: Vec<Mutex<Sender<ResultItem>>> = Vec::new();
            if let Some(ref mut item) = self.results.get(index) {
                // TODO change to drain filter
                for tx in tx_vec.drain(..) {
                    let success: bool = {
                        if let Ok(sender) = tx.lock() {
                            sender.send(item.clone()).is_ok()
                        } else {
                            error!("Sender lock error");
                            false
                        }
                    };
                    if success {
                        new_tx_vec.push(tx);
                    }
                }
            }
            Some(new_tx_vec)
        }
    }
}

pub type SharedState = Arc<RwLock<State>>;

impl fmt::Display for ExecutionContext {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{} {}/{}",
            self.config.name, self.step_index, self.step_max_index,
        )
    }
}

impl ExecutionContext {
    pub fn new(config: Config, config_index: usize, state: SharedState) -> Self {
        ExecutionContext {
            ts_start: Local::now().timestamp(),
            step_index: 0,
            step_result: StepResult::Idle,
            step_max_index: config.steps.len() - 1,
            config,
            config_index,
            state,
        }
    }

    pub fn is_done(&self) -> bool {
        self.step_index > self.step_max_index
    }

    pub fn get_step(&self) -> Result<StepConfig, Error> {
        if self.is_done() {
            bail!("[{}] Step #{} out of bounds", self, self.step_index);
        }
        Ok(self.config.steps[self.step_index].clone())
    }

    pub fn start_new_step(&mut self) -> Result<(), Error> {
        // increment step indexes
        self.step_index += 1;
        // start new timer
        self.ts_start = Local::now().timestamp();
        match self.state.write() {
            Ok(mut state) => {
                if let Some(item) = state.results.get_mut(self.config_index) {
                    item.datetime = Local::now();
                    item.steps_done = Some(self.step_index);
                }
                state.broadcast(self.config_index);
            }
            Err(e) => return Err(format_err!("RWLock error: {}", e)),
        };
        debug!("[{}] Step index increased #{}", self, self.step_index);
        Ok(())
    }

    pub fn timeout(&mut self) -> Result<(), Error> {
        match self.state.write() {
            Ok(mut state) => {
                if let Some(item) = state.results.get_mut(self.config_index) {
                    item.datetime = Local::now();
                    item.state = ResultItemState::Err;
                    item.error = Some(format!("Timeout: {}", self.config.step_timeout));
                }
                state.broadcast(self.config_index);
            }
            Err(e) => return Err(format_err!("RWLock error: {}", e)),
        };
        Ok(())
    }

    pub fn done(&mut self) -> Result<(), Error> {
        match self.state.write() {
            Ok(mut state) => {
                if let Some(item) = state.results.get_mut(self.config_index) {
                    item.datetime = Local::now();
                    item.state = ResultItemState::Done;
                    item.steps_done = item.steps_total;
                }
                state.broadcast(self.config_index);
            }
            Err(e) => return Err(format_err!("RWLock error: {}", e)),
        };
        Ok(())
    }

    pub fn error(&mut self, error_message: String) -> Result<(), Error> {
        match self.state.write() {
            Ok(mut state) => {
                if let Some(item) = state.results.get_mut(self.config_index) {
                    item.datetime = Local::now();
                    item.state = ResultItemState::Err;
                    item.error = Some(error_message);
                }
                state.broadcast(self.config_index);
            }
            Err(e) => return Err(format_err!("RWLock error: {}", e)),
        };
        Ok(())
    }

    pub fn add_value_and_start_new_step(&mut self, value: f64) -> Result<(), Error> {
        match self.state.write() {
            Ok(mut state) => {
                if let Some(item) = state.results.get_mut(self.config_index) {
                    let step = &self.get_step()?;

                    let key = if let Some(ref key) = step.key {
                        key.to_owned()
                    } else {
                        if step.kind == StepKind::Value {
                            unreachable!("'key' not defined for step value");
                        }
                        "".to_owned()
                    };

                    item.values.push(ValueItem {
                        value,
                        key: key.to_owned(),
                        level: if let Some(ref levels) = step.levels {
                            get_level(value, levels)
                        } else {
                            None
                        },
                    });

                    item.datetime = Local::now();
                    self.step_index += 1;
                    self.ts_start = Local::now().timestamp();
                    item.steps_done = Some(self.step_index);
                }
                state.broadcast(self.config_index);
            }
            Err(e) => return Err(format_err!("RWLock error: {}", e)),
        }

        Ok(())
    }

    pub fn add_screenshot_and_start_new_step(&mut self, value: Vec<u8>) -> Result<(), Error> {
        match self.state.write() {
            Ok(mut state) => {
                if let Some(item) = state.results.get_mut(self.config_index) {
                    let step = &self.get_step()?;
                    let (name, for_uri) = if let Some(ref key) = step.key {
                        (key.to_owned(), key.to_owned())
                    } else {
                        ("".to_string(), item.screenshots.len().to_string())
                    };

                    item.screenshots.push(ScreenshotItem {
                        uri: format!("{}/{}.png", item.slug, slugify(&for_uri)),
                        name,
                        data: value,
                    });

                    item.datetime = Local::now();
                    self.step_index += 1;
                    self.ts_start = Local::now().timestamp();
                    item.steps_done = Some(self.step_index);
                }
                state.broadcast(self.config_index);
            }
            Err(e) => return Err(format_err!("RWLock error: {}", e)),
        }

        Ok(())
    }
}
