use yew::prelude::*;

use super::get_api_url;

pub enum WidgetFormMsg {
    Width(usize),
    Height(usize),
    FontSize(usize),
    Noop,
}

pub struct WidgetForm {
    link: ComponentLink<Self>,
    width: usize,
    height: usize,
    fontsize: usize,
}

impl Component for WidgetForm {
    type Message = WidgetFormMsg;
    type Properties = ();

    fn create(_props: Self::Properties, link: ComponentLink<Self>) -> Self {
        Self {
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

    fn change(&mut self, _props: Self::Properties) -> ShouldRender {
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

        let url = get_api_url(&format!(
            "/widget/png/{}/{}/{}",
            self.width, self.height, self.fontsize
        ));

        html! {
            <div class="container mx-auto mt-1">
                <div class="w-1/2 mx-auto bg-white rounded shadow-lg">
                    <div class="p-4 text-black text-xl border-b border-grey-lighter">{ "PNG widget preview" }</div>
                    <div class="p-4">
                        <div class="flex mb-4 justify-between">
                            <div class="w-1/3">
                                <label class="block text-grey-darker text-sm font-bold m-2" for="width">{ "Width" }</label>
                                <input onchange=on_width class="appearance-none border rounded m-2 text-grey-darker" id="width" type="text" placeholder="Width" value=self.width />
                            </div>
                            <div class="w-1/3">
                                <label class="block text-grey-darker text-sm font-bold m-2" for="height">{ "Height" }</label>
                                <input onchange=on_height class="appearance-none border rounded m-2 text-grey-darker" id="height" type="text" placeholder="Height" value=self.height />
                            </div>
                            <div class="w-1/3">
                                <label class="block text-grey-darker text-sm font-bold m-2" for="font">{ "Font size" }</label>
                                <input onchange=on_fontsize class="appearance-none border rounded m-2 text-grey-darker" id="font" type="text" placeholder="Font size" value=self.fontsize />
                            </div>
                        </div>
                        <div class="mb-4">
                            <label class="block text-grey-darker text-sm font-bold mb-2" for="url">{ "URL" }</label>
                            <a class="text-grey-ligther text-sm" id="url" href=url.to_owned() target="_blank">{ &url }</a>
                        </div>
                        <div class="mb-4">
                            <label class="block text-grey-darker text-sm font-bold mb-2" for="password">{ "Preview" }</label>
                            <img class="border-4 border-blue-500 border-opacity-25" src=&url />
                            <p class="text-grey text-xs mt-1">{ "Don't forget add token param to url "}</p>
                        </div>
                    </div>
                </div>
            </div>
        }
    }
}
