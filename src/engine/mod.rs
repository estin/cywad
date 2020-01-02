pub mod traits;

#[cfg(feature = "webkit")]
pub mod webkit;

#[cfg(feature = "devtools")]
pub mod devtools;

#[derive(Clone)]
#[cfg(feature = "webkit")]
pub struct EngineOptions {
    pub keep_open: bool,
}

#[derive(Clone)]
#[cfg(feature = "devtools")]
pub struct EngineOptions {
    pub command: Option<String>,
    pub endpoint: String,
    pub http_timeout: u64,
    pub max_frame_size: usize,
}

pub fn new() -> impl traits::EngineTrait {
    #[cfg(feature = "webkit")]
    let engine = webkit::Webkit {};

    #[cfg(feature = "devtools")]
    let engine = devtools::Devtools {};

    engine
}

impl Default for EngineOptions {
    #[cfg(feature = "webkit")]
    fn default() -> Self {
        EngineOptions { keep_open: false }
    }

    #[cfg(feature = "devtools")]
    fn default() -> Self {
        EngineOptions {
            command: None,
            endpoint: "http://127.0.0.1:9222/json".into(),
            http_timeout: 5,
            max_frame_size: 3 * 1024 * 1024,
        }
    }
}
