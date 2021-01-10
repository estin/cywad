use super::{get_api_url, Model, Msg};


use cywad_core::{ResultItem};

use yew::prelude::*;

pub enum ScreenshotModalMsg {
    ToParent(Msg),
    Previous,
    Next,
    Noop,
}

#[derive(Properties, Clone)]
pub struct ScreenshotModalProps {
    pub item: ResultItem,
    pub parent: ComponentLink<Model>,
}

pub struct ScreenshotModal {
    props: ScreenshotModalProps,
    link: ComponentLink<Self>,
    index: usize,
}

impl Component for ScreenshotModal {
    type Message = ScreenshotModalMsg;
    type Properties = ScreenshotModalProps;

    fn create(props: Self::Properties, link: ComponentLink<Self>) -> Self {
        Self {
            props,
            link,
            index: 0,
        }
    }

    fn update(&mut self, msg: Self::Message) -> ShouldRender {
        match msg {
            ScreenshotModalMsg::ToParent(inner) => {
                log::info!("send message to parent: {:?}", inner);
                self.props.parent.send_message(inner);
                false
            }
            ScreenshotModalMsg::Previous => {
                self.index -= 1;
                true
            }
            ScreenshotModalMsg::Next => {
                self.index += 1;
                true
            }
            ScreenshotModalMsg::Noop => false,
        }
    }

    fn change(&mut self, _props: Self::Properties) -> ShouldRender {
        false
    }

    fn view(&self) -> Html {
        let close = self
            .link
            .callback(|_| ScreenshotModalMsg::ToParent(Msg::ShowScreenshots(None)));

        let screenshot_len = self.props.item.screenshots.len();

        let disabled_classes = "opacity-50 cursor-not-allowed";

        let mut prev_classes = vec!["relative inline-flex items-center px-2 py-2 rounded-l-md border border-gray-300 bg-white text-sm font-medium text-gray-500 hover:bg-gray-50"];
        let prev = {
            if self.index > 0 {
                self.link.callback(|_| ScreenshotModalMsg::Previous)
            } else {
                prev_classes.push(disabled_classes);
                self.link.callback(|_| ScreenshotModalMsg::Noop)
            }
        };

        let mut next_classes = vec!["relative inline-flex items-center px-2 py-2 rounded-l-md border border-gray-300 bg-white text-sm font-medium text-gray-500 hover:bg-gray-50"];
        let next = {
            if self.index < screenshot_len - 1 {
                self.link.callback(|_| ScreenshotModalMsg::Next)
            } else {
                next_classes.push(disabled_classes);
                self.link.callback(|_| ScreenshotModalMsg::Noop)
            }
        };

        match self.props.item.screenshots.get(self.index) {
            Some(item) => {
                let title = format!("{} - {}", self.props.item.name, item.name);
                html! {
                    <div class="w-full h-full absolute z-10 inset-0 bg-white">
                        <div class="my-12 mx-auto px-4 md:px-12 flex flex-col items-center justify-center divide-y divide-gray-500 divide-opacity-50">
                            <div class="flex flex-row justify-between p-3 w-9/12">
                                <div>
                                    <h5 class="text-xl text-black"><span class="font-bold">{ &self.props.item.name }</span> { " - " } { &item.name }</h5>
                                </div>
                                <div>
                                    <button onclick=close.clone() class="bg-gray-500 text-white font-bold py-2 px-4 rounded text-sm">{" × "}</button>
                                </div>
                            </div>
                            <div class="p-6">
                                <img src={get_api_url(&format!("/screenshot/{}", item.uri))} alt={title} />
                            </div>
                            <div class="flex flex-row justify-center w-9/12">
                                <div class="p-3 flex flex-grow justify-center">
                                    <nav class="relative z-0 inline-flex shadow-sm -space-x-px">
                                    <a onclick=prev class=prev_classes>
                                        <svg class="h-5 w-5" x-description="Heroicon name: chevron-left" xmlns="http://www.w3.org/2000/svg" viewBox="0 0 20 20" fill="currentColor" aria-hidden="true">
                                            <path fill-rule="evenodd" d="M12.707 5.293a1 1 0 010 1.414L9.414 10l3.293 3.293a1 1 0 01-1.414 1.414l-4-4a1 1 0 010-1.414l4-4a1 1 0 011.414 0z" clip-rule="evenodd"></path>
                                        </svg>
                                    </a>
                                    <a onclick=next class=next_classes>
                                        <svg class="h-5 w-5" x-description="Heroicon name: chevron-right" xmlns="http://www.w3.org/2000/svg" viewBox="0 0 20 20" fill="currentColor" aria-hidden="true">
                                            <path fill-rule="evenodd" d="M7.293 14.707a1 1 0 010-1.414L10.586 10 7.293 6.707a1 1 0 011.414-1.414l4 4a1 1 0 010 1.414l-4 4a1 1 0 01-1.414 0z" clip-rule="evenodd"></path>
                                        </svg>
                                    </a>
                                    </nav>
                                </div>
                                <div class="pt-3">
                                    <button onclick=close class="bg-gray-500 text-white font-bold py-2 px-4 rounded text-sm">{ "Close" }</button>
                                </div>
                            </div>
                        </div>
                    </div>
                }
            }
            None => html! {},
        }
    }
}
