#![recursion_limit = "512"]
use anyhow::Error;
use wasm_bindgen::prelude::*;

use yew::format::{Json, Nothing};
use yew::prelude::*;
use yew::services::fetch::{
    Credentials, FetchOptions, FetchService, FetchTask, Mode, Request, Response,
};

use cywad_core::{HeartBeat, ItemResponse, ItemsResponse};

mod bindings;
mod item_card;
mod screenshot_modal;
mod widget_form;
use crate::item_card::ItemCard;
use crate::screenshot_modal::ScreenshotModal;
use crate::widget_form::WidgetForm;

fn get_api_url(method: &str) -> String {
    format!(
        "{}{}",
        option_env!("CYWAD_BASE_API_URL").unwrap_or("http://127.0.0.1:5000"),
        method
    )
}

// #[derive(Debug, Serialize, Deserialize)]
// pub struct ItemsResponse {
//     pub info: AppInfo,
//     pub items: Vec<ResultItem>,
// }
//
// #[derive(Debug, Serialize, Deserialize)]
// pub struct ItemResponse {
//     pub item: ResultItem,
//     pub server_datetime: DateTime<Local>,
// }

pub struct Model {
    link: ComponentLink<Self>,
    data: Option<ItemsResponse>,
    item_screenshots_index: Option<usize>,
    ft: Option<FetchTask>,
    sse_closure: Closure<dyn FnMut(String, String)>,
}

#[derive(Debug)]
pub enum Msg {
    Items(ItemsResponse),
    Item((usize, ItemResponse)),
    FetchResourceFailed,
    UpdateItem(usize),
    ShowScreenshots(Option<usize>),
    SseItem(String),
    SseHeartbeat(String),
}

impl Model {
    fn fetch_data(&mut self) {
        let request = Request::get(get_api_url("/api/items"))
            .header("Access-Control-Request-Method", "GET")
            .body(Nothing)
            .expect("Failed to build request.");

        let callback =
            self.link
                .callback(|response: Response<Json<Result<ItemsResponse, Error>>>| {
                    if let (meta, Json(Ok(body))) = response.into_parts() {
                        if meta.status.is_success() {
                            log::info!("Data: {:?}", serde_json::to_string(&body));
                            return Msg::Items(body);
                        }
                    }
                    Msg::FetchResourceFailed
                });

        self.ft = FetchService::fetch_with_options(
            request,
            FetchOptions {
                mode: Some(Mode::Cors),
                credentials: Some(Credentials::SameOrigin),
                ..FetchOptions::default()
            },
            callback,
        )
        .ok();
    }

    fn update_item(&mut self, index: usize) {
        let item = self.data.as_ref().unwrap().items.get(index).unwrap();
        let request = Request::get(get_api_url(&format!("/api/{}/update", item.slug)))
            .header("Access-Control-Request-Method", "GET")
            .body(Nothing)
            .expect("Failed to build request.");

        let callback = self.link.callback(
            move |response: Response<Json<Result<ItemResponse, Error>>>| {
                if let (meta, Json(Ok(body))) = response.into_parts() {
                    if meta.status.is_success() {
                        return Msg::Item((index, body));
                    }
                }
                Msg::FetchResourceFailed
            },
        );

        self.ft = FetchService::fetch_with_options(
            request,
            FetchOptions {
                mode: Some(Mode::Cors),
                credentials: Some(Credentials::SameOrigin),
                ..FetchOptions::default()
            },
            callback,
        )
        .ok();
    }
}

impl Component for Model {
    type Message = Msg;
    type Properties = ();
    fn create(_: Self::Properties, link: ComponentLink<Self>) -> Self {
        let on_sse_item = link.callback(Msg::SseItem);
        let on_sse_heartbeat = link.callback(Msg::SseHeartbeat);
        let mut instance = Self {
            link,
            data: None,
            item_screenshots_index: None,
            ft: None,
            sse_closure: Closure::wrap(Box::new(move |message_type: String, data: String| {
                match message_type.as_str() {
                    "heartbeat" => on_sse_heartbeat.emit(data),
                    "item" => on_sse_item.emit(data),
                    _ => {}
                };
            }) as Box<dyn FnMut(String, String)>),
        };
        instance.fetch_data();

        bindings::register_sse_handler(get_api_url("/sse"), &instance.sse_closure);

        instance
    }

    fn update(&mut self, msg: Self::Message) -> ShouldRender {
        match msg {
            Msg::Items(data) => {
                self.data = Some(data);
            }
            Msg::Item((index, value)) => {
                if let Some(ref mut data) = self.data {
                    data.items[index] = value.item;
                    data.info.server_datetime = value.server_datetime;
                }
            }
            Msg::UpdateItem(index) => {
                log::info!("Update item: {}", index);
                self.update_item(index);
            }
            Msg::ShowScreenshots(index) => {
                log::info!("Show sreenshots: {:?}", index);
                if self.data.is_some() {
                    self.item_screenshots_index = index;
                }
            }
            Msg::SseItem(message) => {
                log::info!("Sse item: {}", message);
                if let Ok(value) = serde_json::from_str::<ItemResponse>(&message) {
                    if let Some(ref mut data) = self.data {
                        if let Some((index, _)) = data
                            .items
                            .iter()
                            .enumerate()
                            .find(|(_index, item)| item.slug == value.item.slug)
                        {
                            data.items[index] = value.item;
                        }
                        data.info.server_datetime = value.server_datetime;
                    }
                }
            }
            Msg::SseHeartbeat(message) => {
                log::info!("Sse heartbeat: {}", message);
                if let Ok(value) = serde_json::from_str::<HeartBeat>(&message) {
                    if let Some(ref mut data) = self.data {
                        data.info.server_datetime = value.server_datetime;
                    }
                }
            }
            _ => {}
        }
        true
    }

    fn change(&mut self, _props: Self::Properties) -> ShouldRender {
        // Should only return "true" if new properties are different to
        // previously received properties.
        // This component has no properties so we will always return "false".
        false
    }

    fn view(&self) -> Html {
        let items = move || -> Html {
            match &self.data {
                Some(data) => {
                    html! { { data.items.iter().enumerate().map(|(index, item)| html! { <ItemCard index=index item=item parent=self.link.clone() /> }).collect::<Html>() } }
                }
                _ => html! {},
            }
        };

        let server_datetime_html = {
            if let Some(ref data) = self.data {
                html! { <h1 class="text-2xl text-black">{ data.info.server_datetime.format("%Y-%m-%d %H:%M").to_string() }</h1> }
            } else {
                html! {}
            }
        };

        let screenshot_modal_html = {
            match self.item_screenshots_index {
                Some(index) => match &self.data {
                    Some(data) => match data.items.get(index) {
                        Some(item) => {
                            html! { <ScreenshotModal item=item parent=self.link.clone() /> }
                        }
                        _ => html! {},
                    },
                    _ => html! {},
                },
                _ => html! {},
            }
        };

        let widget_form_html = if let Some(data) = &self.data {
            html! { <WidgetForm server_datetime=data.info.server_datetime /> }
        } else {
            html! {}
        };

        // https://codepen.io/codetimeio/pen/RYMEJe
        html! {
            <div class="container my-6 mx-auto px-4 md:px-12">
                { server_datetime_html }
                <div class="flex flex-wrap -mx-1 lg:-mx-4">
                    { items() }
                </div>
                { screenshot_modal_html }
                { widget_form_html }
            </div>
        }
    }
}

fn main() {
    wasm_logger::init(wasm_logger::Config::default());
    yew::start_app::<Model>();
}
