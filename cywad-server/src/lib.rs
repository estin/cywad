use std::rc::Rc;
use std::str::FromStr;
use std::sync::mpsc::{channel, Receiver, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time;

use std::pin::Pin;
use std::task::{Context, Poll};

use std::borrow::Cow;
use std::time::{Duration, Instant};

use cfg_if::cfg_if;

use chrono::prelude::{DateTime, Local};
use failure::format_err;
use log::{debug, error, info};
use serde::{Deserialize, Serialize};

use cron::Schedule;

use ::http::header::AUTHORIZATION;

use bytes::Bytes;
use futures::future::FutureExt;
use futures::{Future, Stream};

use tokio::time::delay_for;

use actix_cors::Cors;
use actix_files as fs;
use actix_web::http::{ContentEncoding, StatusCode};
use actix_web::middleware::Logger;

use actix_service::{Service, Transform};
use actix_web::http::header::{HeaderName, HeaderValue};
use actix_web::{dev::ServiceRequest, dev::ServiceResponse, Error};
use futures::future::{ok, Ready};

use actix_web::HttpServer;
use actix_web::*;

use regex::Regex;

use cywad_core::{AppInfo, ResultItem, ResultItemState, SharedState, SCHEDULER_SLEEP};

cfg_if! {
    if #[cfg(feature = "png_widget")] {
        use cywad_widget::PNGWidget;
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct HeartBeat {
    pub server_datetime: DateTime<Local>,
}

impl HeartBeat {
    pub fn default() -> Self {
        HeartBeat {
            server_datetime: Local::now(),
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct ItemsResponse<'a> {
    pub info: AppInfo,
    #[serde(borrow)]
    pub items: Cow<'a, Vec<ResultItem>>,
}

#[derive(Serialize, Deserialize)]
pub struct ItemPush<'a> {
    pub server_datetime: DateTime<Local>,
    #[serde(borrow)]
    pub item: Cow<'a, ResultItem>,
}

#[derive(Clone)]
pub struct WebConfig {
    pub username: Option<String>,
    pub password: Option<String>,
    pub static_files_path: Option<String>,
    pub sse_hb_duration: Duration,
    pub sse_wakeup_duration: Duration,
}

impl WebConfig {
    pub fn new() -> Self {
        WebConfig {
            username: None,
            password: None,
            static_files_path: None,
            sse_hb_duration: Duration::from_millis(15000),
            sse_wakeup_duration: Duration::from_millis(500),
        }
    }
}

impl Default for WebConfig {
    fn default() -> Self {
        Self::new()
    }
}

pub struct WebState {
    pub shared_state: SharedState,
    pub config: WebConfig,
}

pub struct BasicAuth(Rc<BasicAuthInner>);

struct BasicAuthInner {
    pub credentials: Option<String>,
    static_re: Regex,
}

impl BasicAuth {
    pub fn new(username: Option<&str>, password: Option<&str>) -> BasicAuth {
        BasicAuth(Rc::new(BasicAuthInner {
            credentials: match (username, password) {
                (Some(u), Some(p)) => Some(base64::encode(&format!("{}:{}", u, p))),
                _ => None,
            },
            static_re: Regex::new(
                r"\.(js|json|map|css|png|jpg|jpeg|gif|ico|svg|xml|eot|otf|ttf|woff|woff2)$",
            )
            .expect("Invalid static files regex"),
        }))
    }
}

impl<S, B> Transform<S> for BasicAuth
where
    S: Service<Request = ServiceRequest, Response = ServiceResponse<B>, Error = Error>,
    S::Future: 'static,
    B: 'static,
{
    type Request = ServiceRequest;
    type Response = ServiceResponse<B>;
    type Error = Error;
    type InitError = ();
    type Transform = BasicAuthMiddleware<S>;
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ok(BasicAuthMiddleware {
            service,
            inner: self.0.clone(),
        })
    }
}

#[derive(Deserialize)]
pub struct WithTokenRequest {
    token: String,
}

pub struct BasicAuthMiddleware<S> {
    service: S,
    inner: Rc<BasicAuthInner>,
}

impl<S, B> Service for BasicAuthMiddleware<S>
where
    S: Service<Request = ServiceRequest, Response = ServiceResponse<B>, Error = Error>,
    S::Future: 'static,
    B: 'static,
{
    type Request = ServiceRequest;
    type Response = ServiceResponse<B>;
    type Error = Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>>>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.service.poll_ready(cx)
    }

    fn call(&mut self, req: ServiceRequest) -> Self::Future {
        let inner_credentials = match self.inner.credentials {
            Some(ref c) => c,
            _ => {
                return Box::pin(self.service.call(req));
            }
        };

        // skip check for static files, exclude screenshots
        if self.inner.static_re.is_match(req.path()) & !req.path().contains("/screenshot") {
            return Box::pin(self.service.call(req));
        }

        // check by token query param
        if let Ok(params) =
            actix_web::web::Query::<WithTokenRequest>::from_query(req.query_string())
        {
            if params.into_inner().token == *inner_credentials {
                return Box::pin(self.service.call(req));
            }
        } else {
            // check by AUTHORIZATION header
            if let Some(h) = req.headers().get(AUTHORIZATION) {
                if let Ok(header) = h.to_str() {
                    if let Some(credentials) = header.split("Basic ").nth(1) {
                        if inner_credentials == credentials {
                            let cywad_token = credentials.to_owned();

                            let fut = self.service.call(req);

                            return Box::pin(async move {
                                let mut res = fut.await?;
                                res.headers_mut().insert(
                                    HeaderName::from_lowercase(b"cywad-token").map_err(|e| format_err!("create header name: {}", e))?,
                                    HeaderValue::from_str(&cywad_token).map_err(|e| format_err!("create header value: {}", e))?,
                                );
                                Ok(res)
                            });
                        }
                    }
                }
            }
        }

        Box::pin(ok(req.into_response(
            HttpResponse::Unauthorized()
                .header(http::header::WWW_AUTHENTICATE, "Basic realm=\"CYWAD\"")
                .finish()
                .into_body(),
        )))
    }
}

async fn info(req: HttpRequest) -> Result<HttpResponse> {
    debug!("{:?}", req);
    let body = serde_json::to_string(&AppInfo::default())?;

    Ok(HttpResponse::build(StatusCode::OK)
        .content_type("application/json; charset=utf-8")
        .body(body))
}

async fn items(req: HttpRequest, web_state: web::Data<WebState>) -> Result<HttpResponse> {
    debug!("{:?}", req);

    let state = web_state
        .shared_state
        .read()
        .map_err(|e| format_err!("RWLock error: {}", e))?;

    let data = ItemsResponse {
        info: AppInfo::default(),
        items: Cow::Borrowed(&state.results),
    };

    // let body = serde_json::to_string(&*state.results)?;
    let body = serde_json::to_string(&data)?;

    Ok(HttpResponse::build(StatusCode::OK)
        .content_type("application/json; charset=utf-8")
        .body(body))
}

async fn update(req: HttpRequest, web_state: web::Data<WebState>) -> Result<HttpResponse> {
    debug!("{:?}", req);
    let slug = req.match_info().query("slug");
    let now = Local::now();

    let index = {
        let state = web_state
            .shared_state
            .read()
            .map_err(|e| format_err!("RWLock error: {}", e))?;

        match state.results.iter().position(|item| item.slug == slug) {
            Some(i) => i,
            None => {
                return Ok(HttpResponse::build(StatusCode::NOT_FOUND).finish());
            }
        }
    };

    // change state
    let state_changed = {
        let mut state = web_state
            .shared_state
            .write()
            .map_err(|e| format_err!("RWLock error: {}", e))?;

        match state.results.get_mut(index) {
            Some(result) => match result.state {
                ResultItemState::InWork | ResultItemState::InQueue => false,
                _ => {
                    result.state = ResultItemState::InQueue;
                    result.datetime = now;
                    result.steps_done = None;
                    result.steps_total = None;
                    result.attempt_count = None;
                    true
                }
            },
            None => false,
        }
    };

    // fire
    if state_changed {
        // broadcast
        {
            let mut state = web_state
                .shared_state
                .write()
                .map_err(|e| format_err!("RWLock error: {}", e))?;

            state.broadcast(index);
        }

        // send to the worker
        let state = web_state
            .shared_state
            .read()
            .map_err(|e| format_err!("RWLock error: {}", e))?;

        if let Some(ref tx) = state.tx {
            let tx = tx.lock().map_err(|e| format_err!("Sender error: {}", e))?;
            tx.send(index)
                .map_err(|e| format_err!("Sende error: {}", e))?;
        }
    }

    let state = web_state
        .shared_state
        .read()
        .map_err(|e| format_err!("RWLock error: {}", e))?;

    let data = ItemPush {
        server_datetime: now,
        item: Cow::Borrowed(&state.results[index]),
    };

    let body = serde_json::to_string(&data)?;

    Ok(HttpResponse::build(StatusCode::OK)
        .content_type("application/json; charset=utf-8")
        .body(body))
}

async fn screenshot(req: HttpRequest, web_state: web::Data<WebState>) -> Result<HttpResponse> {
    debug!("{:?}", req);

    let slug = req.match_info().query("slug");
    let key = req.match_info().query("key");
    let uri = format!("{}/{}", slug, key);

    // find screenshot
    let screenshot = {
        let state = web_state
            .shared_state
            .read()
            .map_err(|e| format_err!("RWLock error: {}", e))?;

        match state.results.iter().find(|item| item.slug == slug) {
            Some(item) => match item.screenshots.iter().find(|item| item.uri == uri) {
                Some(item) => Some(item.data.to_owned()),
                _ => None,
            },
            _ => None,
        }
    };

    match screenshot {
        Some(data) => Ok(HttpResponse::build(StatusCode::OK)
            .content_type("image/png")
            .body(data)),
        None => Ok(HttpResponse::build(StatusCode::NOT_FOUND).finish()),
    }
}

async fn sse(req: HttpRequest, web_state: web::Data<WebState>) -> Result<HttpResponse> {
    debug!("{:?}", req);
    let reader: Option<Sse> = {
        let (tx, rx) = channel::<ResultItem>();

        let mut state = web_state
            .shared_state
            .write()
            .map_err(|e| format_err!("RWLock error: {}", e))?;

        if let Some(ref mut tx_vec) = state.tx_vec {
            tx_vec.push(Mutex::new(tx));
            Some(Sse {
                rx,
                last_send: Instant::now(),
                hb_duration: web_state.config.sse_hb_duration,
                wakeup_duration: web_state.config.sse_wakeup_duration,
            })
        } else {
            None
        }
    };

    if let Some(sse_stream) = reader {
        Ok(HttpResponse::build(StatusCode::OK)
            .set_header(http::header::CONTENT_TYPE, "text/event-stream")
            .set_header(
                http::header::CONTENT_ENCODING,
                ContentEncoding::Identity.as_str(),
            )
            .streaming(sse_stream))
    } else {
        Ok(HttpResponse::build(StatusCode::BAD_REQUEST).finish())
    }
}

struct Sse {
    rx: Receiver<ResultItem>,
    last_send: Instant,
    hb_duration: Duration,
    wakeup_duration: Duration,
}

impl Stream for Sse {
    type Item = Result<Bytes, Error>;

    // fn poll_next(&mut self) -> Poll<Result<Option<Bytes>, Error>> {
    fn poll_next(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        match self.rx.try_recv() {
            Ok(item) => {
                self.get_mut().last_send = Instant::now();
                let data = ItemPush {
                    server_datetime: Local::now(),
                    item: Cow::Borrowed(&item),
                };
                if let Ok(payload) = serde_json::to_string(&data) {
                    Poll::Ready(Some(Ok(Bytes::copy_from_slice(
                        &format!("event: item\ndata: {}\n\n", payload).as_bytes(),
                    ))))
                } else {
                    // failed to serialize to json. terminate stream
                    Poll::Ready(None)
                }
            }
            Err(e) => {
                match e {
                    TryRecvError::Empty => {
                        let now = Instant::now();

                        if now.duration_since(self.last_send) > self.hb_duration {
                            self.get_mut().last_send = now;
                            if let Ok(payload) = serde_json::to_string(&HeartBeat::default()) {
                                return Poll::Ready(Some(Ok(Bytes::copy_from_slice(
                                    &format!("event: heartbeat\ndata: {}\n\n", payload).as_bytes(),
                                ))));
                            } else {
                                // failed to serialize to json. terminate stream
                                return Poll::Ready(None);
                            }
                        }

                        // register task to wake up stream
                        let waker = cx.waker().clone();
                        let wakeup_duration = self.wakeup_duration;
                        tokio::spawn(async move {
                            delay_for(wakeup_duration).await;
                            waker.wake();
                        });

                        Poll::Pending
                    }
                    TryRecvError::Disconnected => Poll::Ready(None),
                }
            }
        }
    }
}

#[cfg(feature = "png_widget")]
async fn png_widget(req: HttpRequest, web_state: web::Data<WebState>) -> Result<HttpResponse> {
    debug!("{:?}", req);
    let match_info = req.match_info();
    let w = match_info.query("width");
    let h = match_info.query("height");
    let f = match_info.query("fontsize");

    let widget = PNGWidget {
        width: w.parse::<u32>().unwrap_or(480),
        height: h.parse::<u32>().unwrap_or(300),
        fontsize: f.parse::<f32>().unwrap_or(12.4),
        background: {
            if let Some(background) = match_info.get("bg") {
                let color =
                    hex::decode(background).map_err(|e| format_err!("hex decode error: {}", e))?;
                if color.len() != 4 {
                    return Ok(HttpResponse::build(StatusCode::BAD_REQUEST)
                        .body("Invalid background hex color"));
                }
                let mut b = [0; 4];
                b.copy_from_slice(&color);
                Some(b)
            } else {
                None
            }
        },
    };

    Ok(HttpResponse::build(StatusCode::OK)
        .content_type("image/png")
        .body(widget.render(&web_state.shared_state)?))
}

pub fn configure_app(cfg: &mut web::ServiceConfig, web_config: WebConfig) {
    cfg.route("/api/info", web::get().to(info))
        .route("/api/items", web::get().to(items))
        .route("/api/{slug}/update", web::get().to(update))
        .route("/screenshot/{slug}/{key}", web::get().to(screenshot))
        .route("/sse", web::get().to(sse));

    #[cfg(feature = "png_widget")]
    {
        cfg.route(
            "/widget/png/{width}/{height}/{fontsize}",
            web::get().to(png_widget),
        );
    }

    if let Some(ref path) = web_config.static_files_path {
        cfg.service(fs::Files::new("/", path).index_file("index.html"));
    }
}

pub fn serve(listen: &str, state: SharedState, web_config: WebConfig) {
    let sys = actix::System::new("balance-informer");

    let server = HttpServer::new(move || {
        let web_state = WebState {
            shared_state: Arc::clone(&state),
            config: web_config.clone(),
        };

        App::new()
            .data(web_state)
            .wrap(Logger::default())
            .wrap(
                Cors::new()
                    .supports_credentials()
                    .expose_headers(vec!["cywad-token"])
                    .finish(),
            )
            .wrap(BasicAuth::new(
                web_config.username.as_ref().map(|v| v.as_ref()),
                web_config.password.as_ref().map(|v| v.as_ref()),
            ))
            .configure(|cfg| configure_app(cfg, web_config.clone()))
    })
    .bind(listen)
    .unwrap_or_else(|_| panic!("Can not bind to {}", listen))
    .shutdown_timeout(0)
    .run();

    debug!("Starting http server: {}", listen);
    actix::spawn(server.map(|_| ()));
    let _ = sys.run();
}

pub fn populate_initial_state(state: &SharedState) {
    match state.write() {
        Ok(mut state) => {
            // mark as in queue
            for result in &mut state.results {
                result.state = ResultItemState::InQueue;
                result.steps_done = None;
                result.steps_total = None;
            }

            // pass to queue
            for (index, _config) in state.configs.iter().enumerate() {
                if let Some(ref tx) = state.tx {
                    match tx.lock() {
                        Ok(tx) => {
                            match tx.send(index) {
                                Ok(()) => {}
                                Err(_) => error!("Failed send by tx channel"),
                            };
                        }
                        Err(_) => error!("Unable lock tx channel"),
                    };
                }
            }
        }
        Err(e) => error!("RWLock error: {}", e),
    };
}

pub fn process_retry(state: &SharedState) {
    let now = Local::now();
    let mut to_fire: Vec<(usize, DateTime<Local>)> = Vec::new();
    match state.read() {
        Ok(state) => {
            for (i, item) in state.results.iter().enumerate() {
                if item.state != ResultItemState::Err {
                    continue;
                }
                if let Some(config) = state.configs.get(i) {
                    if let Some(ref retry) = config.retry {
                        let attempt_index = item.attempt_count.unwrap_or(1) - 1;
                        if let Some(wait) = retry.get(attempt_index) {
                            let when = item.datetime + chrono::Duration::seconds(*wait);
                            if now <= when {
                                info!(
                                    "Retry {} attempt index {} after wait {} seconds",
                                    item.slug, attempt_index, *wait,
                                );
                                to_fire.push((i, when));
                            }
                        }
                    }
                }
            }
        }
        Err(e) => {
            error!("RWLock error: {}", e);
            return;
        }
    };

    if to_fire.is_empty() {
        return;
    }
    match state.write() {
        Ok(mut state) => {
            for to_fire_item in &to_fire {
                if let Some(item) = state.results.get_mut(to_fire_item.0) {
                    // info!(
                    //     "Retry {} next attempt would be {}",
                    //     item.slug,
                    //     item.attempt_count.unwrap_or(1) + 1,
                    // );
                    item.state = ResultItemState::InQueue;
                    item.datetime = now;
                    if item.scheduled.is_none() | (Some(to_fire_item.1) >= item.scheduled) {
                        item.scheduled = Some(to_fire_item.1);
                    }
                }
            }

            if let Some(ref tx) = state.tx {
                match tx.lock() {
                    Ok(tx) => {
                        for to_fire_item in &to_fire {
                            match tx.send(to_fire_item.0) {
                                Ok(()) => {}
                                Err(_) => error!("Failed send by tx channel"),
                            };
                        }
                    }
                    Err(_) => error!("Unable lock tx channel"),
                };
            }

            for to_fire_item in to_fire {
                state.broadcast(to_fire_item.0);
            }
        }
        Err(e) => {
            error!("RWLock error: {}", e);
        }
    }
}

pub fn run_scheduler(state: &SharedState, one_shot: bool) {
    let mut has_schedules = false;
    let mut schedules: Vec<Option<Schedule>> = vec![];

    match state.read() {
        Ok(state) => {
            // pass to queue
            for config in &state.configs {
                if let Some(ref cron_str) = config.cron {
                    // parse cron
                    match Schedule::from_str(cron_str) {
                        Ok(schedule) => {
                            has_schedules = true;
                            schedules.push(Some(schedule));
                        }
                        Err(e) => {
                            schedules.push(None);
                            error!("Cron parse error: {}", e);
                        }
                    }
                } else {
                    schedules.push(None);
                }
            }
        }
        Err(e) => error!("RWLock error: {}", e),
    };

    let sleep_duration = time::Duration::from_secs(
        std::env::var("SCHEDULER_SLEEP")
            .map(|v| v.parse::<u64>().unwrap_or(SCHEDULER_SLEEP))
            .unwrap_or(SCHEDULER_SLEEP),
    );

    loop {
        // process retry
        process_retry(state);

        // process by schedule
        if has_schedules {
            let now = Local::now();

            for (i, item) in schedules.iter().enumerate() {
                let schedule = match item {
                    Some(s) => s,
                    None => continue,
                };

                let datetime = match schedule.after(&now).take(1).next() {
                    Some(i) => i,
                    None => continue,
                };

                match state.write() {
                    Ok(mut state) => {
                        // check
                        let fire = match state.results.get_mut(i) {
                            Some(mut result) => {
                                let scheduled = Some(datetime);

                                if result.scheduled.is_some() && result.state == ResultItemState::InQueue {
                                    continue;
                                }

                                if result.scheduled.is_some() && scheduled > result.scheduled {
                                    info!("schedule {} at {:?}", result.name, datetime);
                                    result.state = ResultItemState::InQueue;
                                    result.datetime = now;
                                    result.attempt_count = None;
                                    result.scheduled = scheduled;
                                    true
                                } else {
                                    result.scheduled = scheduled;
                                    false
                                }

                            }
                            None => false,
                        };

                        // pass to queue
                        if fire {
                            if let Some(ref tx) = state.tx {
                                match tx.lock() {
                                    Ok(tx) => {
                                        match tx.send(i) {
                                            Ok(()) => {}
                                            Err(_) => error!("Failed send by tx channel"),
                                        };
                                    }
                                    Err(_) => error!("Unable lock tx channel"),
                                };
                            }

                            state.broadcast(i);
                        }
                    }
                    Err(e) => {
                        error!("RWLock error: {}", e);
                        continue;
                    }
                };
            }
        }

        if one_shot {
            return;
        }
        thread::sleep(sleep_duration);
    }
}
