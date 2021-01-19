use chrono::{DateTime, Local};
use yew::prelude::*;

use super::get_api_url;

pub enum WidgetFormMsg {
    Width(usize),
    Height(usize),
    FontSize(usize),
    Noop,
}

#[derive(Properties, Clone)]
pub struct WidgetProps {
    pub server_datetime: DateTime<Local>,
}

pub struct WidgetForm {
    props: WidgetProps,
    link: ComponentLink<Self>,
    width: usize,
    height: usize,
    fontsize: usize,
}

impl Component for WidgetForm {
    type Message = WidgetFormMsg;
    type Properties = WidgetProps;

    fn create(props: Self::Properties, link: ComponentLink<Self>) -> Self {
        Self {
            props,
            link,
            width: 500,
            height: 100,
            fontsize: 14,
        }
    }

    fn update(&mut self, msg: Self::Message) -> ShouldRender {
        match msg {
            WidgetFormMsg::Width(v) => self.width = v,
            WidgetFormMsg::Height(v) => self.height = v,
            WidgetFormMsg::FontSize(v) => self.fontsize = v,
            _ => {
                return false;
            }
        };

        true
    }

    fn change(&mut self, props: Self::Properties) -> ShouldRender {
        log::info!(
            "WidgetForm change {:?} != {:?} is {:?}",
            self.props.server_datetime,
            props.server_datetime,
            self.props.server_datetime != props.server_datetime
        );
        if self.props.server_datetime != props.server_datetime {
            self.props = props;
            return true;
        }
        false
    }

    fn view(&self) -> Html {
        let on_width = self.link.callback(|cd| {
            if let ChangeData::Value(s) = cd {
                if let Ok(v) = s.parse::<usize>() {
                    return WidgetFormMsg::Width(v);
                }
            }
            WidgetFormMsg::Noop
        });
        let on_height = self.link.callback(|cd| {
            if let ChangeData::Value(s) = cd {
                if let Ok(v) = s.parse::<usize>() {
                    return WidgetFormMsg::Height(v);
                }
            }
            WidgetFormMsg::Noop
        });
        let on_fontsize = self.link.callback(|cd| {
            if let ChangeData::Value(s) = cd {
                if let Ok(v) = s.parse::<usize>() {
                    return WidgetFormMsg::FontSize(v);
                }
            }
            WidgetFormMsg::Noop
        });

        let widget_url = get_api_url(&format!(
            "/widget/png/{}/{}/{}",
            self.width, self.height, self.fontsize
        ));
        let widget_src = format!("{}?t={}", widget_url, self.props.server_datetime);

        html! {
            <div class="p-4 mx-auto lg:w-1/3 bg-white rounded shadow-lg">
                <div class="p-4 text-black text-xl border-b border-grey-lighter">{ "PNG widget preview" }</div>
                <div class="p-4">
                    <div class="flex flex-auto mb-4 justify-between">
                        <div class="w-1/3">
                            <label class="block text-grey-darker text-sm font-bold" for="width">{ "Width" }</label>
                            <input onchange=on_width class="appearance-none border rounded text-grey-darker" id="width" type="text" placeholder="Width" value=self.width size="5"/>
                        </div>
                        <div class="w-1/3">
                            <label class="block text-grey-darker text-sm font-bold" for="height">{ "Height" }</label>
                            <input onchange=on_height class="appearance-none border rounded text-grey-darker" id="height" type="text" placeholder="Height" value=self.height  size="5"/>
                        </div>
                        <div class="w-1/3">
                            <label class="block text-grey-darker text-sm font-bold whitespace-nowrap" for="font">{ "Font size" }</label>
                            <input onchange=on_fontsize class="appearance-none border rounded text-grey-darker" id="font" type="text" placeholder="Font size" value=self.fontsize  size="5"/>
                        </div>
                    </div>
                    <div class="mb-4">
                        <label class="block text-grey-darker text-sm font-bold mb-2" for="url">{ "URL" }</label>
                        <a class="text-grey-ligther text-xs" id="url" href=widget_url.to_owned() target="_blank">{ &widget_url }</a>
                    </div>
                    <div class="mb-4">
                        <label class="block text-grey-darker text-sm font-bold mb-2" for="password">{ "Preview" }</label>
                        <img class="border-4 border-blue-500 border-opacity-25" src=&widget_src />
                        <p class="text-grey text-xs mt-1">{ "Don't forget add token param to url "}</p>
                    </div>
                </div>
            </div>
        }
    }
}
