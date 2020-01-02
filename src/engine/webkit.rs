use failure::Error;
use std::io::Cursor;
use std::sync::{Arc, Mutex};

use cairo::{Context, Format, ImageSurface};
use gtk;
use gtk::{Builder, ContainerExt, Continue, GtkWindowExt, Inhibit, WidgetExt, Window, WindowType};
use webkit2gtk::{
    CookieManagerExt, SettingsExt, TLSErrorsPolicy, WebContext, WebContextExt, WebView, WebViewExt,
};

use super::super::core::{
    ExecutionContext, PreviouResultItemState, ResultItemState, SharedState, StepKind, StepResult,
};
use super::traits::EngineTrait;
use super::EngineOptions;
use chrono::prelude::Local;

pub struct Webkit;

macro_rules! glade_template {
    ($width:expr, $height:expr) => {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<interface>
  <object class="GtkOffscreenWindow" id="window">
    <property name="title" translatable="no">CYWAD</property>
    <property name="default-width">{}</property>
    <property name="default-height">{}</property>
  </object>
</interface>
"#,
            $width, $height
        )
    };
}

enum ExecutionResult {
    InWork,
    Done,
    Error,
}

fn get_screenshot(webview: &WebView) -> Result<Vec<u8>, Error> {
    let vec = Vec::new();
    let mut c = Cursor::new(vec);
    let allocation = webview.get_allocation();
    let surface = ImageSurface::create(Format::ARgb32, allocation.width, allocation.height)
        .map_err(|_| format_err!("Unable create surface"))?;
    let context = Context::new(&surface);
    webview.draw(&context);
    surface
        .write_to_png(&mut c)
        .map_err(|_| format_err!("Unable write png"))?;
    Ok(c.into_inner())
}

fn execute_step(
    execution_context: &Arc<Mutex<ExecutionContext>>,
    webview: &WebView,
) -> Result<ExecutionResult, Error> {
    let step = {
        let guard = execution_context.try_lock();
        if guard.is_err() {
            warn!("Execution context locked by previous iteration...skip...");
            return Ok(ExecutionResult::InWork);
        }
        let mut context = guard.map_err(|e| format_err!("Mutex error: {}", e))?;

        // check timeout
        let elapsed = (Local::now().timestamp() - context.ts_start) * 1000;
        debug!(
            "[{}] Step #{} elapsed {}ms of {}ms",
            context, context.step_index, elapsed, context.config.step_timeout,
        );
        if elapsed > context.config.step_timeout {
            error!("[{}] Step #{} timeout", context, context.step_index);
            context.timeout()?;
            return Ok(ExecutionResult::Error);
        }

        match context.step_result {
            StepResult::Idle => {
                info!("[{}] Start", context);
            }
            StepResult::InWork => {
                debug!(
                    "[{}] Step #{} not done yet...skip... iteration",
                    context, context.step_index
                );
                return Ok(ExecutionResult::InWork);
            }
            StepResult::Next => {
                info!("[{}] Step #{} OK", context, context.step_index);
                context.start_new_step()?;
                debug!("[{}] Step index increased #{}", context, context.step_index);
            }
            StepResult::Repeat => {
                debug!("[{}] Step #{} repeat", context, context.step_index);
            }
            StepResult::Error => {
                warn!("[{}] Step #{} failed", context, context.step_index);
                context.error("Execution error".to_owned())?;
                return Ok(ExecutionResult::Error);
            }
        };

        // job done
        if context.is_done() {
            info!("[{}] Done", context);
            context.done()?;
            return Ok(ExecutionResult::Done);
        }

        // mark step in work
        context.step_result = StepResult::InWork;

        context.get_step()?
    };

    match step.kind {
        StepKind::Screenshot => {
            let mut context = execution_context
                .lock()
                .map_err(|e| format_err!("Mutex error: {}", e))?;

            context.add_screenshot_and_start_new_step(get_screenshot(&webview)?)?;
            context.step_result = StepResult::Idle;
        }
        StepKind::Wait | StepKind::Value | StepKind::Exec => {
            let kind = step.kind.to_owned();
            let exec = if let Some(ref exec) = step.exec {
                exec
            } else {
                unreachable!("'exec' not defined for step kind");
            };

            let execution_context_clone = Arc::clone(&execution_context);
            // let webview_clone = webview.clone();

            // webview.run_javascript_with_callback(exec, move |result| {
            webview.run_javascript(exec, None, move |result| {
                let mut context = match execution_context_clone.lock() {
                    Ok(context) => context,
                    Err(e) => {
                        error!("Mutex error: {}", e);
                        return;
                    }
                };

                match result {
                    Ok(result) => {
                        let js_context = match result.get_global_context() {
                            Some(c) => c,
                            None => {
                                error!("JS context empty");
                                context.step_result = StepResult::Error;
                                return;
                            }
                        };

                        let value = match result.get_value() {
                            Some(v) => v,
                            None => {
                                error!("Value empty");
                                context.step_result = StepResult::Error;
                                return;
                            }
                        };

                        match kind {
                            StepKind::Exec => {
                                debug!("[{}] Step #{} exec done", context, context.step_index);
                                context.step_result = StepResult::Next;
                            }

                            StepKind::Value => {
                                let result = value.to_number(&js_context);
                                info!(
                                    "[{}] Step #{} result number: {:?}",
                                    context, context.step_index, result
                                );

                                match result {
                                    Some(value) => {
                                        match context.add_value_and_start_new_step(value) {
                                            Ok(_) => context.step_result = StepResult::Idle,
                                            Err(_) => context.step_result = StepResult::Error,
                                        };
                                    }
                                    None => {
                                        error!("Value empty");
                                        context.step_result = StepResult::Error;
                                        return;
                                    }
                                };
                            }
                            StepKind::Wait => {
                                let result = value.to_boolean(&js_context);
                                debug!(
                                    "[{}] Step #{} result {:?}",
                                    context, context.step_index, result
                                );
                                context.step_result = if result {
                                    StepResult::Next
                                } else {
                                    StepResult::Repeat
                                }
                            }
                            _ => unreachable!("not allowed step kind"),
                        }
                    }
                    Err(error) => {
                        error!("[{}] Some error: {}", context, error);
                        if kind != StepKind::Wait {
                            let _ = context.error(format!("Error {}", error));
                            context.step_result = StepResult::Error;
                        }
                    }
                };
            });
        }
    };

    Ok(ExecutionResult::InWork)
}

impl EngineTrait for Webkit {
    fn execute(
        &mut self,
        config_index: usize,
        state: SharedState,
        engine_options: EngineOptions,
    ) -> Result<(), Error> {
        gtk::init()?;

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

        // let timeout = config.timeout;
        let step_interval = config.step_interval;

        let context =
            WebContext::get_default().ok_or_else(|| format_err!("Error on get web context",))?;

        context.set_tls_errors_policy(TLSErrorsPolicy::Ignore);

        let window: Arc<Window> = if engine_options.keep_open {
            // show window on keep_open
            Arc::new(Window::new(WindowType::Toplevel))
        } else {
            // ugly hack to create offscreen window - like headless mode
            let builder = Builder::new_from_string(&glade_template!(
                config.window_width,
                config.window_height
            ));
            Arc::new(builder.get_object("window").expect("Couldn't get window"))
        };

        let webview = WebView::new_with_context(&context);
        webview.load_uri(&config.url);
        window.add(&webview);

        let window = Arc::clone(&window);
        let cookie_manager = context
            .get_cookie_manager()
            .ok_or_else(|| format_err!("Fail to get cookie manager"))?;

        let settings = WebViewExt::get_settings(&webview)
            .ok_or_else(|| format_err!("Fail to get settings"))?;
        settings.set_enable_developer_extras(true);

        window.resize(config.window_width as i32, config.window_height as i32);
        window.show_all();

        let execution_context: Arc<Mutex<ExecutionContext>> = Arc::new(Mutex::new(
            ExecutionContext::new(config, config_index, state),
        ));

        let window_clone = Arc::clone(&window);

        gtk::timeout_add(step_interval, move || {
            match execute_step(&Arc::clone(&execution_context), &webview.clone()) {
                Ok(r) => match r {
                    ExecutionResult::Done | ExecutionResult::Error => {
                        if !engine_options.keep_open {
                            cookie_manager.delete_all_cookies();
                            window_clone.close();
                        }
                        Continue(false)
                    }
                    ExecutionResult::InWork => Continue(true),
                },
                Err(e) => {
                    error!("Engine error: {}", e);
                    Continue(false)
                }
            }
        });

        window.connect_delete_event(|_, _| {
            gtk::main_quit();
            Inhibit(false)
        });

        gtk::main();

        Ok(())
    }
}
