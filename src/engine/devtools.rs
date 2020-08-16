use failure::Error;

use failure::{bail, format_err};
use log::{debug, error, info};
use serde::{Deserialize, Serialize};
use serde_json;
use std::process::Command;
use std::time::Duration;

use super::super::core::{
    ExecutionContext, PreviouResultItemState, ResultItemState, SharedState, StepKind, StepResult,
};
use super::traits::EngineTrait;
use super::EngineOptions;

use std::net::TcpStream;

pub struct Devtools;

use actix::io::SinkWrite;
use actix::*;
use actix_codec::Framed;
use actix_web::client;
use actix_web::http::Uri;

use awc;
use chrono::prelude::Local;
use futures::future::{FutureExt, TryFutureExt};
use futures::stream::{SplitSink, StreamExt};

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
    // WithoutParams,
}

#[derive(Serialize)]
struct JsonRpcRequest {
    id: usize,
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
    execution_context: ExecutionContext,
    request_id: usize,
    run_later_handle: Option<SpawnHandle>,
    run_interval_handle: Option<SpawnHandle>,
    to_send: Vec<(String, JsonRpcParams)>,
}

impl Actor for WSClient {
    type Context = Context<Self>;
    fn started(&mut self, ctx: &mut Self::Context) {
        info!("Actor started");

        // start check timeout
        self.run_interval_handle = Some(ctx.run_interval(
            Duration::from_millis(self.execution_context.config.step_interval.into()),
            |a, ctx| {
                let context = &mut a.execution_context;

                if context.is_done() {
                    if let Some(handle) = a.run_interval_handle {
                        ctx.cancel_future(handle);
                    }
                    return;
                }

                // check timeout
                let elapsed = (Local::now().timestamp() - context.ts_start) * 1000;
                debug!(
                    "[{}] Step #{} elapsed {}ms of {}ms",
                    context, context.step_index, elapsed, context.config.step_timeout,
                );
                if elapsed > context.config.step_timeout {
                    error!("[{}] Step #{} timeout", context, context.step_index);
                    if let Err(e) = context.timeout() {
                        error!("Some error on set state - {}", e);
                    }
                    System::current().stop();
                }
            },
        ));
    }

    fn stopped(&mut self, _: &mut Self::Context) {
        info!("actor stoped");
        System::current().stop();
    }
}

impl WSClient {
    fn _send(&mut self) {
        if self.to_send.is_empty() {
            return;
        }

        let (method, params) = self.to_send.remove(0);

        match serde_json::to_string(&JsonRpcRequest {
            id: self.request_id,
            method,
            params,
        }) {
            Ok(cmd) => {
                info!("send {} - {}", self.request_id, cmd);
                self.request_id += 1;
                if let Err(we) = self.writer.write(awc::ws::Message::Text(cmd)) {
                    if let Err(se) = self
                        .execution_context
                        .error(format!("Write request to ws error - {}", we))
                    {
                        error!("Some error on set state - {}", se);
                    }
                    System::current().stop();
                }
            }
            Err(e) => {
                info!("Build request error: {}", e);
                if let Err(e) = self
                    .execution_context
                    .error(format!("Serialize request error - {}", e))
                {
                    error!("Some error on set state - {}", e);
                }
                System::current().stop();
            }
        }
    }
    fn send(&mut self, method: &str, params: JsonRpcParams) {
        self.to_send.push((method.into(), params));
    }

    fn cleanup(&mut self, _ctx: &mut Context<Self>) {
        // reset current tab
        self.send(
            "Page.navigate",
            JsonRpcParams::Navigate(NavigateParams {
                url: "chrome://system/".to_owned(),
            }),
        );
        // not supported?
        // self.send("Network.clearBrowserCache", JsonRpcParams::WithoutParams);
        // self.send("Network.clearBrowserCookies", JsonRpcParams::WithoutParams);
    }

    fn execute_step(&mut self, ctx: &mut Context<Self>) {
        // check job done and exit
        if self.execution_context.is_done() {
            if let Err(e) = self.execution_context.done() {
                error!("Some error on set state - {}", e);
            }

            // reset current tab
            self.cleanup(ctx);

            // wait a litle and stop
            ctx.run_later(Duration::from_millis(300), |_, _| {
                System::current().stop();
            });
            return;
        }

        debug!(
            "[{}] Step #{} try start",
            self.execution_context, self.execution_context.step_index
        );

        // prevent double execution
        if self.execution_context.step_result != StepResult::Idle {
            return;
        }

        // prevent double execution
        if self.execution_context.step_result == StepResult::InWork {
            debug!(
                "[{}] Step #{} not done yet...skip... iteration",
                self.execution_context, self.execution_context.step_index
            );
            return;
        }

        self.run_later_handle = Some(ctx.run_later(
            Duration::from_millis(self.execution_context.config.step_interval.into()),
            |a, ctx| {
                let do_send = a.to_send.is_empty();

                match a.execution_context.get_step() {
                    Ok(ref step) => {
                        a.execution_context.step_result = StepResult::InWork;
                        match step.kind {
                            StepKind::Screenshot => {
                                a.send(
                                    "Page.captureScreenshot",
                                    JsonRpcParams::Capture(CaptureParams {
                                        format: "png".into(),
                                    }),
                                );
                            }
                            StepKind::Wait | StepKind::Value | StepKind::Exec => {
                                let exec = if let Some(ref exec) = step.exec {
                                    exec
                                } else {
                                    unreachable!("'exec' not defined for step kind");
                                };

                                a.send(
                                    "Runtime.evaluate",
                                    JsonRpcParams::Evaluate(EvaluateParams {
                                        expression: exec.to_string(),
                                    }),
                                );
                            }
                        };
                    }
                    Err(_) => {
                        if let Some(handle) = a.run_later_handle {
                            ctx.cancel_future(handle);
                        }
                    }
                };

                if do_send {
                    a._send();
                }
            },
        ));
    }

    fn process_response(&mut self, response: serde_json::Value) -> Result<(), Error> {
        let step = &self.execution_context.get_step()?;

        // send next message
        self._send();

        match step.kind {
            StepKind::Screenshot => {
                if let Some(data) = response.get("data") {
                    if let serde_json::Value::String(value) = data {
                        let mut buffer = Vec::<u8>::new();
                        base64::decode_config_buf(value, base64::STANDARD, &mut buffer)
                            .map_err(|e| format_err!("Faild to parse image Base64 - {}", e))?;

                        self.execution_context
                            .add_screenshot_and_start_new_step(buffer)?;
                    }
                }
            }
            StepKind::Exec | StepKind::Wait | StepKind::Value => {
                // some js runtime error
                if let Some(exception) = response.get("exceptionDetails") {
                    if let Ok(message) = serde_json::to_string(exception) {
                        error!(
                            "[{}] Step #{} error {}",
                            self.execution_context, self.execution_context.step_index, message
                        );
                        self.execution_context.error(message)?;
                    } else {
                        self.execution_context.error("Some error".to_owned())?;
                    }
                    System::current().stop();
                    return Ok(());
                }

                let response: serde_json::Value = if let Some(response) = response.get("result") {
                    response.clone()
                } else {
                    return Ok(());
                };
                debug!("Step response: {}", response);

                match step.kind {
                    StepKind::Exec => {
                        if let Err(e) = self.execution_context.start_new_step() {
                            error!("Some error on set state - {}", e);
                        }
                    }
                    StepKind::Wait | StepKind::Value => {
                        let value = if let Some(value) = response.get("value") {
                            value.clone()
                        } else {
                            return Ok(());
                        };
                        debug!("Step evaluation value: {}", value);
                        match value {
                            serde_json::Value::Bool(res) => {
                                if res {
                                    self.execution_context.start_new_step()?;
                                }
                            }
                            serde_json::Value::Number(res) => {
                                self.execution_context
                                    .add_value_and_start_new_step(res.as_f64().ok_or_else(
                                        || format_err!("Can't retrieve f64 value"),
                                    )?)?;
                            }
                            _ => {
                                // parse result
                                self.execution_context.start_new_step()?;
                            }
                        }
                    }
                    _ => {
                        unreachable!("yep");
                    }
                }
            }
        }

        Ok(())
    }
}

// Handle server websocket messages
impl StreamHandler<Result<Frame, WsProtocolError>> for WSClient {
    fn started(&mut self, ctx: &mut Self::Context) {
        info!("Connected");

        // start excetion
        info!("start execution");

        // reset current tab
        self.cleanup(ctx);

        // ignore certificate errors
        self.send(
            "Security.setIgnoreCertificateErrors",
            JsonRpcParams::Security(SecurityParams { ignore: true }),
        );

        // open url
        let url = self.execution_context.config.url.to_owned();
        self.send(
            "Page.navigate",
            JsonRpcParams::Navigate(NavigateParams { url }),
        );

        // set window size
        let width = self.execution_context.config.window_width;
        let height = self.execution_context.config.window_height;
        self.send(
            "Emulation.setDeviceMetricsOverride",
            JsonRpcParams::Metrics(MetricsParams {
                width,
                height,
                mobile: false,
                device_scale_factor: 1.0,
                screen_orientation: ScreenOrientation {
                    angle: 0,
                    otype: "portraitPrimary".into(),
                },
            }),
        );

        // start execute first step
        self.execute_step(ctx);

        self._send();
    }

    // fn handle(&mut self, msg: ws::Message, ctx: &mut Self::Context) {
    fn handle(&mut self, msg: Result<awc::ws::Frame, WsProtocolError>, ctx: &mut Self::Context) {
        let result: serde_json::Value = {
            if let Ok(awc::ws::Frame::Text(txt)) = msg {
                info!("Server: {:?}", txt);
                match serde_json::from_slice::<serde_json::Value>(&txt) {
                    Ok(v) => {
                        if let Some(response_id) = v.get("id") {
                            if response_id != self.request_id - 1 {
                                debug!(
                                    "response id {} but expects {}. skip",
                                    response_id,
                                    self.request_id - 1,
                                );
                                return;
                            }

                            info!("Context - idle");
                            self.execution_context.step_result = StepResult::Idle;

                            if let Some(result) = v.get("result") {
                                result.clone()
                            } else {
                                return;
                            }
                        } else {
                            return;
                        }
                    }
                    Err(e) => {
                        error!("Some error on ws response parse - {}", e);
                        return;
                    }
                }
            } else {
                return;
            }
        };

        if !self.execution_context.is_done() {
            if let Err(e) = self.process_response(result) {
                error!("Some error on ws response proccessing - {}", e);
            }
            self.execute_step(ctx);
        }
    }

    fn finished(&mut self, ctx: &mut Self::Context) {
        info!("Server disconnected");
        if let Err(e) = self.execution_context.error("Server disconnected".into()) {
            error!("Some error on set state - {}", e);
        }
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
                    .map_err(|e| {
                        format_err!("browser command failed to start {} - {}", command, e)
                    })?;
                info!("Browser started with id: {}", browser.id());
                Some(browser)
            }
            None => None,
        };

        // now wait to ensure browser is starts
        let uri = engine_options
            .endpoint
            .parse::<Uri>()
            .map_err(|e| format_err!("On parse endopoint - {}", e))?;
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
                .map_err(|e| format_err!("browser kill error: {}", e))?;

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
            let s = state
                .read()
                .map_err(|e| format_err!("RWLock error: {}", e))?;
            s.configs[config_index].clone()
        };

        // initialize result
        {
            let mut state = state
                .write()
                .map_err(|e| format_err!("RWLock error: {}", e))?;
            if let Some(item) = state.results.get_mut(config_index) {
                // save previous success state
                if item.state == ResultItemState::Done {
                    item.previous = Some(PreviouResultItemState {
                        datetime: item.datetime,
                        values: (&item.values).to_vec(),
                        screenshots: (&item.screenshots).to_vec(),
                    });
                }

                // reset state
                item.state = ResultItemState::InWork;
                item.values.clear();
                item.screenshots.clear();
                item.steps_done = Some(0);
                item.steps_total = Some(config.steps.len());
                item.attempt_count = Some(item.attempt_count.unwrap_or(0) + 1);
            }

            state.broadcast(config_index);
        }

        let sys = System::new("cywad");

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
                    format_err!("Failed to connect to debug port - {}", e)
                })
                .await?;
            let data = response
                .json::<Vec<DevToolsResponse>>()
                .map_err(|e| {
                    System::current().stop();
                    format_err!("Failed to parse response - {}", e)
                })
                .await?;

            let item = &data[0];

            let client = awc::Client::new();

            let (_response, framed) = client
                .ws((&item.web_socket_debugger_url).replace("ws", "http"))
                .max_frame_size(engine_options.max_frame_size)
                .connect()
                .map_err(|e| {
                    System::current().stop();
                    format_err!("Failed to connect to websocket - {}", e)
                })
                .await?;

            let (sink, stream) = framed.split();
            WSClient::create(move |ctx| {
                WSClient::add_stream(stream, ctx);
                WSClient {
                    writer: SinkWrite::new(sink, ctx),
                    request_id: 0,
                    execution_context: ExecutionContext::new(config, config_index, state),
                    run_later_handle: None,
                    run_interval_handle: None,
                    to_send: Vec::new(),
                }
            });

            Ok::<(), Error>(())
        };

        actix::spawn(fut.map(|_| ()));

        let _ = sys.run();

        Ok(())
    }
}

impl actix::io::WriteHandler<WsProtocolError> for WSClient {}
