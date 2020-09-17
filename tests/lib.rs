use cron::Schedule;
use failure::format_err;

use lazy_static::lazy_static;
use log::debug;
use std::str::FromStr;

use actix_web::*;

use actix_web::{test, web, App};

use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddrV4, TcpListener};
use std::panic;
use std::str;
use std::sync::mpsc;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex, RwLock};
use std::thread;
use std::time::Duration;

use actix_web::http::StatusCode;
use actix_web::HttpServer;

use futures_util::future::FutureExt;
use futures_util::stream::StreamExt;

use cywad_core::{
    validate_config, Config, EngineOptions, EngineTrait, ResultItem, ResultItemState,
    ScreenshotItem, SharedState, State,
};

#[derive(Debug)]
struct MockServerState {
    base_url: Option<String>,
    is_running: bool,
    test_index: usize,
    content_by_test_id: HashMap<usize, String>,
}

#[derive(Debug)]
struct MockServerInfo {
    url: String,
}

lazy_static! {
    static ref MOCK_SERVER_STATE: Mutex<MockServerState> = {
        Mutex::new(MockServerState {
            base_url: None,
            is_running: false,
            test_index: 0,
            content_by_test_id: HashMap::new(),
        })
    };
}

fn start_server(html: &str) -> Result<MockServerInfo, failure::Error> {
    {
        let mut state = MOCK_SERVER_STATE
            .lock()
            .map_err(|e| format_err!("Lock error: {}", e))?;

        if !state.is_running {
            state.is_running = true;

            let (tx, rx): (Sender<String>, Receiver<String>) = mpsc::channel();

            // start server in thread
            thread::spawn(move || {
                // find free port
                let listen = {
                    let loopback = Ipv4Addr::new(127, 0, 0, 1);
                    let socket = SocketAddrV4::new(loopback, 0);
                    let listener = TcpListener::bind(socket).expect("bind error");
                    listener.local_addr().expect("get local addr error")
                };

                // configure mock server
                let sys = actix::System::new("mock-server");

                async fn index(req: HttpRequest) -> Result<HttpResponse> {
                    debug!("Mock server request {:?}", req);

                    let mut content: Option<String> = None;
                    if let Some(param) = req.query_string().split("index=").nth(1) {
                        if let Ok(ref index) = param.parse::<usize>() {
                            let state = MOCK_SERVER_STATE.lock().expect("lock error");
                            if let Some(value) = state.content_by_test_id.get(index) {
                                content = Some(value.to_owned());
                            }
                        }
                    }
                    if let Some(value) = content {
                        debug!("Mock server request {:?} - response: OK", req);
                        Ok(HttpResponse::build(StatusCode::OK)
                            .content_type("text/html; charset=utf-8")
                            .body(value))
                    } else {
                        debug!("Mock server request {:?} - response: NOT_FOUND", req);
                        Ok(HttpResponse::build(StatusCode::NOT_FOUND).finish())
                    }
                }

                debug!(
                    "Listening on http://{} {:?}",
                    listen,
                    thread::current().id(),
                );

                let fut = async move {
                    HttpServer::new(|| App::new().route("/", web::get().to(index)))
                        .bind(listen)
                        .unwrap_or_else(|_| panic!("Can not bind to {}", listen))
                        .shutdown_timeout(0)
                        .workers(1)
                        .run()
                        .await?;
                    Ok::<(), Error>(())
                };

                tx.send(format!("http://{}/", listen))
                    .expect("tx send error");

                debug!("Starting mock http server: {}", listen);
                actix::spawn(fut.map(|_| ()));
                let _ = sys.run();
            });

            // wait until server starts
            state.base_url = Some(rx.recv().map_err(|e| format_err!("rx recv error: {}", e))?);
        }
    }

    {
        let mut state = MOCK_SERVER_STATE
            .lock()
            .map_err(|e| format_err!("Lock error: {}", e))?;
        let test_index = state.test_index;
        state
            .content_by_test_id
            .insert(test_index, html.to_string());
    }
    {
        let state = MOCK_SERVER_STATE
            .lock()
            .map_err(|e| format_err!("Lock error: {}", e))?;
        let url = state
            .base_url
            .as_ref()
            .ok_or_else(|| format_err!("Base url empty"))?;
        let info = MockServerInfo {
            url: format!("{}?index={}", url, state.test_index),
        };
        Ok(info)
    }
}

#[test]
fn test_validate_config() -> Result<(), failure::Error> {
    let config_step_without_exec = r#"
        url = "mock"
        name = "test"
        window_width = 1280
        window_height = 1024
        step_timeout = 3000
        step_interval = 10

        [[steps]]
        kind = "wait"
    "#;
    let config: Config = toml::from_str(&config_step_without_exec)
        .map_err(|e| format_err!("on load config - {}", e))?;

    debug!("Config: {:#?}", config);
    assert_eq!(
        &format!("{}", validate_config(&config).unwrap_err()),
        "'wait/value/exec' step #1 without 'exec' field",
    );

    let config_value_without_key = r#"
        url = "mock"
        name = "test"
        window_width = 1280
        window_height = 1024
        step_timeout = 3000
        step_interval = 10

        [[steps]]
        kind = "value"
        exec = "return 1;"
    "#;
    let config: Config = toml::from_str(&config_value_without_key)
        .map_err(|e| format_err!("on load config - {}", e))?;

    debug!("Config: {:#?}", config);
    assert_eq!(
        &format!("{}", validate_config(&config).unwrap_err()),
        "'value' step #1 without 'key' field",
    );

    let config_invalid_cron = r#"
        url = "mock"
        name = "test"
        cron = "some cron schedule"
        window_width = 1280
        window_height = 1024
        step_timeout = 3000
        step_interval = 10

        [[steps]]
        kind = "exec"
        exec = "return 1;"
    "#;

    let config: Config =
        toml::from_str(&config_invalid_cron).map_err(|e| format_err!("on load config - {}", e))?;

    debug!("Config: {:#?}", config);
    assert_eq!(
        &format!("{}", validate_config(&config).unwrap_err()),
        "\'cron\' field - invalid expression: Invalid cron expression.",
    );
    Ok(())
}

// Work around testing for gtk and single thread restriction
#[test]
fn test_engine_summary() -> Result<(), failure::Error> {
    test_engine_success()?;
    test_engine_error()?;
    test_engine_timeout()?;
    Ok(())
}

fn test_engine_success() -> Result<(), failure::Error> {
    let _ = env_logger::try_init();
    let info = start_server(include_str!("data/index.html"))?;

    debug!("Server info: {:?}", info);

    let config_toml = r#"
        url = "mock"
        name = "test success"
        cron = "0   0   8     *       *  *  *"
        window_width = 1280
        window_height = 1024
        step_timeout = 1000
        step_interval = 100

        [[steps]]
        kind = "wait"
        exec = """(function () {
            return document.querySelector(".value1") ? true : false;
        })();
        """
        [[steps]]
        kind = "value"
        key = "value_name1"
        exec = """(function () {
            var value = parseFloat(document.querySelector('.value1')
                .innerHTML
                .replace(/[^0-9\\.,]+/g, '')
                .replace(',', '.'));
            window.value1 = value;
            return value;
        })();
        """
            [[steps.levels]]
            name = "green"
            more = 50
            [[steps.levels]]
            name = "yellow"
            less = 30
            [[steps.levels]]
            name = "red"
            less = 10
        [[steps]]
        kind = "value"
        key = "value_name2"
        exec = """(function () {
            var value = parseFloat(document.querySelector('.value2')
                .innerHTML
                .replace(/[^0-9\\.,]+/g, '')
                .replace(',', '.'));
            return value;
        })();
        """
            [[steps.levels]]
            name = "green"
            more = 550
            [[steps.levels]]
            name = "yellow"
            less = 300
            [[steps.levels]]
            name = "red"
            less = 100
        [[steps]]
        kind = "exec"
        exec = """(function() {
            var clock = document.createElement("div");
            clock.innerHTML = 'CYWAD: ' + new Date() + ' balance: ' + window.value1;
            clock.className = "informer-clock";
            clock.style.color = "red";
            clock.style.fontWeight = 'bold';
            clock.style.position = 'absolute';
            clock.style.top = '210px';
            clock.style.left = '200px';
            document.body.appendChild(clock);
        })()"""
        [[steps]]
        kind = "screenshot"
    "#;

    let mut config: Config =
        toml::from_str(&config_toml).map_err(|e| format_err!("on load config - {}", e))?;
    config.url = info.url;

    debug!("Config: {:#?}", config);

    let mut engine = cywad::engine::new();
    let state: SharedState = Arc::new(RwLock::new(State {
        configs: Vec::new(),
        results: Vec::new(),
        tx: None,
        tx_vec: None,
    }));

    // initialize
    let state_clone = state.clone();
    {
        let mut state = state_clone.write().expect("RwLock error");
        state.results.push(ResultItem::new(&config.name));
        state.configs.push(config);
    }

    assert!(engine
        .execute(0, state_clone, EngineOptions::default())
        .is_ok());
    let state = state.read().expect("RwLock error");
    let result = &state.results[0];
    assert!(result.is_ok());
    assert_eq!(result.values[0].value, 100.0);
    assert_eq!(result.values[0].level, Some("green".to_string()));
    assert_eq!(result.values[1].value, 200.0);
    assert_eq!(result.values[1].level, Some("yellow".to_string()));
    assert_eq!(result.screenshots.len(), 1);
    Ok(())
}

fn test_engine_timeout() -> Result<(), failure::Error> {
    let _ = env_logger::try_init();
    let info = start_server(include_str!("data/index.html"))?;

    debug!("Server info: {:?}", info);

    let config_toml = r#"
        url = "mock"
        name = "test - timeout"
        cron = "0   0   8     *       *  *  *"
        window_width = 1280
        window_height = 1024
        step_timeout = 500
        step_interval = 10

        [[steps]]
        kind = "wait"
        key = "value_name"
        exec = """(function () {
            return document.querySelector(".not-existed-value") ? true : false;
        })();
        """
            [[steps.levels]]
            name = "green"
            more = 0
        [[steps]]
        kind = "screenshot"
    "#;

    let mut config: Config =
        toml::from_str(&config_toml).map_err(|e| format_err!("on load config - {}", e))?;
    config.url = info.url;

    debug!("Config: {:#?}", config);

    let mut engine = cywad::engine::new();

    let state: SharedState = Arc::new(RwLock::new(State {
        configs: Vec::new(),
        results: Vec::new(),
        tx: None,
        tx_vec: None,
    }));

    // initialize
    let state_clone = Arc::clone(&state);
    {
        let mut state = state_clone.write().expect("RwLock error");
        state.results.push(ResultItem::new(&config.name));
        state.configs.push(config);
    }

    assert!(engine
        .execute(0, state_clone, EngineOptions::default())
        .is_ok());
    let state = state.read().expect("RwLock error");
    let result = &state.results[0];
    assert!(result.is_err());
    assert!(result.values.is_empty());
    // assert_eq!(result.screenshots.len(), 1);
    Ok(())
}

fn test_engine_error() -> Result<(), failure::Error> {
    let _ = env_logger::try_init();
    let info = start_server(include_str!("data/index.html"))?;

    debug!("Server info: {:?}", info);

    let config_toml = r#"
        url = "mock"
        name = "test - js error"
        cron = "0   0   8     *       *  *  *"
        window_width = 1280
        window_height = 1024
        step_timeout = 3000
        step_interval = 10
        [[steps]]
        kind = "value"
        key = "value_name"
        exec = """(function () {
            return null();
        })();
        """
            [[steps.levels]]
            name = "green"
            more = 0
        [[steps]]
        kind = "screenshot"
    "#;

    let mut config: Config =
        toml::from_str(&config_toml).map_err(|e| format_err!("on load config - {}", e))?;
    config.url = info.url;

    debug!("Config: {:#?}", config);

    let mut engine = cywad::engine::new();
    let state: SharedState = Arc::new(RwLock::new(State {
        configs: Vec::new(),
        results: Vec::new(),
        tx: None,
        tx_vec: None,
    }));

    // initialize
    let state_clone = Arc::clone(&state);
    {
        let mut state = state_clone.write().expect("RwLock error");
        state.results.push(ResultItem::new(&config.name));
        state.configs.push(config);
    }

    assert!(engine
        .execute(0, state_clone, EngineOptions::default())
        .is_ok());
    let state = state.read().expect("RwLock error");
    let result = &state.results[0];
    assert!(result.is_err());
    assert!(result.values.is_empty());
    // assert_eq!(result.screenshots.len(), 1);
    Ok(())
}

#[cfg(any(feature = "devtools", feature = "server"))]
#[actix_rt::test]
async fn test_server() -> Result<(), failure::Error> {
    let config_toml = r#"
        url = "mock"
        name = "some-test-name"
        cron = "0   0   8     *       *  *  *"
        window_width = 1280
        window_height = 1024
        step_timeout = 3000
        step_interval = 10
        [[steps]]
        kind = "wait"
        exec = """(function () {
            return document.querySelector(".value") ? true : false;
        })();
        """
        [[steps]]
        kind = "value"
        key = "value_name"
        exec = """(function () {
            var value = parseFloat(document.querySelector('.value')
                .innerHTML
                .replace(/[^0-9\\.,]+/g, '')
                .replace(',', '.'));
            window.value = value;
            return value;
        })();
        """
            [[steps.levels]]
            name = "green"
            more = 150
            [[steps.levels]]
            name = "yellow"
            less = 150
            [[steps.levels]]
            name = "red"
            less = 50
        [[steps]]
        kind = "exec"
        exec = """(function() {
            var clock = document.createElement("div");
            clock.innerHTML = 'CYWAD: ' + new Date() + ' balance: ' + window.value;
            clock.className = "informer-clock";
            clock.style.color = "red";
            clock.style.fontWeight = 'bold';
            clock.style.position = 'absolute';
            clock.style.top = '210px';
            clock.style.left = '200px';
            document.body.appendChild(clock);
        })()"""
        [[steps]]
        kind = "screenshot"
    "#;

    let config: Config =
        toml::from_str(&config_toml).map_err(|e| format_err!("on load config - {}", e))?;

    let (tx, job_rx): (Sender<usize>, _) = mpsc::channel();

    let state: SharedState = Arc::new(RwLock::new(State {
        configs: Vec::new(),
        results: Vec::new(),
        tx: Some(Mutex::new(tx)),
        tx_vec: Some(Vec::new()),
    }));

    // initialize
    let state_clone = Arc::clone(&state);
    {
        let mut state = state_clone
            .write()
            .map_err(|e| format_err!("lock error - {}", e))?;

        let mut result_item = ResultItem::new(&config.name);
        result_item.slug = "test".to_string();
        result_item.screenshots = vec![ScreenshotItem {
            name: "test".to_string(),
            uri: "test/test".to_string(),
            data: vec![1, 2, 3],
        }];
        state.results.push(result_item);
        state.configs.push(config.clone());
    }

    let mut web_config = cywad_server::WebConfig::default();
    web_config.sse_hb_duration = Duration::from_millis(10);
    web_config.sse_wakeup_duration = Duration::from_millis(10);

    let mut srv = test::init_service({
        let web_state = cywad_server::WebState {
            shared_state: Arc::clone(&state),
            config: web_config.clone(),
        };
        App::new()
            .data(web_state)
            .configure(|cfg| cywad_server::configure_app(cfg, web_config.clone()))
    })
    .await;

    // info
    let req = test::TestRequest::get().uri("/api/info").to_request();
    // .map_err(|e| format_err!("actix error: {}", e))
    let resp = test::call_service(&mut srv, req).await;
    assert!(resp.status().is_success());

    // items
    let req = test::TestRequest::get().uri("/api/items").to_request();
    let bytes = test::read_response(&mut srv, req).await;
    let body = str::from_utf8(&bytes)?;
    let data: cywad_server::ItemsResponse = serde_json::from_str(body)?;
    debug!("items {:?}", data.items);
    assert_eq!(data.items.len(), 1);
    let item = &data.items[0];
    assert!(item.values.is_empty());
    assert!(item.error.is_none());
    assert_eq!(item.name, config.name);
    assert_eq!(item.state, ResultItemState::Idle);
    assert_eq!(item.screenshots.len(), 1);

    // screenshot
    let req = test::TestRequest::get()
        .uri(&format!("/screenshot/{}/test", item.slug))
        .to_request();
    let resp = test::call_service(&mut srv, req).await;
    assert!(resp.status().is_success());
    assert_eq!(
        resp.headers().get(http::header::CONTENT_TYPE),
        Some(&actix_web::http::header::HeaderValue::from_str(
            "image/png"
        )?),
    );

    // update
    let req = test::TestRequest::get()
        .uri(&format!("/api/{}/update", item.slug))
        .to_request();
    let resp = test::call_service(&mut srv, req).await;
    assert!(resp.status().is_success());

    // items
    let req = test::TestRequest::get().uri("/api/items").to_request();
    let bytes = test::read_response(&mut srv, req).await;
    let body = str::from_utf8(&bytes)?;
    let data: cywad_server::ItemsResponse = serde_json::from_str(body)?;
    // let data: server::ItemsResponse = test::read_response_json(&mut srv, req);
    debug!("items {:?}", data.items);
    assert_eq!(data.items.len(), 1);
    let item = &data.items[0];
    assert!(item.values.is_empty());
    assert!(item.error.is_none());
    assert_eq!(item.name, config.name);
    assert_eq!(item.state, ResultItemState::InQueue);
    assert_eq!(item.screenshots.len(), 1);
    assert!(job_rx.recv().is_ok());

    // sse
    let item_clone = item.clone();
    let req = test::TestRequest::get().uri("/sse").to_request();
    let mut resp = test::call_service(&mut srv, req).await;
    assert!(resp.status().is_success());

    {
        let state = state_clone.read().map_err(|e| format_err!("State read error {}", e))?;
        let tx_vec = state.tx_vec.as_ref().ok_or_else(|| format_err!("tx vec is None"))?;
        assert_eq!(tx_vec.len(), 1);
    }

    let handle = thread::spawn(move || -> Result<(), failure::Error> {
        let state = state_clone.read().map_err(|e| format_err!("State error {}", e))?;
        let v = state.tx_vec.as_ref().ok_or_else(|| format_err!("tx vec is None"))?;
        for ref mut tx in v.iter() {
            let sender = tx.lock().map_err(|e| format_err!("Lock error {}", e))?;
            sender.send(item_clone.clone())?;
        }
        Ok(())
    });
    let _ = handle.join().map_err(|_| format_err!("Join error"))?;

    // item
    let (bytes, resp) = resp.take_body().into_future().await;
    let bytes = bytes.ok_or_else(|| format_err!("bytes empty"))?.map_err(|e| format_err!("Unwrap bytes error {}", e))?;
    let body = str::from_utf8(&bytes)?;
    let payload = body.split("data: ").nth(1).ok_or_else(|| format_err!("Empty payload"))?;
    let data: cywad_server::ItemPush = serde_json::from_str(payload)?;
    assert_eq!(item.slug, data.item.slug);

    // heartbeat
    let (bytes, _) = resp.into_future().await;
    let bytes = bytes.ok_or_else(|| format_err!("bytes empty"))?.map_err(|e| format_err!("Unwrap bytes error {}", e))?;
    let body = str::from_utf8(&bytes)?;
    assert!(body.contains("event: heartbeat"));
    let payload = body.split("data: ").nth(1).ok_or_else(|| format_err!("Empty payload"))?;
    let _heartbeat: cywad_server::HeartBeat = serde_json::from_str(payload)?;

    // widget
    let req = test::TestRequest::get()
        .uri("/widget/png/480/300/12")
        .to_request();
    let resp = test::call_service(&mut srv, req).await;
    assert!(resp.status().is_success());
    assert_eq!(
        resp.headers().get(http::header::CONTENT_TYPE),
        Some(&actix_web::http::header::HeaderValue::from_str(
            "image/png"
        )?),
    );
    Ok(())
}

#[actix_rt::test]
async fn test_basic_auth() -> Result<(), failure::Error> {
    let state: SharedState = Arc::new(RwLock::new(State {
        configs: Vec::new(),
        results: Vec::new(),
        tx: None,
        tx_vec: None,
    }));

    let mut web_config = cywad_server::WebConfig::default();
    web_config.username = Some("test".to_owned());
    web_config.password = Some("test".to_owned());

    let mut srv = test::init_service({
        let web_state = cywad_server::WebState {
            shared_state: Arc::clone(&state),
            config: web_config.clone(),
        };
        App::new()
            .data(web_state)
            .wrap(cywad_server::BasicAuth::new(
                web_config.username.as_ref().map(|v| v.as_ref()),
                web_config.password.as_ref().map(|v| v.as_ref()),
            ))
            .configure(|cfg| cywad_server::configure_app(cfg, web_config.clone()))
    })
    .await;

    // info
    let req = test::TestRequest::get().uri("/api/info").to_request();
    let resp = test::call_service(&mut srv, req).await;
    assert!(resp.status().is_client_error());
    assert_eq!(
        resp.headers().get(http::header::WWW_AUTHENTICATE),
        Some(&actix_web::http::header::HeaderValue::from_str(
            "Basic realm=\"CYWAD\""
        )?),
    );

    let req = test::TestRequest::get()
        .uri("/api/info")
        .header(
            http::header::AUTHORIZATION,
            format!("Basic {}", base64::encode("test:test")),
        )
        .to_request();
    let resp = test::call_service(&mut srv, req).await;
    assert!(resp.status().is_success());

    let req = test::TestRequest::get()
        .uri("/api/info")
        .header(
            http::header::AUTHORIZATION,
            format!("Basic {}", base64::encode("test:wrong")),
        )
        .to_request();
    let resp = test::call_service(&mut srv, req).await;
    assert!(resp.status().is_client_error());
    assert_eq!(
        resp.headers().get(http::header::WWW_AUTHENTICATE),
        Some(&actix_web::http::header::HeaderValue::from_str(
            "Basic realm=\"CYWAD\""
        )?),
    );

    let req = test::TestRequest::get()
        .uri("/widget/png/480/300/12")
        .to_request();
    let resp = test::call_service(&mut srv, req).await;
    assert!(resp.status().is_client_error());
    assert_eq!(
        resp.headers().get(http::header::WWW_AUTHENTICATE),
        Some(&actix_web::http::header::HeaderValue::from_str(
            "Basic realm=\"CYWAD\""
        )?),
    );

    let req = test::TestRequest::get()
        .uri(&format!(
            "/widget/png/480/300/12?token={}",
            base64::encode("test:test")
        ))
        .to_request();
    let resp = test::call_service(&mut srv, req).await;
    assert!(resp.status().is_success());

    let req = test::TestRequest::get()
        .uri(&format!(
            "/widget/png/480/300/12?token={}",
            base64::encode("test:wrong")
        ))
        .to_request();
    let resp = test::call_service(&mut srv, req).await;
    assert!(resp.status().is_client_error());
    assert_eq!(
        resp.headers().get(http::header::WWW_AUTHENTICATE),
        Some(&actix_web::http::header::HeaderValue::from_str(
            "Basic realm=\"CYWAD\""
        )?),
    );
    Ok(())
}

#[test]
fn test_retry() {
    let _ = env_logger::try_init();

    let config_toml = r#"
        url = "mock"
        name = "test retry"
        retry = [ 10, 15, 60 ]
        window_width = 1280
        window_height = 1024
        step_timeout = 10000
        step_interval = 10

        [[steps]]
        kind = "wait"
        exec = """(function () {
            return document.querySelector(".value1") ? true : false;
        })();
        """
        [[steps]]
        kind = "screenshot"
    "#;

    let config: Config = toml::from_str(&config_toml).expect("parse config error");

    debug!("Config: {:#?}", config);

    assert!(config.retry.is_some());

    let state: SharedState = Arc::new(RwLock::new(State {
        configs: Vec::new(),
        results: Vec::new(),
        tx: None,
        tx_vec: None,
    }));

    // initialize
    let state_clone = state;
    {
        let mut state = state_clone.write().expect("RwLock error");
        state.results.push(ResultItem::new(&config.name));
        state.configs.push(config);
    }

    // attempt == 1
    cywad_server::populate_initial_state(&state_clone);
    {
        let mut state = state_clone.write().expect("RwLock error");
        assert_eq!(state.results[0].state, ResultItemState::InQueue);
        assert!(state.results[0].attempt_count.is_none());
        state.results[0].attempt_count = Some(1);
        state.results[0].state = ResultItemState::Err;
    }

    // retry attempt index == 0
    cywad_server::process_retry(&state_clone);
    {
        let mut state = state_clone.write().expect("RwLock error");
        assert_eq!(state.results[0].state, ResultItemState::InQueue);
        state.results[0].attempt_count = Some(2);
        state.results[0].state = ResultItemState::Err;
    }

    // retry attempt index == 1
    cywad_server::process_retry(&state_clone);
    {
        let mut state = state_clone.write().expect("RwLock error");
        assert_eq!(state.results[0].state, ResultItemState::InQueue);
        state.results[0].attempt_count = Some(3);
        state.results[0].state = ResultItemState::Err;
    }

    // retry attempt index == 2
    cywad_server::process_retry(&state_clone);
    {
        let mut state = state_clone.write().expect("RwLock error");
        assert_eq!(state.results[0].state, ResultItemState::InQueue);
        state.results[0].attempt_count = Some(4);
        state.results[0].state = ResultItemState::Err;
    }

    // retry limit reached
    cywad_server::process_retry(&state_clone);
    {
        let state = state_clone.read().expect("RwLock error");
        assert_eq!(state.results[0].state, ResultItemState::Err);
    }
}

#[test]
fn test_cron() -> Result<(), failure::Error> {
    let _ = env_logger::try_init();

    let expression = "0   *   *     *       *  *  *";
    let config_toml = r#"
        url = "mock"
        name = "test cron"
        cron = "0   *   *     *       *  *  *"
        retry = [ 10, 15, 60 ]
        window_width = 1280
        window_height = 1024
        step_timeout = 10000
        step_interval = 10

        [[steps]]
        kind = "value"
        exec = """(function () {
            return 1;
        })();
        """
    "#;

    let config: Config = toml::from_str(&config_toml).expect("parse config error");

    debug!("Config: {:#?}", config);

    assert!(config.cron.is_some());

    let state: SharedState = Arc::new(RwLock::new(State {
        configs: Vec::new(),
        results: Vec::new(),
        tx: None,
        tx_vec: None,
    }));

    // initialize
    let state_clone = state;
    {
        let mut state = state_clone.write().expect("RwLock error");
        state.results.push(ResultItem::new(&config.name));
        state.configs.push(config);
    }

    cywad_server::populate_initial_state(&state_clone);
    cywad_server::run_scheduler(&state_clone, true);
    let now = chrono::Local::now();
    let schedule = Schedule::from_str(expression).map_err(|e| format_err!("Cron error {}", e))?;
    {
        let state = state_clone.read().expect("RwLock error");
        debug!(
            "Scheduled {:?} {:?}",
            state.results[0].scheduled,
            schedule.after(&now).take(1).next()
        );
        assert_eq!(
            state.results[0].scheduled,
            schedule.after(&now).take(1).next()
        );
        assert!(state.results[0].scheduled.ok_or_else(|| format_err!("Schedule empty"))? > now);
        Ok(())
    }
}
