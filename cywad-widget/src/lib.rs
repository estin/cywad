#[macro_use]
extern crate lazy_static;

use failure::Error;

use chrono::prelude::{DateTime, Local};

use image::{png, ColorType, Rgba, RgbaImage};
use imageproc::drawing::draw_text_mut;
use rusttype::{point, Font, FontCollection, Scale};

use cywad_core::{AppInfo, ResultItemState, SharedState};

use failure::format_err;

macro_rules! load_font {
    ($path:tt) => {{
        FontCollection::from_bytes(Vec::from(include_bytes!($path) as &[u8]))
            .expect("load font error")
            .into_font()
            .expect("init font error")
    }};
}

macro_rules! parse_color {
    ($color:tt) => {{
        let color = hex::decode($color)
            .map_err(|e| format_err!("hex decode error: {}", e))
            .unwrap();
        if color.len() != 4 {
            panic!("Invalid color");
        }
        let mut b: [u8; 4] = [0; 4];
        b.copy_from_slice(&color);
        Rgba(b)
    }};
}

lazy_static! {
    static ref FONT_BOLD: Font<'static> = load_font!("../misc/fonts/Lato-Regular.ttf");
    static ref FONT_ITALIC: Font<'static> = load_font!("../misc/fonts/Lato-Italic.ttf");
    static ref FONT_LIGHT: Font<'static> = load_font!("../misc/fonts/Lato-Light.ttf");
    static ref BLACK: Rgba<u8> = parse_color!("000000ff");
    static ref GREEN: Rgba<u8> = parse_color!("008000ff");
    static ref YELLOW: Rgba<u8> = parse_color!("ffff00ff");
    static ref RED: Rgba<u8> = parse_color!("ff0000ff");
}

// TODO param background
// TODO param color map
pub struct PNGWidget {
    pub width: u32,
    pub height: u32,
    pub fontsize: f32,
    pub background: Option<[u8; 4]>,
}

fn repr_since(now: &DateTime<Local>, dtime: &DateTime<Local>) -> String {
    let duration = now.signed_duration_since(*dtime);

    if duration.num_hours() > 0 {
        return format!("{}h", duration.num_hours());
    }

    if duration.num_minutes() > 0 {
        return format!("{}m", duration.num_minutes());
    }

    format!("{}s", duration.num_seconds())
}

impl PNGWidget {
    fn get_text_size(&self, text: &str, scale: Scale, font: &Font) -> Result<(u32, u32), Error> {
        let mut width: u32 = 0;
        let mut height: u32 = 0;

        let v_metrics = font.v_metrics(scale);
        let offset = point(0.0, v_metrics.ascent);
        let g = font
            .layout(text, scale, offset)
            .last()
            .ok_or_else(|| format_err!("layout empty"))?;
        if let Some(bb) = g.pixel_bounding_box() {
            width = bb.max.x as u32;
            height = bb.max.y as u32;
        }

        Ok((width, height))
    }

    pub fn render(&self, state: &SharedState) -> Result<Vec<u8>, Error> {
        let info = AppInfo::default();
        let info_text = format!(
            "{} - v{}",
            info.server_datetime.format("%H:%M %d.%m").to_string(),
            info.version,
        );
        let info_fontsize = if self.fontsize > 6.0 {
            self.fontsize - 4.0
        } else {
            self.fontsize
        };

        let state = state
            .read()
            .map_err(|e| format_err!("RWLock error: {}", e))?;
        let mut image = RgbaImage::new(self.width, self.height);

        // fill background
        if let Some(b) = self.background {
            let color = Rgba { data: b };
            for p in image.pixels_mut() {
                *p = color;
            }
        }

        let scale = Scale {
            x: self.fontsize * 2.0,
            y: self.fontsize,
        };
        let info_scale = Scale {
            x: info_fontsize * 2.0,
            y: info_fontsize,
        };

        let bold_space = self.get_text_size("-", scale, &*FONT_BOLD)?;
        // let italic_space = self.get_text_size("-", scale, &FONT_ITALIC);
        let light_space = self.get_text_size("-", scale, &FONT_LIGHT)?;

        // info
        draw_text_mut(
            &mut image,
            *BLACK,
            0,
            0,
            info_scale,
            &FONT_LIGHT,
            &info_text,
        );

        let mut start_y =
            self.get_text_size(&info_text, info_scale, &FONT_LIGHT)?.1 + light_space.1 / 3;

        for item in &state.results {
            // title
            let name = format!(
                "{} {}:",
                repr_since(&info.server_datetime, &item.datetime),
                item.name
            );
            let name_size = self.get_text_size(&name, scale, &FONT_LIGHT)?;

            draw_text_mut(&mut image, *BLACK, 0, start_y, scale, &FONT_LIGHT, &name);

            let mut start_x = name_size.0 + light_space.0;

            match item.state {
                ResultItemState::Idle | ResultItemState::InQueue => {
                    draw_text_mut(
                        &mut image,
                        *BLACK,
                        start_x,
                        start_y,
                        scale,
                        &FONT_ITALIC,
                        "in queue",
                    );
                }
                ResultItemState::InWork => {
                    draw_text_mut(
                        &mut image,
                        *BLACK,
                        start_x,
                        start_y,
                        scale,
                        &FONT_ITALIC,
                        &format!(
                            "in work - {} / {}",
                            item.steps_done.map_or("-".to_string(), |v| v.to_string()),
                            item.steps_total.map_or("-".to_string(), |v| v.to_string()),
                        ),
                    );
                }
                ResultItemState::Done => {
                    let values_len = item.values.len();

                    // values
                    for (i, value) in item.values.iter().enumerate() {
                        let text = format!("{}", value.value);
                        let value_size = self.get_text_size(&text, scale, &FONT_BOLD)?;

                        let color = match value.level {
                            Some(ref level) => match level.as_str() {
                                "green" => *GREEN,
                                "yellow" => *YELLOW,
                                "red" => *RED,
                                _ => *BLACK,
                            },
                            _ => *BLACK,
                        };

                        draw_text_mut(
                            &mut image, color, start_x, start_y, scale, &FONT_BOLD, &text,
                        );

                        start_x = start_x + value_size.0 + bold_space.0;

                        if i + 1 < values_len {
                            let sep = "/";
                            let sep_size = self.get_text_size(&sep, scale, &FONT_BOLD)?;

                            draw_text_mut(
                                &mut image,
                                *BLACK,
                                start_x,
                                start_y,
                                scale,
                                &FONT_LIGHT,
                                &sep,
                            );
                            start_x = start_x + sep_size.0 + light_space.0;
                        }
                    }
                }
                ResultItemState::Err => {
                    draw_text_mut(&mut image, *RED, start_x, start_y, scale, &FONT_BOLD, "err");
                }
            };

            start_y = start_y + name_size.1 + bold_space.1 / 3;
        }

        let mut png: Vec<u8> = Vec::new();

        png::PNGEncoder::new(&mut png).encode(
            &image,
            self.width,
            self.height,
            ColorType::RGBA(8),
        )?;

        Ok(png)
    }
}
