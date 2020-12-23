use anyhow::{anyhow, Error};
use cfg_if::cfg_if;
use log::{error, info};

#[cfg(all(feature = "webkit", feature = "devtools"))]
compile_error!("features `crate/webkit` and `crate/devtools` are mutually exclusive");

use std::sync::{Arc, RwLock};

pub mod engine;

use cywad_core::{
    load_config, Config, EngineOptions, EngineTrait, ResultItem, SharedState, State,
    APP_DESCRIPTION, APP_NAME, APP_VERSION,
};

cfg_if! {
    if #[cfg(feature = "server")] {
        use std::fs;
        use std::sync::mpsc;
        use std::sync::mpsc::{Receiver, Sender};
        use std::sync::Mutex;
        use std::thread;

    }
}

use clap::{App, Arg, SubCommand};

fn main() -> Result<(), String> {
    if let Err(e) = run() {
        return Err(format!("{}", e));
    }
    Ok(())
}

fn build_engine_options(matches: &clap::ArgMatches) -> Result<EngineOptions, Error> {
    #[cfg(feature = "webkit")]
    let engine_options = EngineOptions {
        keep_open: matches.is_present("keep_open"),
    };

    #[cfg(feature = "devtools")]
    let engine_options = {
        let mut options = EngineOptions::default();
        if let Some(command) = matches.value_of("command") {
            options.command = Some(command.into());
        }
        if let Some(endpoint) = matches.value_of("endpoint") {
            options.endpoint = endpoint.into();
        }
        if let Some(http_timeout) = matches.value_of("http_timeout") {
            options.http_timeout = http_timeout
                .parse::<u64>()
                .map_err(|e| anyhow!("http-timeout options parse error: {}", e))?;
        }
        if let Some(max_frame_size) = matches.value_of("max_frame_size") {
            options.max_frame_size = max_frame_size
                .parse::<usize>()
                .map_err(|e| anyhow!("max-frame-size options parse error: {}", e))?;
        }
        options
    };

    Ok(engine_options)
}

fn run() -> Result<(), Error> {
    // set default logging level
    match std::env::var("RUST_LOG") {
        Ok(_) => {}
        Err(_) => std::env::set_var("RUST_LOG", "actix_web=info,cywad=info"),
    }
    env_logger::init();

    #[cfg(feature = "webkit")]
    let engine_name = "webkit";

    #[cfg(feature = "devtools")]
    let engine_name = "devtools";

    #[cfg(feature = "webkit")]
    let cli_engine_options = SubCommand::with_name("cli")
        .arg(
            Arg::with_name("config")
                .short("c")
                .long("config")
                .value_name("FILE")
                .help("Config file")
                .required(true)
                .takes_value(true),
        )
        .arg(
            Arg::with_name("keep_open")
                .short("k")
                .long("keep-open")
                .help("Keep open browser")
                .required(false)
                .takes_value(false),
        );

    #[cfg(feature = "devtools")]
    let cli_engine_options = SubCommand::with_name("cli")
        .arg(
            Arg::with_name("config")
                .short("c")
                .long("config")
                .value_name("FILE")
                .help("Config file")
                .required(true)
                .takes_value(true),
        )
        .arg(
            Arg::with_name("command")
                .long("command")
                .help("Browser launch command. If it present then browser will start on each config processing and killed on finish.")
                .env("CYWAD_COMMAND")
                .required(false)
                .takes_value(true),
        )
        .arg(
            Arg::with_name("endpoint")
                .long("endpoint")
                .help("Browser debug port http location")
                .env("CYWAD_ENDPOINT")
                .required(false)
                .takes_value(true),
        )
        .arg(
            Arg::with_name("http_timeout")
                .long("http-timeout")
                .help("Devtools HTTP timeout in seconds, by default 5s")
                .env("CYWAD_HTTP_TIMEOUT")
                .required(false)
                .takes_value(true),
        )
        .arg(
            Arg::with_name("max_frame_size")
                .long("max-frame-size")
                .help("WS max frame size in bytes")
                .env("CYWAD_MAX_FRAME_SIZE")
                .required(false)
                .takes_value(true),
        );

    let mut builder = App::new(format!(
        "{} - {}",
        APP_NAME.unwrap_or("balance-informer"),
        engine_name,
    ))
    .about(APP_DESCRIPTION.unwrap_or("Does awesome things"))
    .version(APP_VERSION.unwrap_or("unknown"))
    .subcommand(cli_engine_options);

    #[cfg(feature = "server")]
    {
        builder = builder.subcommand(
            SubCommand::with_name("serve")
                .arg(
                    Arg::with_name("command")
                        .long("command")
                        .help("Browser launch command. If it present then browser will start on each config processing and killed on finish.")
                        .env("CYWAD_COMMAND")
                        .required(false)
                        .takes_value(true),
                )
                .arg(
                    Arg::with_name("endpoint")
                        .long("endpoint")
                        .help("Browser debug port http location")
                        .env("CYWAD_ENDPOINT")
                        .required(false)
                        .takes_value(true),
                )
                .arg(
                    Arg::with_name("http_timeout")
                        .long("http-timeout")
                        .help("Devtools HTTP timeout in seconds, by default 5s")
                        .env("CYWAD_HTTP_TIMEOUT")
                        .required(false)
                        .takes_value(true),
                )
                .arg(
                    Arg::with_name("max_frame_size")
                        .long("max-frame-size")
                        .help("WS max frame size in bytes")
                        .env("CYWAD_MAX_FRAME_SIZE")
                        .required(false)
                        .takes_value(true),
                )
                .arg(
                    Arg::with_name("path_to_config")
                        .short("p")
                        .long("path-to-config")
                        .value_name("PATH")
                        .help("Path to config files")
                        .env("CYWAD_PATH_TO_CONFIG")
                        .required(true)
                        .takes_value(true),
                )
                .arg(
                    Arg::with_name("path_to_static")
                        .short("s")
                        .long("path-to-static")
                        .value_name("STATIC_PATH")
                        .help("Path to static files to serve")
                        .env("CYWAD_STATIC_PATH")
                        .required(false)
                        .takes_value(true),
                )
                .arg(
                    Arg::with_name("listen")
                        .short("l")
                        .long("listen")
                        .value_name("interface:port")
                        .help("Listen interface and port, by default 127.0.0.1:5000")
                        .env("CYWAD_LISTEN")
                        .required(false)
                        .takes_value(true),
                )
                .arg(
                    Arg::with_name("username")
                        .long("username")
                        .value_name("username")
                        .help("username")
                        .env("CYWAD_USERNAME")
                        .required(false)
                        .takes_value(true),
                )
                .arg(
                    Arg::with_name("password")
                        .long("password")
                        .value_name("password")
                        .help("password")
                        .env("CYWAD_PASSWORD")
                        .required(false)
                        .takes_value(true),
                )
                .arg(
                    Arg::with_name("actix_fs_pool")
                        .long("actix-fs-pool")
                        .value_name("actix_fs_pool")
                        .help("default cpu pool size for static files, default is 1")
                        .env("CYWAD_ACTIX_FS_POOL")
                        .required(false)
                        .takes_value(true),
                ),
        )
    }

    let matches = builder.get_matches();

    if let Some(matches) = matches.subcommand_matches("cli") {
        let config_file = matches.value_of("config").unwrap_or("default.conf");

        let config = match load_config(config_file) {
            Ok(config) => config,
            Err(error) => {
                return Err(anyhow!(
                    "Validation error! File {} - {}",
                    config_file,
                    error
                ));
            }
        };

        let state: SharedState = Arc::new(RwLock::new(State {
            configs: Vec::new(),
            results: Vec::new(),
            tx: None,
            tx_vec: None,
        }));

        // initialize
        let state_clone = Arc::clone(&state);
        {
            let mut state = state_clone
                .write()
                .map_err(|e| anyhow!("RwLock error: {}", e))?;
            state.results.push(ResultItem::new(&config.name));
            state.configs.push(config);
        }

        let mut engine = engine::new();
        engine.execute(0, state_clone, build_engine_options(matches)?)?;

        // convert sreenshot png to data:image/png;base64,...
        {
            let mut state = state.write().map_err(|e| anyhow!("RwLock error: {}", e))?;
            let screenshots = &mut state.results[0].screenshots;
            for item in screenshots {
                (*item).uri = format!("data:image/png;base64,{}", base64::encode(&item.data));
            }
        }

        let state = state.read().map_err(|e| anyhow!("RwLock error: {}", e))?;

        println!(
            "{}",
            serde_json::to_string_pretty(&*state.results)
                .map_err(|e| anyhow!("Serde error: {}", e))?
        );
        return Ok(());
    }

    #[cfg(feature = "server")]
    {
        if let Some(matches) = matches.subcommand_matches("serve") {
            let path = matches
                .value_of("path_to_config")
                .ok_or_else(|| anyhow!("Configuration error: path to config not defined"))?;

            // set default actix settings
            if cfg!(feature = "server") {
                match std::env::var("ACTIX_FS_POOL") {
                    Ok(_) => {}
                    Err(_) => {
                        std::env::set_var(
                            "ACTIX_FS_POOL",
                            matches.value_of("actix_fs_pool").unwrap_or("1"),
                        );
                    }
                };
            }

            let (tx, rx): (Sender<usize>, Receiver<usize>) = mpsc::channel();

            let state: SharedState = Arc::new(RwLock::new(State {
                configs: Vec::new(),
                results: Vec::new(),
                tx: Some(Mutex::new(tx)),
                tx_vec: Some(Vec::new()),
            }));

            let mut configs_by_filename: Vec<(String, Config)> = Vec::new();

            // read and validate configs
            for file in fs::read_dir(path)? {
                let filepath = file?.path().display().to_string();
                if !filepath.contains(".toml") {
                    continue;
                }
                let config = match load_config(filepath.as_str()) {
                    Ok(config) => config,
                    Err(error) => {
                        return Err(anyhow!(
                            "Validation error! File {} - {}",
                            filepath.as_str(),
                            error
                        ));
                    }
                };
                configs_by_filename.push((filepath, config));
            }

            // sort by filename
            configs_by_filename.sort_by_key(|c| c.0.to_owned());

            // initialize state
            let state_clone = Arc::clone(&state);
            {
                let mut state = state_clone
                    .write()
                    .map_err(|e| anyhow!("RwLock error: {}", e))?;

                for (_, config) in configs_by_filename {
                    state.results.push(ResultItem::new(&config.name));
                    state.configs.push(config);
                }
            }

            // start execution thread
            let state_clone = Arc::clone(&state);

            let engine_options = build_engine_options(matches)?;

            thread::spawn(move || {
                let mut engine = engine::new();

                loop {
                    if let Ok(index) = rx.recv() {
                        match engine.execute(
                            index,
                            Arc::clone(&state_clone),
                            engine_options.clone(),
                        ) {
                            Ok(()) => info!("Done"),
                            Err(e) => {
                                error!("Engine error: {}", e);
                                {
                                    match state_clone.write() {
                                        Ok(mut state) => state.mark_as_err(index),
                                        Err(e) => {
                                            error!("RWLock error: {}", e)
                                        }
                                    };
                                }
                            }
                        };
                    }
                }
            });

            // start scheduler thread
            let state_clone = Arc::clone(&state);
            thread::spawn(move || {
                cywad_server::populate_initial_state(&state_clone);
                cywad_server::run_scheduler(&state_clone, false);
            });

            let mut web_config = cywad_server::WebConfig::default();
            web_config.username = matches.value_of("username").map(String::from);
            web_config.password = matches.value_of("password").map(String::from);
            web_config.static_files_path = matches.value_of("path_to_static").map(String::from);

            // serve from main thread
            cywad_server::serve(
                matches.value_of("listen").unwrap_or("127.0.0.1:5000"),
                state,
                web_config,
            );
            return Ok(());
        }
    }

    // nothing matches
    println!("{}", matches.usage());

    Ok(())
}
