// use failure::Error;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use anyhow::{anyhow, bail, Error};
// use failure::{bail, format_err};
use log::{debug, error, info};
use serde::{Deserialize, Serialize};

use std::collections::HashMap;
use std::process::Command;
use std::time::Duration;

use cywad_core::{
    EngineOptions, EngineTrait, ExecutionContext, PreviouResultItemState, ResultItemState,
    SharedState, StepKind, StepResult,
};

use std::net::TcpStream;

pub struct Devtools;

use actix::io::SinkWrite;
use actix::*;
use actix_codec::Framed;
use actix_web::client;
use actix_web::http::Uri;

use chrono::prelude::Local;
use futures::future::TryFutureExt;
use futures::stream::{SplitSink, StreamExt};

use futures::future::poll_fn;
use futures::task;
// use tokio::time;

use awc::{
    error::WsProtocolError,
    ws::{Codec, Frame, Message},
    BoxedSocket,
};

const CHECK_PORT_OPEN_TRIES: u64 = 5;

#[derive(Serialize)]
struct SecurityParams {
    ignore: bool,
}

#[derive(Serialize)]
struct ScreenOrientation {
    angle: u32,
    #[serde(rename = "type")]
    otype: String,
}

#[derive(Serialize)]
struct MetricsParams {
    width: u32,
    height: u32,
    mobile: bool,
    #[serde(rename = "deviceScaleFactor")]
    device_scale_factor: f32,
    #[serde(rename = "screenOrientation")]
    screen_orientation: ScreenOrientation,
}

#[derive(Serialize)]
struct NavigateParams {
    url: String,
}

#[derive(Serialize)]
struct EvaluateParams {
    expression: String,
}

#[derive(Serialize)]
struct CaptureParams {
    format: String,
}

#[derive(Serialize)]
#[serde(untagged)]
enum JsonRpcParams {
    Navigate(NavigateParams),
    Evaluate(EvaluateParams),
    Capture(CaptureParams),
    Security(SecurityParams),
    Metrics(MetricsParams),
    WithoutParams,
}

#[derive(Serialize)]
struct JsonRpcRequest {
    id: usize,
    method: String,
    params: JsonRpcParams,
}

#[derive(Message)]
#[rtype(result = "Result<serde_json::Value, Error>")]
struct ClientCmd {
    method: String,
    params: JsonRpcParams,
}

#[derive(Deserialize, Debug)]
struct DevToolsResponse {
    #[serde(alias = "webSocketDebuggerUrl")]
    web_socket_debugger_url: String,
}

struct WSClient {
    writer: SinkWrite<Message, SplitSink<Framed<BoxedSocket, Codec>, Message>>,
    request_id: usize,
    responses: HashMap<usize, serde_json::Value>,
    request_ready: HashMap<usize, Arc<AtomicBool>>,
}

impl Actor for WSClient {
    type Context = Context<Self>;
    fn started(&mut self, _ctx: &mut Self::Context) {
        info!("WSClient started");
    }

    fn stopped(&mut self, _: &mut Self::Context) {
        info!("WSClient stoped");
        System::current().stop();
    }
}

// Handle server websocket messages
impl StreamHandler<Result<Frame, WsProtocolError>> for WSClient {
    fn started(&mut self, _ctx: &mut Self::Context) {
        info!("WSClient Connected");
    }

    fn handle(&mut self, msg: Result<awc::ws::Frame, WsProtocolError>, _ctx: &mut Self::Context) {
        if let Ok(awc::ws::Frame::Text(txt)) = msg {
            info!("Server: {:?}", txt);
            match serde_json::from_slice::<serde_json::Value>(&txt) {
                Ok(v) => {
                    let mut response_is_valid = false;
                    if let Some(response_id) = v.get("id") {
                        if let Some(response_id) = response_id.as_u64() {
                            let response_id = response_id as usize;
                            if let Some(result) = v.get("result") {
                                if let Some(slot) = self.request_ready.get_mut(&response_id) {
                                    self.responses.insert(response_id, result.clone());
                                    slot.store(true, Ordering::Relaxed);
                                    response_is_valid = true;
                                }
                            }
                        }
                    }

                    if !response_is_valid {
                        error!("WS response have't id or result attribute");
                    }
                }
                Err(e) => {
                    error!("Some error on ws response parse - {}", e);
                }
            }
        }
    }

    fn finished(&mut self, ctx: &mut Self::Context) {
        info!("WSClient. Disconnected");
        ctx.stop();
    }
}

impl EngineTrait for Devtools {
    fn execute(
        &mut self,
        config_index: usize,
        state: SharedState,
        engine_options: EngineOptions,
    ) -> Result<(), Error> {
        let browser = match &engine_options.command {
            Some(command) => {
                let mut parts = command.split(' ');
                let executable = match parts.next() {
                    Some(value) => value,
                    None => bail!("Invalid engine command"),
                };
                info!("Try start browser: {}", command);
                let browser = Command::new(executable)
                    .args(parts.collect::<Vec<&str>>())
                    .spawn()
                    .map_err(|e| anyhow!("browser command failed to start {} - {}", command, e))?;
                info!("Browser started with id: {}", browser.id());
                Some(browser)
            }
            None => None,
        };

        // now wait to ensure browser is starts
        let uri = engine_options
            .endpoint
            .parse::<Uri>()
            .map_err(|e| anyhow!("On parse endopoint - {}", e))?;

        let host_port = format!(
            "{}:{}",
            uri.host().unwrap_or("127.0.0.1"),
            uri.port_u16().unwrap_or(9222),
        );
        let sleep_duration =
            Duration::from_millis(engine_options.http_timeout * 1000 / CHECK_PORT_OPEN_TRIES);
        for i in 0..CHECK_PORT_OPEN_TRIES {
            debug!("Check #{} {} is open", i, host_port);
            if TcpStream::connect(&host_port).is_ok() {
                break;
            }
            debug!("Wait {:?}", sleep_duration);
            std::thread::sleep(sleep_duration);
        }

        let result = self.run(config_index, state, engine_options);

        if let Some(mut process) = browser {
            info!("Try kill browser {}", process.id());
            process
                .kill()
                .map_err(|e| anyhow!("browser kill error: {}", e))?;

            // now wait to ensure browser is killed
            for i in 0..CHECK_PORT_OPEN_TRIES {
                debug!("Check #{} {} is closed", i, host_port);
                if TcpStream::connect(&host_port).is_err() {
                    break;
                }
                debug!("Wait {:?}", sleep_duration);
                std::thread::sleep(sleep_duration);
            }
        }

        result
    }
}

impl Devtools {
    fn run(
        &mut self,
        config_index: usize,
        state: SharedState,
        engine_options: EngineOptions,
    ) -> Result<(), Error> {
        // create excution context
        let config = {
            let s = state.read().map_err(|e| anyhow!("RWLock error: {}", e))?;
            s.configs
                .get(config_index)
                .ok_or_else(|| anyhow!("Config not found by index {}", config_index))?
                .clone()
        };

        // initialize result
        {
            let mut state = state.write().map_err(|e| anyhow!("RWLock error: {}", e))?;
            if let Some(item) = state.results.get_mut(config_index) {
                // save previous success state
                item.previous = Some(PreviouResultItemState {
                    datetime: item.datetime,
                    values: (&item.values).to_vec(),
                    screenshots: (&item.screenshots).to_vec(),
                });

                // reset state
                item.state = ResultItemState::InWork;
                item.datetime = Local::now();
                item.values.clear();
                item.screenshots.clear();
                item.steps_done = Some(0);
                item.steps_total = Some(config.steps.len());
                item.attempt_count = Some(item.attempt_count.unwrap_or(0) + 1);
            }

            state.broadcast(config_index);
        }

        // let sys = System::new("cywad");
        let mut sys = System::new("cywad");

        info!(
            "Try connect to {} with timeout {}s",
            engine_options.endpoint, engine_options.http_timeout,
        );

        let client = client::Client::default();

        let fut = async move {
            let request = client
                .get(&engine_options.endpoint)
                .timeout(Duration::new(engine_options.http_timeout, 0));

            let mut response = request
                .send()
                .map_err(|e| {
                    System::current().stop();
                    anyhow!("Failed to connect to debug port - {}", e)
                })
                .await?;
            let data = response
                .json::<Vec<DevToolsResponse>>()
                .map_err(|e| {
                    System::current().stop();
                    anyhow!("Failed to parse response - {}", e)
                })
                .await?;

            if data.is_empty() {
                System::current().stop();
                bail!("Devtools empty response");
            }

            let item = &data[0];

            let client = awc::Client::new();

            let (_response, framed) = client
                .ws((&item.web_socket_debugger_url).replace("ws", "http"))
                .max_frame_size(engine_options.max_frame_size)
                .connect()
                .map_err(|e| {
                    System::current().stop();
                    anyhow!("Failed to connect to websocket - {}", e)
                })
                .await?;

            let (sink, stream) = framed.split();

            let ws_client = WSClient::create(move |ctx| {
                WSClient::add_stream(stream, ctx);
                WSClient {
                    writer: SinkWrite::new(sink, ctx),
                    request_id: Local::now().timestamp() as usize,
                    // execution_context: ExecutionContext::new(config, config_index, state),
                    responses: HashMap::new(),
                    request_ready: HashMap::new(),
                }
            });

            let mut execution_context = ExecutionContext::new(config, config_index, state);
            let _ = ws_client
                .send(ClientCmd {
                    method: "Page.navigate".to_owned(),
                    params: JsonRpcParams::Navigate(NavigateParams {
                        url: "chrome://system".to_owned(),
                    }),
                })
                .timeout(Duration::from_millis(
                    execution_context.config.step_timeout as u64,
                ))
                .await?;

            // not supported?
            // let _ = ws_client
            //     .send(ClientCmd {
            //         method: "Network.clearBrowserCache".to_owned(),
            //         params: JsonRpcParams::WithoutParams,
            //         timeout: execution_context.config.step_timeout,
            //     })
            //     .await?;

            let _ = ws_client
                .send(ClientCmd {
                    method: "Network.clearBrowserCookies".to_owned(),
                    params: JsonRpcParams::WithoutParams,
                })
                .timeout(Duration::from_millis(
                    execution_context.config.step_timeout as u64,
                ))
                .await?;

            // ignore certificate errors
            let _ = ws_client
                .send(ClientCmd {
                    method: "Security.setIgnoreCertificateErrors".to_owned(),
                    params: JsonRpcParams::Security(SecurityParams { ignore: true }),
                })
                .timeout(Duration::from_millis(
                    execution_context.config.step_timeout as u64,
                ))
                .await?;

            // open url
            let url = execution_context.config.url.to_owned();
            let _ = ws_client
                .send(ClientCmd {
                    method: "Page.navigate".to_owned(),
                    params: JsonRpcParams::Navigate(NavigateParams { url }),
                })
                .timeout(Duration::from_millis(
                    execution_context.config.step_timeout as u64,
                ))
                .await?;

            // set window size
            let width = execution_context.config.window_width;
            let height = execution_context.config.window_height;
            let _ = ws_client
                .send(ClientCmd {
                    method: "Emulation.setDeviceMetricsOverride".to_owned(),
                    params: JsonRpcParams::Metrics(MetricsParams {
                        width,
                        height,
                        mobile: false,
                        device_scale_factor: 1.0,
                        screen_orientation: ScreenOrientation {
                            angle: 0,
                            otype: "portraitPrimary".into(),
                        },
                    }),
                })
                .timeout(Duration::from_millis(
                    execution_context.config.step_timeout as u64,
                ))
                .await?;

            // start process
            loop {
                let elapsed = (Local::now().timestamp() - execution_context.ts_start) * 1000;
                debug!(
                    "[{}] Step #{} elapsed {}ms of {}ms",
                    execution_context,
                    execution_context.step_index,
                    elapsed,
                    execution_context.config.step_timeout,
                );
                if elapsed > execution_context.config.step_timeout {
                    error!(
                        "[{}] Step #{} timeout",
                        execution_context, execution_context.step_index
                    );
                    if let Err(e) = execution_context.timeout() {
                        bail!("Some error on set state - {}", e);
                    }
                    bail!(
                        "[{}] Step #{} timeout",
                        execution_context,
                        execution_context.step_index
                    );
                }

                let step = execution_context.get_step()?;
                execution_context.step_result = StepResult::InWork;
                let response = match step.kind {
                    StepKind::Screenshot => {
                        ws_client
                            .send(ClientCmd {
                                method: "Page.captureScreenshot".to_owned(),
                                params: JsonRpcParams::Capture(CaptureParams {
                                    format: "png".into(),
                                }),
                            })
                            .timeout(Duration::from_millis(
                                execution_context.config.step_timeout as u64,
                            ))
                            .await?
                    }
                    StepKind::Wait => {
                        let exec = if let Some(ref exec) = step.exec {
                            exec
                        } else {
                            unreachable!("'exec' not defined for step kind");
                        };
                        ws_client
                            .send(ClientCmd {
                                method: "Runtime.evaluate".to_owned(),
                                params: JsonRpcParams::Evaluate(EvaluateParams {
                                    expression: exec.to_string(),
                                }),
                            })
                            .timeout(Duration::from_millis(
                                execution_context.config.step_timeout as u64,
                            ))
                            .await?
                    }
                    StepKind::Value | StepKind::Exec => {
                        let exec = if let Some(ref exec) = step.exec {
                            exec
                        } else {
                            unreachable!("'exec' not defined for step kind");
                        };
                        ws_client
                            .send(ClientCmd {
                                method: "Runtime.evaluate".to_owned(),
                                params: JsonRpcParams::Evaluate(EvaluateParams {
                                    expression: exec.to_string(),
                                }),
                            })
                            .timeout(Duration::from_millis(
                                execution_context.config.step_timeout as u64,
                            ))
                            .await?
                    }
                };

                let response = response?;

                match step.kind {
                    StepKind::Screenshot => {
                        if let Some(data) = response.get("data") {
                            if let serde_json::Value::String(value) = data {
                                let mut buffer = Vec::<u8>::new();
                                base64::decode_config_buf(value, base64::STANDARD, &mut buffer)
                                    .map_err(|e| anyhow!("Faild to parse image Base64 - {}", e))?;

                                execution_context.add_screenshot_and_start_new_step(buffer)?;
                            }
                        }
                    }
                    StepKind::Exec | StepKind::Wait | StepKind::Value => {
                        // some js runtime error
                        if let Some(exception) = response.get("exceptionDetails") {
                            if let Ok(message) = serde_json::to_string(exception) {
                                error!(
                                    "[{}] Step #{} error {}",
                                    execution_context, execution_context.step_index, message
                                );
                                execution_context.error(message)?;
                            } else {
                                execution_context.error("Some error".to_owned())?;
                            }
                            System::current().stop();
                            bail!("Some js runtime error")
                        }

                        let response: serde_json::Value =
                            if let Some(response) = response.get("result") {
                                response.clone()
                            } else {
                                let message = "WS response without result attribute";
                                error!(
                                    "[{}] Step #{} {}",
                                    execution_context, execution_context.step_index, message,
                                );
                                execution_context.error(message.to_owned())?;
                                System::current().stop();
                                bail!(message)
                            };
                        debug!("Step response: {}", response);

                        match step.kind {
                            StepKind::Exec => {
                                execution_context.start_new_step()?;
                            }
                            StepKind::Wait | StepKind::Value => {
                                let value = if let Some(value) = response.get("value") {
                                    value.clone()
                                } else {
                                    let message = "WS response without value";
                                    error!(
                                        "[{}] Step #{} {}",
                                        execution_context, execution_context.step_index, message,
                                    );
                                    execution_context.error(message.to_owned())?;
                                    System::current().stop();
                                    bail!(message)
                                };
                                debug!("Step evaluation value: {}", value);
                                match value {
                                    serde_json::Value::Bool(res) => {
                                        if res {
                                            execution_context.start_new_step()?;
                                        }
                                    }
                                    serde_json::Value::Number(res) => {
                                        execution_context.add_value_and_start_new_step(
                                            res.as_f64().ok_or_else(|| {
                                                anyhow!("Can't retrieve f64 value")
                                            })?,
                                        )?;
                                    }
                                    _ => {
                                        // parse result
                                        execution_context.start_new_step()?;
                                    }
                                }
                            }
                            _ => {
                                unreachable!("yep");
                            }
                        }
                    }
                }

                if execution_context.is_done() {
                    execution_context.done()?;
                    break;
                }

                // sleep
                tokio::time::delay_for(std::time::Duration::from_millis(
                    execution_context.config.step_interval as u64,
                ))
                .await;
            }

            System::current().stop();

            Ok::<(), Error>(())
        };

        // actix::spawn(fut.map(|_| ()));
        // let _ = sys.run();
        // Ok(())

        sys.block_on(fut)
    }
}

impl actix::io::WriteHandler<WsProtocolError> for WSClient {}

impl Handler<ClientCmd> for WSClient {
    type Result = ResponseActFuture<Self, Result<serde_json::Value, Error>>;

    fn handle(&mut self, msg: ClientCmd, _ctx: &mut Context<Self>) -> Self::Result {
        let request_id = self.request_id;
        let is_ready_slot = Arc::new(AtomicBool::new(false));
        let is_ready = Arc::clone(&is_ready_slot);
        self.request_ready.insert(request_id, is_ready_slot);

        match serde_json::to_string(&JsonRpcRequest {
            id: self.request_id,
            method: msg.method,
            params: msg.params,
        }) {
            Ok(cmd) => {
                info!("send {} - {}", self.request_id, cmd);
                self.request_id += 1;
                if let Err(we) = self.writer.write(awc::ws::Message::Text(cmd)) {
                    return Box::new(actix::fut::wrap_future(futures::future::ready(Err(
                        anyhow!("Error on send messages {}", we),
                    ))));
                }
            }
            Err(e) => {
                return Box::new(actix::fut::wrap_future(futures::future::ready(Err(
                    anyhow!("Build request error {}", e),
                ))));
            }
        };

        Box::new(
            poll_fn(move |_context| -> task::Poll<Result<bool, Error>> {
                if is_ready.load(Ordering::Relaxed) {
                    task::Poll::Ready(Ok(true))
                } else {
                    task::Poll::Pending
                }
            })
            .into_actor(self)
            .map(move |_result, actor, _ctx| {
                if actor.request_ready.remove(&request_id).is_none() {
                    bail!(
                        "Request #{} is't present in request ready slots",
                        request_id
                    );
                };
                match actor.responses.remove(&request_id) {
                    None => bail!("Request #{} is't present in responses slots", request_id),
                    Some(result) => Ok(result),
                }
            }),
        )
    }
}
