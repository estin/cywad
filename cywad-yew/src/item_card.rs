use super::{Model, Msg};

use chrono::prelude::{Local};

use yew::prelude::*;

use cywad_core::{ResultItem, ValueItem};

pub enum ItemCardMsg {
    ToParent(Msg),
}

#[derive(Properties, Clone)]
pub struct ItemCardProps {
    pub index: usize,
    pub item: ResultItem,
    pub parent: ComponentLink<Model>,
}

pub struct ItemCard {
    props: ItemCardProps,
    link: ComponentLink<Self>,
}

impl Component for ItemCard {
    type Message = ItemCardMsg;
    type Properties = ItemCardProps;

    fn create(props: Self::Properties, link: ComponentLink<Self>) -> Self {
        Self { props, link }
    }

    fn update(&mut self, msg: Self::Message) -> ShouldRender {
        match msg {
            ItemCardMsg::ToParent(inner) => {
                log::info!("send message to parent: {:?}", inner);
                self.props.parent.send_message(inner);
            }
        }
        false
    }

    fn change(&mut self, props: Self::Properties) -> ShouldRender {
        // log::info!(
        //     "ItemCard change {:?} != {:?} is {:?}",
        //     self.props.item.datetime,
        //     props.item.datetime,
        //     self.props.item.datetime != props.item.datetime
        // );

        if self.props.item.datetime != props.item.datetime {
            self.props = props;
            return true;
        }
        false
    }

    fn view(&self) -> Html {
        let item = &self.props.item;
        let index = self.props.index;

        let time_ago_html = {
            let updated_ago = Local::now().signed_duration_since(self.props.item.datetime);

            let hours = updated_ago.num_hours();
            if hours > 0 {
                format!("{} hours", hours)
            } else {
                let minutes = updated_ago.num_minutes();
                if minutes > 0 {
                    format!("{} minutes", minutes)
                } else {
                    format!("{} seconds", updated_ago.num_seconds())
                }
            }
        };

        let status_html = {
            if item.is_in_work() || item.is_err() {
                format!(
                    "({:?} {}/{})",
                    item.state,
                    item.steps_done.unwrap_or(0),
                    item.steps_total.unwrap_or(0)
                )
            } else {
                format!("({:?})", item.state)
            }
        };

        let render_value = |v: &ValueItem| -> Html {
            let (ring_classes, value_classes) = match v.level.as_deref() {
                Some("green") => (vec![], vec!["text-green-700 font-semibold"]),
                Some("yellow") => (
                    vec!["rounded-md ring-4 ring-yellow-500 ring-opacity-50 p-1"],
                    vec!["text-yellow-500 font-semibold"],
                ),
                Some("red") => (
                    vec!["rounded-md ring-4 ring-red-500 ring-opacity-50 p-1"],
                    vec!["text-red-500 font-semibold"],
                ),
                _ => (vec![], vec![]),
            };
            html! {
                <div><span class=ring_classes><span class=value_classes>{ v.value }</span>{ " " } { v.key.to_owned() }</span></div>
            }
        };
        let update_html = {
            let mut classes = vec!["focus:outline-none uppercase p-3 flex items-center bg-yellow-500 text-blue-50 max-w-max shadow-sm hover:shadow-lg rounded-full w-10 h-10"];
            if item.is_in_work() || item.is_in_queue() {
                classes.push("opacity-50 cursor-not-allowed");
            }

            if item.is_in_work() {
                classes.push("animate-spin");
            }

            let svg_icon = {
                if item.is_in_work() {
                    html! {
                        <svg class="transform scale-150 h-10 w-10 text-white" xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24">
                            <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
                            <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path>
                        </svg>
                    }
                } else {
                    html! {
                        <svg width="32" height="32" preserveAspectRatio="xMidYMid meet" viewBox="0 0 32 32" style="transform: rotate(360deg);">
                            <path d="M26 18A10 10 0 1 1 16 8h6.182l-3.584 3.585L20 13l6-6l-6-6l-1.402 1.414L22.185 6H16a12 12 0 1 0 12 12z" fill="currentColor"></path>
                        </svg>
                    }
                }
            };

            let scheduled = {
                if let Some(s) = item.scheduled {
                    html! { <span class="text-gray-500 text-xs m-1">{format!("scheduled {}", s.format("%Y-%m-%d %H:%M"))}</span> }
                } else {
                    html! {}
                }
            };
            html! {
                <div class="flex flex-row items-center justify-between">
                    <button onclick=self.link.callback(move |_| ItemCardMsg::ToParent(Msg::UpdateItem(index))) class=classes>
                        { svg_icon }
                    </button>
                    { scheduled }
                </div>
            }
        };

        let screenhots_html = {
            let mut classes = vec!["bg-blue-500 text-white font-bold py-2 px-4 rounded text-sm"];
            if item.screenshots.is_empty() {
                classes.push("opacity-50 cursor-not-allowed");
            }
            html! {
                <div>
                    <button onclick=self.link.callback(move |_| ItemCardMsg::ToParent(Msg::ShowScreenshots(Some(index)))) class=classes>
                        { format!("screenshots ({})", item.screenshots.len()) }
                    </button>
                </div>
            }
        };

        html! {
            <div class="my-1 px-1 w-full md:w-1/2 lg:my-4 lg:px-4 lg:w-1/3">
                <article class="overflow-hidden rounded-lg shadow-lg sm:shadow-2xl">
                <header class="flex items-center justify-between leading-tight p-2">
                    <h1 class="text-2xl text-black">{ &item.name }</h1>
                    <p class="text-gray-darker text-sm">
                    <span title={ item.datetime.format("%Y-%m-%d %H:%M") }>{ time_ago_html }</span>
                    <span class="text-gray-500 text-xs p-0.5">{ status_html }</span>
                    </p>
                </header>
                <div class="grid grid-cols-3 gap-2 h-20 place-items-auto items-left text-left justify-between leading-tight p-2 whitespace-nowrap text-sm">
                    { item.values.iter().map(render_value).collect::<Html>() }
                </div>
                <footer class="flex flex-row items-center justify-between p-2">
                    { update_html }
                    { screenhots_html }
                </footer>
                </article>
            </div>
        }
    }
}
