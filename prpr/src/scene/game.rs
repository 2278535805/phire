#![allow(unused)]

crate::tl_file!("game");

use chinese_number::{ChineseCase, ChineseCountMethod, ChineseVariant, NumberToChinese, ChineseToNumber};
use regex::Regex;
use super::{
    draw_background,
    ending::RecordUpdateState,
    loading::{BasicPlayer, UpdateFn, UploadFn},
    request_input, return_input, show_message, take_input, EndingScene, NextScene, Scene,
};
use crate::{
    bin::{BinaryReader, BinaryWriter},
    config::{Config, Mods},
    core::{copy_fbo, BadNote, Chart, ChartExtra, Effect, Point, Resource, UIElement, Vector},
    ext::{ease_in_out_quartic, parse_time, screen_aspect, semi_white, RectExt, SafeTexture},
    fs::FileSystem,
    info::{ChartFormat, ChartInfo},
    judge::Judge,
    parse::{parse_extra, parse_pec, parse_phigros, parse_rpe},
    task::Task,
    time::TimeManager,
    ui::{RectButton, Ui},
};
use anyhow::{bail, Context, Result};
use concat_string::concat_string;
use lyon::path::Path;
use macroquad::{prelude::*, window::InternalGlContext};
use sasa::{Music, MusicParams};
use serde::{Deserialize, Serialize};
use std::{
    any::Any,
    cell::RefCell,
    fs::File,
    io::{Cursor, ErrorKind},
    ops::{DerefMut, Range},
    path::PathBuf,
    process::{Command, Stdio},
    rc::Rc,
    sync::{Arc, Mutex},
    time::Duration,
};
use tracing::{debug, warn};
use lazy_static::lazy_static;

const PAUSE_CLICK_INTERVAL: f32 = 0.7;

lazy_static! {
    static ref RE_FILTER: Regex = Regex::new(r##"[^a-zA-Z0-9!#$%&'()*+,\-.\/:;<=>?@\\\[\]^_`{|}~ΜΟΒСՕⅭОмвＣＯＭＢ]"##).unwrap();
    static ref RE_VALIDATE: Regex = Regex::new(r"^[CСⅭＣ][OՕΟ0ОＯ][MΜмＭ][BΒ8вＢ][OՕΟ0ОＯ]$").unwrap();
}

#[cfg(feature = "closed")]
mod inner;
#[cfg(feature = "closed")]
use inner::*;

const WAIT_TIME: f32 = 0.5;
const AFTER_TIME: f32 = 0.7;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SimpleRecord {
    pub score: i32,
    pub accuracy: f32,
    pub full_combo: bool,
}

impl SimpleRecord {
    pub fn update(&mut self, other: &SimpleRecord) -> bool {
        let mut changed = false;
        if other.score > self.score {
            self.score = other.score;
            changed = true;
        }
        if other.accuracy > self.accuracy {
            self.accuracy = other.accuracy;
            changed = true;
        }
        if other.full_combo & !self.full_combo {
            self.full_combo = other.full_combo;
            changed = true;
        }
        changed
    }
}

fn fmt_time(t: f32) -> String {
    let f = t < 0.;
    let t = t.abs();
    let secs = t % 60.;
    let mut t = (t / 60.) as u64;
    let mins = t % 60;
    t /= 60;
    let hrs = t % 100;
    format!("{}{hrs:02}:{mins:02}:{secs:05.2}", if f { "-" } else { "" })
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
extern "C" {
    fn on_game_start();
}

#[derive(PartialEq, Eq)]
pub enum GameMode {
    Normal,
    TweakOffset,
    Exercise,
    NoRetry,
    View,
}

#[derive(Clone)]
enum State {
    Starting,
    BeforeMusic,
    Playing,
    Ending,
}

pub struct PauseRewind {
    time: Option<f64>,
    duration: Option<f64>,
    dim: bool,
}

pub struct GameScene {
    should_exit: bool,
    next_scene: Option<NextScene>,

    pub mode: GameMode,
    pub res: Resource,
    pub chart: Chart,
    pub judge: Judge,
    pub gl: InternalGlContext<'static>,
    player: Option<BasicPlayer>,
    chart_bytes: Vec<u8>,
    info_offset: f32,
    effects: Vec<Effect>,

    first_in: bool,
    exercise_range: Range<f32>,
    exercise_press: Option<(i8, u64)>,
    exercise_btns: (RectButton, RectButton),

    pub music: Music,

    state: State,
    pub last_update_time: f64,
    pause_rewind: PauseRewind,
    pause_first_time: f32,

    pub bad_notes: Vec<BadNote>,

    upload_fn: Option<UploadFn>,
    update_fn: Option<UpdateFn>,

    pub touch_points: Vec<(f32, f32)>,
}

macro_rules! reset {
    ($self:ident, $res:expr, $tm:ident) => {{
        $self.bad_notes.clear();
        $self.judge.reset();
        $self.chart.reset();
        $res.judge_line_color = Color::from_hex($res.res_pack.info.color_perfect_line);
        $self.music.pause()?;
        $self.music.seek_to(0.)?;
        $tm.speed = $res.config.speed as _;
        $tm.reset();
        $self.last_update_time = $tm.now();
        $self.state = State::Starting;
        $self.pause_rewind = PauseRewind {
            time: None,
            duration: None,
            dim: false
        };
    }};
}

macro_rules! reset_speed {
    ($self:ident, $res:expr, $tm:ident) => {{
        $self.bad_notes.clear();
        $self.judge.reset();
        $self.chart.reset();
        $res.judge_line_color = Color::from_hex($res.res_pack.info.color_perfect_line);
        $self.music.pause();
        $self.music.seek_to(0.);
        $tm.speed = $res.config.speed as _;
        $tm.reset();
        $self.last_update_time = $tm.now();
        $self.state = State::Starting;
    }};
}

impl GameScene {
    pub const BEFORE_TIME: f32 = 0.7;
    pub const BEFORE_DURATION: f32 = 1.2;
    pub const WAIT_AFTER_TIME: f32 = AFTER_TIME + 0.3;
    pub const FADEOUT_TIME: f32 = WAIT_TIME + AFTER_TIME + 0.3;

    pub async fn load_chart_bytes(fs: &mut dyn FileSystem, info: &ChartInfo) -> Result<Vec<u8>> {
        if let Ok(bytes) = fs.load_file(&info.chart).await {
            return Ok(bytes);
        }
        if let Some(name) = info.chart.strip_suffix(".pec") {
            if let Ok(bytes) = fs.load_file(&concat_string!(name, ".json")).await {
                return Ok(bytes);
            }
        }
        bail!("Cannot find chart file")
    }

    pub fn int_to_roman(mut num: u32) -> String {
        if num.to_string() == "0" {
            return "-".to_string()
        };
        let mut roman: String = String::new();
        let roman_numerals = [
            (1000000, "M￣"),
            (900000, "CM￣"),
            (500000, "D￣"),
            (400000, "CD￣"),
            (100000, "C￣"),
            (90000, "XC￣"),
            (50000, "L￣"),
            (40000, "XL￣"),
            (10000, "X￣"),
            (1000, "M"),
            (900, "CM"),
            (500, "D"),
            (400, "CD"),
            (100, "C"),
            (90, "XC"),
            (50, "L"),
            (40, "XL"),
            (10, "X"),
            (9, "IX"),
            (5, "V"),
            (4, "IV"),
            (1, "I"),
        ];
    
        for &(value, symbol) in roman_numerals.iter() {
            while num >= value {
                roman.push_str(symbol);
                num -= value;
            }
        }
        roman
        
    }

    pub fn int_to_chinese(num: u32) -> String {
        num.to_chinese(ChineseVariant::Simple, ChineseCase::Lower, ChineseCountMethod::TenThousand).unwrap()
    }

    pub fn float_to_chinese(num: f32) -> String {
        let chinese_digits = ["零", "一", "二", "三", "四", "五", "六", "七", "八", "九"];
        let chinese_units = ["", "十", "百", "千", "万", "十万", "百万", "千万", "亿"];
    
        let integer_part = num.trunc() as u64;
        let decimal_part = (num.fract() * 100.0).round() / 100.0;
    
        let mut result = String::new();
    
        // 整数
        if integer_part == 0 {
            result.push_str(chinese_digits[0]);
        } else {
            let mut n = integer_part;
            let mut unit_index = 0;
            let mut need_zero = false;
    
            while n > 0 {
                let digit = (n % 10) as usize;
                if digit != 0 {
                    if need_zero {
                        result.insert(0, '零');
                        need_zero = false;
                    }
                    result.insert_str(0, chinese_units[unit_index]);
                    result.insert_str(0, chinese_digits[digit]);
                } else {
                    if !result.starts_with("零") {
                        need_zero = true;
                    }
                }
                n /= 10;
                unit_index += 1;
            }
    
            if result.starts_with("一十") {
                result.remove(0);
            }
            if result.ends_with("零") {
                result.pop();
            }
        }
    
        // 小数
        if decimal_part > 0.0 {
            result.push('点');
            let decimal_str = decimal_part.to_string();
            for c in decimal_str.chars().skip(2) { // 跳过"0."
                let digit = c.to_digit(10).unwrap() as usize;
                result.push_str(chinese_digits[digit]);
            }
        }
    
        result
    }
    

    pub async fn load_chart(fs: &mut dyn FileSystem, info: &ChartInfo) -> Result<(Chart, Vec<u8>, ChartFormat)> {
        let extra = if let Some(extra) = fs.load_file("extra.json").await.ok().map(String::from_utf8).transpose()? {
            parse_extra(&extra, fs).await.context("Failed to parse extra")?
        } else if let Some(extra) = fs.load_file("extra1.json").await.ok().map(String::from_utf8).transpose()? {
            parse_extra(&extra, fs).await.context("Failed to parse extra1")?
        } else {
            ChartExtra::default()
        };
        let bytes = Self::load_chart_bytes(fs, info).await.context("Failed to load chart")?;
        let format = info.format.clone().unwrap_or_else(|| {
            if let Ok(text) = std::str::from_utf8(&bytes) {
                if text.starts_with('{') {
                    if text.contains("\"META\"") {
                        ChartFormat::Rpe
                    } else {
                        ChartFormat::Pgr
                    }
                } else {
                    ChartFormat::Pec
                }
            } else {
                ChartFormat::Pbc
            }
        });
        let mut chart = match format {
            ChartFormat::Rpe => parse_rpe(&String::from_utf8_lossy(&bytes), fs, extra).await,
            ChartFormat::Pgr => parse_phigros(&String::from_utf8_lossy(&bytes), extra),
            ChartFormat::Pec => parse_pec(&String::from_utf8_lossy(&bytes), extra),
            ChartFormat::Pbc => {
                let mut r = BinaryReader::new(Cursor::new(&bytes));
                r.read()
            }
        }?;
        chart.load_textures(fs).await?;
        chart.settings.hold_partial_cover = info.hold_partial_cover;
        Ok((chart, bytes, format))
    }

    pub async fn new(
        mode: GameMode,
        info: ChartInfo,
        mut config: Config,
        mut fs: Box<dyn FileSystem>,
        player: Option<BasicPlayer>,
        background: SafeTexture,
        illustration: SafeTexture,
        upload_fn: Option<UploadFn>,
        update_fn: Option<UpdateFn>,
    ) -> Result<Self> {
        match mode {
            GameMode::TweakOffset => {
                config.mods.insert(Mods::AUTOPLAY);
            }
            GameMode::Exercise => {
                config.mods.remove(Mods::AUTOPLAY);
            }
            _ => {}
        }
        let (mut chart, chart_bytes, chart_format) = Self::load_chart(fs.deref_mut(), &info).await?;
        let effects = std::mem::take(&mut chart.extra.global_effects);
        if config.fxaa {
            chart
                .extra
                .effects
                .push(Effect::new(0.0..f32::INFINITY, include_str!("fxaa.glsl"), Vec::new(), false).unwrap());
        }

        let info_offset = info.offset;
        let mut res = Resource::new(
            config,
            chart_format,
            info,
            fs,
            player.as_ref().and_then(|it| it.avatar.clone()),
            background,
            illustration,
            chart.extra.effects.is_empty() && effects.is_empty(),
        )
        .await
        .context("Failed to load resources")?;
        let exercise_range = (chart.offset + info_offset + res.config.offset)..res.track_length;
        
        // Prepare extra sfx from chart.hitsounds
        chart.hitsounds.drain().for_each(|(name, clip)| {
            if let Ok(clip) = res.create_sfx(clip) {
                res.extra_sfxs.insert(name, clip);
            }
        });

        let judge = Judge::new(&chart);

        let music = Self::new_music(&mut res)?;
        Ok(Self {
            should_exit: false,
            next_scene: None,

            mode,
            res,
            chart,
            judge,
            gl: unsafe { get_internal_gl() },
            player,
            chart_bytes,
            effects,
            info_offset,

            first_in: false,
            exercise_range,
            exercise_press: None,
            exercise_btns: (RectButton::new(), RectButton::new()),

            music,

            state: State::Starting,
            last_update_time: 0.,
            pause_rewind: PauseRewind {
                time: None,
                duration: None,
                dim: false
            },
            pause_first_time: f32::NEG_INFINITY,

            bad_notes: Vec::new(),

            upload_fn,
            update_fn,

            touch_points: Vec::new(),
        })
    }

    fn new_music(res: &mut Resource) -> Result<Music> {
        res.audio.create_music(
            res.music.clone(),
            MusicParams {
                amplifier: res.config.volume_music as _,
                playback_rate: res.config.speed as _,
                ..Default::default()
            },
        )
    }

    fn touch_scale(&self) -> f32 {
        (screen_width() / screen_height()) / self.res.aspect_ratio
    }

    fn validate_value(value: &String) -> bool {
        if value == "AUTOPLAY" || value == "RECORD" {
            return false;
        }

        let filtered_value = RE_FILTER.replace_all(value, "").trim().to_string();
        return RE_VALIDATE.is_match(&filtered_value);
    }

    fn ui(&mut self, ui: &mut Ui, tm: &mut TimeManager) -> Result<()> {
        let time = tm.now() as f32;
        let p = match self.state {
            State::Starting => {
                if time <= Self::BEFORE_TIME {
                    1. - (1. - time / Self::BEFORE_TIME).clamp(0., 1.).powi(3)
                } else {
                    1.
                }
            }
            State::BeforeMusic => 1.,
            State::Playing => 1.,
            State::Ending => {
                let t = time - self.res.track_length - WAIT_TIME;
                1. - (t / (AFTER_TIME + 0.3)).clamp(0., 1.).powi(2)
            }
        };
        let c = Color::new(1., 1., 1., self.res.alpha);
        let res = &mut self.res;
        let aspect_ratio = res.aspect_ratio;
        let scale_ratio = 1.777777777777777;
        let top = -1.;
        let eps = 2e-2;
        let pause_w = 0.011 * scale_ratio;
        let pause_h = pause_w * 3.5;
        let pause_center = Point::new(-aspect_ratio + 0.0525 * scale_ratio, top + eps * 3.6454 - (1. - p) * 0.4 + pause_h / 2.);
        if res.config.interactive
            && !tm.paused()
            && self.pause_rewind.time.is_none()
            && Judge::get_touches().iter().any(|touch| {
                touch.phase == TouchPhase::Started && {
                    let p = touch.position;
                    let p = Point::new(p.x * aspect_ratio, p.y * aspect_ratio);
                    (pause_center - p).norm() < 0.05
                }
            })
        {
            let t = tm.now() as f32;
            if t - self.pause_first_time > PAUSE_CLICK_INTERVAL && res.config.double_click_to_pause {
                self.pause_first_time = t;
            } else {
                self.pause_first_time = f32::NEG_INFINITY;
                if !self.music.paused() {
                    self.music.pause()?;
                }
                tm.pause();
            }
        }
        if tm.now() as f32 - self.pause_first_time <= PAUSE_CLICK_INTERVAL {
            ui.fill_circle(pause_center.x, pause_center.y, 0.05 * scale_ratio, Color::new(1., 1., 1., 0.5));
        }

        let margin = 0.0425 * aspect_ratio;

        let score = if res.config.roman {
            Self::int_to_roman(self.judge.score())
        } else if res.config.chinese {
            Self::int_to_chinese(self.judge.score())
        }
        else {
            format!("{:07}", self.judge.score())
        };
        let score_top = top + eps * 2.2 - (1. - p) * 0.4;
        let ct = ui.text(&score).size(0.8 * aspect_ratio).center();
        ui.text("AA").color(Color::new(0., 0., 0., 0.)).draw(); //Fix first text disappear
        self.chart.with_element(ui, res, UIElement::Score, Some((-ct.x + aspect_ratio - margin, ct.y + score_top)), Some((aspect_ratio - margin + 0.001, top + eps * 2.8125)), |ui, color| {
            let mut text_size = 0.71 * scale_ratio;
            let mut text = ui.text(&score).size(text_size);
            let max_width = 0.55 * aspect_ratio;
            let text_width = text.measure().w;
            if text_width > max_width {
                text_size *= max_width / text_width
            }
            drop(text);
            if res.config.render_ui_score {
                ui.text(score)
                    .pos(aspect_ratio - margin + 0.001, top + eps * 2.8125 - (1. - p) * 0.4)
                    .anchor(1., 0.)
                    .size(text_size)
                    .color(Color { a: color.a * c.a, ..color })
                    .draw();
            }
            if res.config.show_acc {
                ui.text(format!("{:05.2}%", self.judge.real_time_accuracy() * 100.))
                    .pos(aspect_ratio - margin, top + eps * 2.2 - (1. - p) * 0.4 + 0.07 + 0.05)
                    .anchor(1., 0.)
                    .size(0.4 * scale_ratio)
                    .color(Color { a: color.a * c.a * 0.7, ..color })
                    .draw();
            }
        });
        self.chart.with_element(ui, res, UIElement::Pause, Some((pause_center.x, pause_center.y)), Some((pause_center.x - pause_w * 1.5, pause_center.y - pause_h * 0.5)), |ui, color| {
            if res.config.render_ui_pause {
                let mut r = Rect::new(pause_center.x - pause_w / 2., pause_center.y - pause_h / 2., pause_w, pause_h);
                //let ct = pause_center.coords;
                let c = Color { a: color.a * c.a, ..color };
                
                r.x -= pause_w;
                ui.fill_rect(r, c);
                r.x += pause_w * 2.;
                ui.fill_rect(r, c);
                ;
        }
        });
        let unit_h = ui.text("0").size(scale_ratio).measure().h;
        let combo_y = top + eps * 1.55 - (1. - p) * 0.4;
        if self.judge.combo() >= 3 {
            let combo = if res.config.roman {
                Self::int_to_roman(self.judge.combo())
            } else if res.config.chinese {
                Self::int_to_chinese(self.judge.combo())
            }
            else {
                self.judge.combo().to_string()
            };
            let btm = self.chart.with_element(ui, res, UIElement::ComboNumber, Some((0., combo_y + unit_h / 2. * 0.98)), Some((0., combo_y + unit_h / 2. * 0.98)), |ui, color| {
                let mut text_size = 0.98 * scale_ratio;
                let max_width = 0.55 * aspect_ratio;
                let mut text = ui.text(&combo)
                    .size(text_size)
                    .color(Color::new(0., 0., 0., 0.))
                    .pos(0., combo_y)
                    .anchor(0.5, 0.);
                let text_width = text.measure().w;
                let text_btm = text.draw().bottom();
                if text_width > max_width {
                    text_size *= max_width / text_width
                }
                if res.config.render_ui_combo {
                    ui.text(&combo)
                    .pos(0., top + eps * 1.30 - (1. - p) * 0.4)
                    .anchor(0.5, 0.)
                    .color(Color { a: color.a * c.a, ..color })
                    .size(text_size)
                    .draw();
                }
                text_btm
            });
            self.chart.with_element(ui, res, UIElement::Combo, Some((0., btm + 0.01 + unit_h / 2. * 0.34)), Some((0., btm + 0.01 + unit_h / 2. * 0.34)), |ui, color| {
                if Self::validate_value(&res.config.combo) || res.config.combo.len() > 50 {
                    ui.text("AUTOPLAY")
                    .pos(0., btm + 0.01)
                    .anchor(0.5, 0.)
                    .size(0.34 * scale_ratio)
                    .color(Color { a: color.a * c.a, ..color })
                    .draw();
                    return;
                }
                ui.text(&res.config.combo)
                    .pos(0., btm + 0.01)
                    .anchor(0.5, 0.)
                    .size(0.34 * scale_ratio)
                    .color(Color { a: color.a * c.a, ..color })
                    .draw();
            });

        }
        let lf = -aspect_ratio + margin;
        let bt = -top - eps * 3.5;
        self.chart.with_element(ui, res, UIElement::Name, Some((lf + ct.x, bt - ct.y)), Some((lf, -top - eps * 2.)), |ui, color| {
            if res.config.render_ui_name {
                let mut text_size = 0.505 * scale_ratio;
                let mut text = ui.text(&res.info.name).size(text_size);
                let max_width = 0.9 * aspect_ratio;
                let text_width = text.measure().w;
                if text_width > max_width {
                    text_size *= max_width / text_width
                }
                drop(text);
                ui.text(&res.info.name)
                    .pos(lf, bt + (1. - p) * 0.4)
                    .anchor(0., 1.)
                    .size(text_size)
                    .color(Color { a: color.a * c.a, ..color })
                    .draw();
            }
        });
        self.chart.with_element(ui, res, UIElement::Level, Some((-lf - ct.x, bt - ct.y)), Some((-lf, -top - eps * 2.)), |ui, color| {
            if res.config.render_ui_level {
                ui.text(&res.info.level)
                    .pos(-lf, bt + (1. - p) * 0.4)
                    .anchor(1., 1.)
                    .size(0.505 * scale_ratio)
                    .color(Color { a: color.a * c.a, ..color })
                    .draw();
            }

            /*let watermark = if res.config.watermark == "AntiLeave" { 
                "".to_string() 
            } else { 
                format!("{}Phigros Recorder - Code by HLMC", res.config.watermark) 
            };*/
        });
        { // self.chart.with_element(ui, res, UIElement::Null, None, |ui, color| ...)
            let watermark = res.config.watermark.clone();
            ui.text(&watermark)
                .pos(0., -top * 0.98 + (1. - p) * 0.4)
                .anchor(0.5, 1.)
                .size(0.25 * scale_ratio)
                .color(Color::new(1., 1., 1., 0.5 * c.a))
                .draw();
            if res.config.chart_ratio <= 0.95 {
                ui.text(&watermark)
                .pos(0., (-top * 0.98 + (1. - p) * 0.4) / res.config.chart_ratio)
                .anchor(0.5, 1.)
                .size(0.25 * scale_ratio / res.config.chart_ratio)
                .color(Color::new(1., 1., 1., 0.5 * c.a))
                .draw();
            }
        };
        let hw = 0.003;
        let height = eps * 1.0;
        let dest = (aspect_ratio * 2. * res.time / res.track_length).max(0.).min(aspect_ratio * 2.);
        self.chart.with_element(ui, res, UIElement::Bar, Some((-aspect_ratio, top + height / 2.)), Some((-aspect_ratio, top + height / 2.)), |ui, color| {
            if res.config.render_ui_bar {
                //let ct = Vector::new(0., top + height / 2.);
                ui.fill_rect(
                    Rect::new(-aspect_ratio, top, dest, height),
                    //Color{ a: color.a * c.a * 0.6, ..color},
                    Color::new(0.565, 0.565, 0.565, color.a * c.a),
                );
                ui.fill_rect(Rect::new(-aspect_ratio + dest - hw, top, hw * 2., height), Color::new(1., 1., 1., color.a * c.a));
            }
        });
        Ok(())
    }

    fn overlay_ui(&mut self, ui: &mut Ui, tm: &mut TimeManager) -> Result<()> {
        let c = semi_white(self.res.alpha);
        let res = &mut self.res;
        for pos in &self.touch_points {
            ui.fill_circle(pos.0, pos.1, 0.04, Color { a: 0.4, ..BLUE });
        }
        if tm.paused() {
            //let o = if self.mode == GameMode::Exercise { -0.3 } else { 0. };
            let o = -0.3;
            let s = 0.06;
            let w = 0.05;
            let no_retry = self.mode == GameMode::NoRetry;
            draw_texture_ex(
                *res.icon_back,
                -s * 3. - w,
                -s + o,
                c,
                DrawTextureParams {
                    dest_size: Some(vec2(s * 2., s * 2.)),
                    ..Default::default()
                },
            );
            draw_texture_ex(
                *res.icon_retry,
                -s,
                -s + o,
                if no_retry { semi_white(res.alpha * 0.6) } else { c },
                DrawTextureParams {
                    dest_size: Some(vec2(s * 2., s * 2.)),
                    ..Default::default()
                },
            );
            draw_texture_ex(
                *res.icon_resume,
                s + w,
                -s + o,
                c,
                DrawTextureParams {
                    dest_size: Some(vec2(s * 2., s * 2.)),
                    ..Default::default()
                },
            );
            if res.config.interactive {
                let mut clicked = None;
                for touch in Judge::get_touches() {
                    if touch.phase != TouchPhase::Started {
                        continue;
                    }
                    let p = touch.position;
                    let p = Point::new(p.x, p.y);
                    for i in -1..=1 {
                        let ct = Point::new((s * 2. + w) * i as f32, o);
                        let d = p - ct;
                        if d.x.abs() <= s && d.y.abs() <= s {
                            clicked = Some(i);
                            break;
                        }
                    }
                }
                if no_retry && clicked == Some(0) {
                    clicked = None;
                }
                let mut pos = self.music.position();
                if clicked.map_or(false, |it| it != -1) && (tm.speed - res.config.speed as f64).abs() > 0.01 {
                    debug!("recreating music");
                    self.music = res.audio.create_music(
                        res.music.clone(),
                        MusicParams {
                            amplifier: res.config.volume_music as _,
                            playback_rate: res.config.speed as _,
                            ..Default::default()
                        },
                    )?;
                }
                match clicked {
                    Some(-1) => {
                        self.should_exit = true;
                    }
                    Some(0) => {
                        reset!(self, res, tm);
                        self.pause_rewind = PauseRewind {
                            time: Some(tm.now()),
                            duration: Some(0.1),
                            dim: false,
                        };
                        res.config.disable_audio = true;
                    }
                    Some(1) => {
                        if tm.now() > self.exercise_range.end as f64 { //self.mode == GameMode::Exercise && 
                            tm.seek_to(self.exercise_range.start as f64);
                            self.music.seek_to(self.exercise_range.start)?;
                            pos = self.exercise_range.start;
                        }
                        self.music.play()?;
                        res.time -= 1.;
                        let dst = pos - 1.;
                        if dst < 0. {
                            self.music.pause()?;
                            self.state = State::BeforeMusic;
                        } else {
                            self.music.seek_to(dst)?;
                        }
                        let now = tm.now();
                        tm.speed = res.config.speed as _;
                        tm.resume();
                        tm.seek_to(now - 1.);
                        self.music.seek_to(now as f32 - 1.);
                        self.pause_rewind = PauseRewind {
                            time: Some(tm.now()),
                            duration: Some(1.0),
                            dim: true
                        };
                        self.res.config.disable_audio = true;
                    }
                    _ => {}
                }
            }
            { //if self.mode == GameMode::Exercise
                let asp = self.touch_scale();
                for touch in ui.ensure_touches() {
                    touch.position *= asp;
                }
                if self.mode == GameMode::Exercise {
                    ui.scope(|ui| {
                        ui.dx(0.3);
                        ui.dy(-0.3);
                        ui.slider(tl!("speed"), 0.5..2.0, 0.05, &mut self.res.config.speed, Some(0.5));
                    });
                }
                ui.dy(0.06);
                let hw = 0.7;
                let h = 0.06;
                let eh = 0.12;
                let rad = 0.03;
                let sp = self.offset().min(0.);
                ui.fill_rect(Rect::new(-hw, -h, hw * 2., h * 2.), Color::new(0.4, 0.4, 0.4, 1.));
                let st = -hw + (self.exercise_range.start - sp) / (self.res.track_length - sp) * hw * 2.;
                let en = -hw + (self.exercise_range.end - sp) / (self.res.track_length - sp) * hw * 2.;
                let t = tm.now() as f32;
                let cur = -hw + (t - sp) / (self.res.track_length - sp) * hw * 2.;
                ui.fill_rect(Rect::new(st, -h, en - st, h * 2.), Color::new(0.6, 0.6, 0.6, 1.));
                ui.fill_rect(Rect::new(st, -eh, 0., eh + h).feather(0.005), Color::new(0.66, 0.78, 0.98, 1.));
                ui.fill_circle(st, -eh, rad, Color::new(0.66, 0.78, 0.98, 1.));
                if self.exercise_press.is_none() {
                    let r = ui.rect_to_global(Rect::new(st, -eh, 0., 0.).feather(rad));
                    self.exercise_press = Judge::get_touches()
                        .iter()
                        .find(|it| it.phase == TouchPhase::Started && r.contains(it.position))
                        .map(|it| (-1, it.id));
                }
                ui.fill_rect(Rect::new(en, -h, 0., eh + h).feather(0.005), Color::new(1., 0.34, 0.54, 1.));
                ui.fill_circle(en, eh, rad, Color::new(1., 0.34, 0.54, 1.));
                if self.exercise_press.is_none() {
                    let r = ui.rect_to_global(Rect::new(en, eh, 0., 0.).feather(rad));
                    self.exercise_press = Judge::get_touches()
                        .iter()
                        .find(|it| it.phase == TouchPhase::Started && r.contains(it.position))
                        .map(|it| (1, it.id));
                }
                ui.fill_rect(Rect::new(cur, -h, 0., h * 2.).feather(0.005), Color::new(0.9, 0.9, 0.9, 1.));
                ui.fill_circle(cur, 0., rad, Color::new(0.95, 0.95, 0.95, 1.));
                if self.exercise_press.is_none() {
                    let r = ui.rect_to_global(Rect::new(cur, 0., 0., 0.).feather(rad));
                    self.exercise_press = Judge::get_touches()
                        .iter()
                        .find(|it| it.phase == TouchPhase::Started && r.contains(it.position))
                        .map(|it| (0, it.id));
                }
                ui.text(fmt_time(t)).pos(0., -0.23).anchor(0.5, 0.).size(0.8).draw();
                if let Some((ctrl, id)) = &self.exercise_press {
                    if let Some(touch) = Judge::get_touches().iter().rfind(|it| it.id == *id) {
                        let x = touch.position.x;
                        let p = (x + hw) / (hw * 2.) * (self.res.track_length - sp) + sp;
                        let p = if self.res.track_length - sp <= 3. || *ctrl == 0 {
                            p.clamp(sp, self.res.track_length)
                        } else {
                            p.clamp(
                                if *ctrl == -1 { sp } else { self.exercise_range.start + 3. },
                                if *ctrl == -1 {
                                    self.exercise_range.end - 3.
                                } else {
                                    self.res.track_length
                                },
                            )
                        };
                        if *ctrl == 0 {
                            tm.seek_to(p as f64);
                            self.music.seek_to(p)?;
                        } else {
                            *(if *ctrl == -1 {
                                &mut self.exercise_range.start
                            } else {
                                &mut self.exercise_range.end
                            }) = p;
                        }
                        if matches!(touch.phase, TouchPhase::Cancelled | TouchPhase::Ended) {
                            self.exercise_press = None;
                        }
                    }
                }
                ui.dy(0.2);
                let r = ui.text(tl!("to")).size(0.8).anchor(0.5, 0.).draw();
                let mut tx = ui
                    .text(fmt_time(self.exercise_range.start))
                    .pos(r.x - 0.02, 0.)
                    .anchor(1., 0.)
                    .size(0.8)
                    .color(BLACK);
                let re = tx.measure();
                self.exercise_btns.0.set(tx.ui, re);
                tx.ui
                    .fill_rect(re.feather(0.01), Color::new(1., 1., 1., if self.exercise_btns.0.touching() { 0.5 } else { 1. }));
                tx.draw();

                let mut tx = ui
                    .text(fmt_time(self.exercise_range.end))
                    .pos(r.right() + 0.02, 0.)
                    .size(0.8)
                    .color(BLACK);
                let re = tx.measure();
                self.exercise_btns.1.set(tx.ui, re);
                tx.ui
                    .fill_rect(re.feather(0.01), Color::new(1., 1., 1., if self.exercise_btns.1.touching() { 0.5 } else { 1. }));
                tx.draw();
                for touch in ui.ensure_touches() {
                    touch.position /= asp;
                }
            }
        }
        if let PauseRewind {
            time: Some(time),
            duration: Some(duration),
            dim
        } = self.pause_rewind {
            let dt = tm.now() - time;
            let t = duration - dt;
            if t <= 0. {
                self.pause_rewind = PauseRewind {
                    time: None,
                    duration: None,
                    dim: false
                };
                self.res.config.disable_audio = false;
            } else if dim {
                let a = (duration - dt / duration).clamp(0.0, 1.0) * 0.75;
                let h = 1. / self.res.aspect_ratio;
                draw_rectangle(-1., -h, 2., h * 2., Color::new(0., 0., 0., a as f32));
                ui.text((t.ceil() as i32).to_string()).anchor(0.5, 0.5).size(1.).color(c).draw();
            }
        }
        Ok(())
    }

    fn interactive(res: &Resource, state: &State) -> bool {
        res.config.interactive && matches!(state, State::Playing)
    }

    fn offset(&self) -> f32 {
        self.chart.offset + self.res.config.offset + self.info_offset
    }

    fn tweak_offset(&mut self, ui: &mut Ui, ita: bool, tm: &mut TimeManager) {
        let width = 0.55;
        let height = 0.3;
        ui.scope(|ui| {
            ui.dx(1. - width - 0.02);
            ui.dy(ui.top - height - 0.02);
            ui.fill_rect(Rect::new(0., 0., width, height), Color { r: 0.13, g: 0.13, b: 0.13, a: 0.5 });
            ui.dy(0.02);
            ui.text(tl!("adjust-offset")).pos(width / 2., 0.).anchor(0.5, 0.).size(0.7).draw();

            ui.dx(width / 1.22);
            if ui.button("cancel", Rect::new(0.02, 0., 0.06, 0.06), "×") {
                self.next_scene = Some(NextScene::PopWithResult(Box::new(Some(self.info_offset))));
            }
            ui.dx(-width / 1.22);

            ui.dy(0.20);
            let r = ui
                .text(format!("{}ms", (self.info_offset * 1000.).round() as i32))
                .pos(width / 2., 0.)
                .anchor(0.5, 0.)
                .size(0.6)
                .no_baseline()
                .draw();
            let d = 0.14;
            let mut bpm_list = self.chart.bpm_list.borrow_mut();
            let beat = (15. / bpm_list.now_bpm(tm.now() as f32)).clamp(0.020, 0.500);
            if ui.button("lg_sub", Rect::new(d, r.center().y, 0., 0.).feather(0.026), "-") && ita {
                self.info_offset -= beat;
            }
            if ui.button("lg_add", Rect::new(width - d, r.center().y, 0., 0.).feather(0.026), "+") && ita {
                self.info_offset += beat;
            }
            let d = 0.08;
            if ui.button("sm_sub", Rect::new(d, r.center().y, 0., 0.).feather(0.022), "-") && ita {
                self.info_offset -= 0.01;
            }
            if ui.button("sm_add", Rect::new(width - d, r.center().y, 0., 0.).feather(0.022), "+") && ita {
                self.info_offset += 0.01;
            }
            let d = 0.03;
            if ui.button("ti_sub", Rect::new(d, r.center().y, 0., 0.).feather(0.017), "-") && ita {
                self.info_offset -= 0.001;
            }
            if ui.button("ti_add", Rect::new(width - d, r.center().y, 0., 0.).feather(0.017), "+") && ita {
                self.info_offset += 0.001;
            }
            /*ui.dy(0.10);
            let pad = 0.02;
            let spacing = 0.01;
            let mut r = Rect::new(pad, 0., (width - pad * 2. - spacing * 2.) / 3., 0.06);
            if ui.button("cancel", r, tl!("offset-cancel")) {
                self.next_scene = Some(NextScene::PopWithResult(Box::new(None::<f32>)));
            }
            r.x += r.w + spacing;
            if ui.button("reset", r, tl!("offset-reset")) {
                self.info_offset = 0.;
            }
            r.x += r.w + spacing;
            if ui.button("save", r, tl!("offset-save")) {
                //self.res.info.offset = self.info_offset;
                self.next_scene = Some(NextScene::PopWithResult(Box::new(Some(self.info_offset))));
            }*/
        });
        ui.scope(|ui| {
            ui.dx(1. - width * 0.97);
            ui.dy(ui.top - height * 0.75);
            ui.slider(tl!("speed"), 0.1..2.0, 0.05, &mut self.res.config.speed, Some(0.3));
            if ui.button("save-speed", Rect::new(0.44, 0.033, 0.05, 0.05), "=") && (tm.speed - self.res.config.speed as f64).abs() > 0.01 {
                debug!("recreating music");
                self.music = self.res.audio.create_music(
                    self.res.music.clone(),
                    MusicParams {
                        amplifier: self.res.config.volume_music as _,
                        playback_rate: self.res.config.speed as _,
                        ..Default::default()
                    },
                ).expect("failed to create music");
                reset_speed!(self, self.res, tm);
            }
        });
    }
}

impl Scene for GameScene {
    fn enter(&mut self, tm: &mut TimeManager, target: Option<RenderTarget>) -> Result<()> {
        #[cfg(target_arch = "wasm32")]
        on_game_start();
        self.music = Self::new_music(&mut self.res)?;
        self.res.camera.render_target = target;
        tm.speed = self.res.config.speed as _;
        tm.adjust_time = self.res.config.adjust_time;
        reset!(self, self.res, tm);
        set_camera(&self.res.camera);
        self.first_in = true;
        Ok(())
    }

    fn pause(&mut self, tm: &mut TimeManager) -> Result<()> {
        if !tm.paused() {
            self.pause_rewind = PauseRewind {
                time: None,
                duration: None,
                dim: false
            };
            self.music.pause()?;
            tm.pause();
        }
        Ok(())
    }

    fn resume(&mut self, tm: &mut TimeManager) -> Result<()> {
        if !matches!(self.state, State::Playing) {
            tm.resume();
        }
        Ok(())
    }

    fn update(&mut self, tm: &mut TimeManager) -> Result<()> {
        self.res.audio.recover_if_needed()?;
        if matches!(self.state, State::Playing) {
            tm.update(self.music.position() as f64);
        }
        if self.mode == GameMode::Exercise && tm.now() > self.exercise_range.end as f64 && !tm.paused() {
            let state = self.state.clone();
            reset!(self, self.res, tm);
            self.state = state;
            tm.seek_to(self.exercise_range.start as f64);
            tm.pause();
            self.music.pause()?;
        }
        let offset = self.offset();
        let time = tm.now() as f32;
        let time = match self.state {
        State::Starting => {
            if time >= Self::BEFORE_DURATION { // wait for animation
                self.res.alpha = 1.;
                self.state = State::BeforeMusic;
                tm.reset();
                tm.seek_to(self.exercise_range.start as f64);
                self.last_update_time = tm.real_time();
                if self.first_in && self.mode == GameMode::Exercise {
                    tm.pause();
                    self.first_in = false;
                }
                tm.now() as f32
            } else {
                self.res.alpha = 1. - (1. - time / Self::BEFORE_TIME).clamp(0., 1.).powi(3);
                self.exercise_range.start
            }
            }
            State::BeforeMusic => {
                if time >= 0.0 {
                    self.music.seek_to(time)?;
                    self.music.play()?;
                    self.state = State::Playing;
                }
                time
            }
            State::Playing => {
                if time > self.res.track_length + WAIT_TIME {
                    self.state = State::Ending;
                }
                time
            }
            State::Ending => {
                let t = time - self.res.track_length - WAIT_TIME;
                if t >= AFTER_TIME + 0.3 {
                    let mut record_data = None;
                    // TODO strengthen the protection
                    #[cfg(feature = "closed")]
                    if let Some(upload_fn) = &self.upload_fn {
                        if !self.res.config.offline_mode && !self.res.config.autoplay() && self.res.config.speed >= 1.0 - 1e-3 {
                            if let Some(player) = &self.player {
                                if let Some(chart) = &self.res.info.id {
                                    record_data = Some(encode_record(self, player.id, *chart));
                                }
                            }
                        }
                    }
                    let result = self.judge.result();
                    let record = if self.res.config.autoplay() || self.res.config.speed < 1.0 - 1e-3 {
                        None
                    } else {
                        Some(SimpleRecord {
                            score: result.score as _,
                            accuracy: result.accuracy as _,
                            full_combo: result.max_combo == result.num_of_notes,
                        })
                    };
                    self.next_scene = match self.mode {
                        GameMode::Normal | GameMode::NoRetry | GameMode::View => Some(NextScene::Overlay(Box::new(EndingScene::new(
                            self.res.background.clone(),
                            self.res.illustration.clone(),
                            self.res.player.clone(),
                            self.res.icons.clone(),
                            self.res.icon_retry.clone(),
                            self.res.icon_proceed.clone(),
                            self.res.info.clone(),
                            self.judge.result(),
                            self.res.challenge_icons[self.res.config.challenge_color.clone() as usize].clone(),
                            &self.res.config,
                            self.res.res_pack.ending.clone(),
                            self.upload_fn.as_ref().map(Arc::clone),
                            self.player.as_ref().map(|it| it.rks),
                            record_data,
                            record,
                        )?))),
                        GameMode::TweakOffset => Some(NextScene::PopWithResult(Box::new(None::<f32>))),
                        GameMode::Exercise => None,
                    };
                }
                self.res.alpha = 1. - (t / AFTER_TIME).clamp(0., 1.).powi(2);
                self.res.track_length
            }
        };
        let time = (time - offset).max(0.);
        self.res.time = time;
        if !tm.paused() /*&& self.pause_rewind.is_none()*/ && self.mode != GameMode::View {
            self.gl.quad_gl.viewport(self.res.camera.viewport);
            self.judge.update(&mut self.res, &mut self.chart, &mut self.bad_notes);
            self.gl.quad_gl.viewport(None);
        }
        if let Some(update) = &mut self.update_fn {
            update(self.res.time, &mut self.res, &mut self.judge);
        }
        let counts = self.judge.counts();
        self.res.judge_line_color = if counts[2] + counts[3] == 0 {
            Color::from_hex(if counts[1] == 0 {
                self.res.res_pack.info.color_perfect_line
            } else {
                self.res.res_pack.info.color_good_line
            })
        } else {
            WHITE
        };
        self.res.judge_line_color.a *= self.res.alpha;
        self.chart.update(&mut self.res);
        let res = &mut self.res;
        if !tm.paused() {
            for video in &mut self.chart.extra.videos {
                if let Err(err) = video.update(res.time) {
                    warn!("video error: {err:?}");
                }
            }
        }
        if res.config.interactive && is_key_pressed(KeyCode::Space) {
            if tm.paused() {
                if matches!(self.state, State::Playing) {
                    self.music.play()?;
                    tm.resume();
                    self.pause_rewind = PauseRewind {
                        time: Some(tm.now()),
                        duration: Some(0.1),
                        dim: false
                    };
                    res.config.disable_audio = true;
                }
            } else if matches!(self.state, State::Playing) && !self.pause_rewind.dim { // State::BeforeMusic
                if !self.music.paused() {
                    self.music.pause()?;
                }
                self.pause_rewind = PauseRewind {
                    time: None,
                    duration: None,
                    dim: false
                };
                tm.pause();
            }
        }
        if Self::interactive(res, &self.state) {
            if is_key_pressed(KeyCode::Left) {
                res.time -= 2.;
                let dst = (self.music.position() - 2.).max(0.);
                self.music.seek_to(dst)?;
                tm.seek_to(dst as f64);
            }
            if is_key_pressed(KeyCode::Right) {
                res.time += 5.;
                let dst = (self.music.position() + 5.).min(res.track_length);
                self.music.seek_to(dst)?;
                tm.seek_to(dst as f64);

                self.pause_rewind = PauseRewind {
                    time: Some(tm.now()),
                    duration: Some(0.1),
                    dim: false
                };
                res.config.disable_audio = true;
            }
            if is_key_pressed(KeyCode::Q) {
                self.should_exit = true;
            }
        }
        for effect in &mut self.effects {
            effect.update(&self.res);
        }
        if let Some((id, text)) = take_input() {
            let offset = self.offset().min(0.);
            match id.as_str() {
                "exercise_start" => {
                    if let Some(t) = parse_time(&text) {
                        if !(offset..self.res.track_length.min(self.exercise_range.end - 3.).max(offset)).contains(&t) {
                            show_message(tl!("ex-time-out-of-range")).error();
                        } else {
                            self.exercise_range.start = t;
                            show_message(tl!("ex-time-set")).ok();
                        }
                    } else {
                        show_message(tl!("ex-invalid-format")).error();
                    }
                }
                "exercise_end" => {
                    if let Some(t) = parse_time(&text) {
                        if !((self.exercise_range.start + 3.).max(offset).min(self.res.track_length)..self.res.track_length).contains(&t) {
                            show_message(tl!("ex-time-out-of-range")).error();
                        } else {
                            self.exercise_range.end = t;
                            show_message(tl!("ex-time-set")).ok();
                        }
                    } else {
                        show_message(tl!("ex-invalid-format")).error();
                    }
                }
                _ => return_input(id, text),
            }
        }
        Ok(())
    }

    fn touch(&mut self, tm: &mut TimeManager, touch: &Touch) -> Result<bool> {
        if self.mode == GameMode::Exercise && tm.paused() {
            let touch = Touch {
                position: touch.position * self.touch_scale(),
                ..touch.clone()
            };
            if self.exercise_btns.0.touch(&touch) {
                request_input("exercise_start", &fmt_time(self.exercise_range.start));
                return Ok(true);
            }
            if self.exercise_btns.1.touch(&touch) {
                request_input("exercise_end", &fmt_time(self.exercise_range.end));
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn render(&mut self, tm: &mut TimeManager, ui: &mut Ui) -> Result<()> {
        let res = &mut self.res;

        let time = tm.now() as f32;
        let p = match self.state {
            State::Starting => {
                if time < Self::BEFORE_DURATION {
                    1. - (1. - time / Self::BEFORE_DURATION)
                } else {
                    1.
                }
            }
            State::BeforeMusic => 1.,
            State::Ending | State::Playing => {
                let t = time - res.track_length;
                1. - (t / Self::BEFORE_DURATION).clamp(0., 1.)
            }
        };
        let ratio = if res.config.chart_ratio == 1. || res.config.disable_loading {
            res.config.chart_ratio
        } else {
            1. + (res.config.chart_ratio - 1.) * ease_in_out_quartic(p)
        };

        if res.update_size(ui.viewport) || self.mode == GameMode::View {
            set_camera(&res.camera);
        }

        let msaa = res.config.sample_count > 1;

        // camera setup
        let vp = res.camera.viewport.unwrap_or(ui.viewport);
        let asp2_window = ui.viewport.2 as f32 / ui.viewport.3 as f32;
        let asp2_chart = vp.2 as f32 / vp.3 as f32;
        let asp2_ui = vp.3 as f32 / vp.2 as f32;
        let asp2_ui_window = ui.viewport.3 as f32 / ui.viewport.2 as f32;

        let viewport_chart = if res.chart_target.is_some() {
            Some((vp.0 - ui.viewport.0, vp.1 - ui.viewport.1, vp.2, vp.3))
        } else {
            res.camera.viewport
        };
        let viewport_window = Some(ui.viewport);

        let chart_onto = res
            .chart_target
            .as_ref()
            .map(|it| if msaa { it.input() } else { it.output() })
            .or(res.camera.render_target);

        let h = 1. / res.aspect_ratio;
        set_camera(&Camera2D {
            zoom: vec2(1., -asp2_window),
            viewport: if res.chart_target.is_some() { None } else { viewport_window },
            render_target: chart_onto,
            ..Default::default()
        });
        if res.config.render_bg {
            clear_background(BLACK);
            draw_background(*res.background, res.config.render_bg_dim);
        }

        if res.config.render_bg_dim && res.config.chart_ratio >= 1. {
            let dim_alpha = 0.7;
            //let alpha = res.alpha * (1. - dim_alpha) + dim_alpha;    
            let dim = Color::new(0.1, 0.1, 0.1, dim_alpha * res.alpha);
            let x_range = vp.0 as f32 / ui.viewport.2 as f32;
            draw_rectangle(-1., -h,x_range * 2., h * 2., dim);
            draw_rectangle(1., -h,-x_range * 2., h * 2., dim);
            draw_rectangle(x_range * 2. - 1., -h, (1. - x_range * 2.) * 2., h * 2., Color::new(0., 0., 0., res.alpha * res.info.background_dim));
        }

        set_camera( &Camera2D {
            zoom: if res.config.chart_ratio < 1. { vec2(asp2_chart / asp2_window * ratio, -asp2_chart * ratio) } else { vec2(1. * ratio, -asp2_chart * ratio) },
            viewport: if res.config.chart_ratio < 1. { viewport_window } else { viewport_chart },
            ..Default::default()
        });
        
        self.gl.quad_gl.render_pass(chart_onto.map(|it| it.render_pass));
        //self.gl.quad_gl.viewport(chart_target_vp);
        if res.config.render_bg_dim && res.config.chart_ratio < 1. {
            draw_rectangle(-1., -h, 2., h * 2., Color::new(0., 0., 0., res.alpha * res.info.background_dim));
        }
        self.chart.render(&mut self.music, tm, ui, res);

        self.gl.quad_gl.render_pass(
            res.chart_target
                .as_ref()
                .map(|it| it.output().render_pass)
                .or_else(|| res.camera.render_pass()),
        );

        self.bad_notes.retain(|dummy| dummy.render(res));
        let t = tm.real_time();
        let dt = (t - std::mem::replace(&mut self.last_update_time, t)) as f32;
        if res.config.particle {
            res.emitter.draw(dt);
        }

        if !res.no_effect {
            set_camera(&Camera2D {
                zoom: vec2(1., asp2_chart),
                ..Default::default()
            });
            for effect in &self.chart.extra.effects {
                effect.render(res);
            }
        }
        
        {
            set_camera(&Camera2D {
                zoom: if res.config.chart_ratio < 1. { vec2(asp2_ui_window * ratio, -1. * ratio) } else { vec2(asp2_ui * ratio, -1. * ratio) },
                viewport: if res.config.chart_ratio < 1. { viewport_window } else { viewport_chart },
                render_target: self.res.chart_target.as_ref().map(|it| it.output()).or(self.res.camera.render_target),
                ..Default::default()
            });
            self.ui(ui, tm)?;
        }

        if !self.res.no_effect && !self.effects.is_empty() {
            set_camera(&Camera2D {
                zoom: vec2(1., asp2_window),
                ..Default::default()
            });
            for effect in &self.effects {
                effect.render(&mut self.res);
            }
        }

        {
            set_camera(&Camera2D {
                zoom: vec2(1., 1.),
                viewport: viewport_window,
                render_target: self.res.chart_target.as_ref().map(|it| it.output()).or(self.res.camera.render_target),
                ..Default::default()
            });
            if tm.paused() {
                draw_rectangle(-1., -1., 2., 2., Color::new(0., 0., 0., 0.6));
            }
        }

        {
            set_camera(&Camera2D {
                zoom: vec2(1., -asp2_window),
                viewport: viewport_window,
                render_target: self.res.chart_target.as_ref().map(|it| it.output()).or(self.res.camera.render_target),
                ..Default::default()
            });
            if self.mode == GameMode::TweakOffset {
                self.tweak_offset(ui, Self::interactive(&self.res, &self.state), tm);
            }
            if self.res.config.touch_debug {
                for touch in Judge::get_touches() {
                    ui.fill_circle(touch.position.x, touch.position.y, 0.04, Color { a: 0.4, ..RED });
                }
            }
        }
        
        {
            set_camera(&Camera2D {
                zoom: vec2(1., -asp2_chart),
                viewport: viewport_chart,
                render_target: self.res.chart_target.as_ref().map(|it| it.output()).or(self.res.camera.render_target),
                ..Default::default()
            });
            self.overlay_ui(ui, tm)?;
        }

        if msaa || !self.res.no_effect {
            // render the texture onto screen
            if let Some(target) = &self.res.chart_target {
                self.gl.flush();
                self.gl.quad_gl.viewport(None);
                set_camera(&Camera2D {
                    zoom: vec2(1., asp2_window),
                    render_target: self.res.camera.render_target,
                    viewport: viewport_window,
                    ..Default::default()
                });
                draw_texture_ex(
                    target.output().texture,
                    -1.,
                    -ui.top,
                    WHITE,
                    DrawTextureParams {
                        dest_size: Some(vec2(2., ui.top * 2.)),
                        ..Default::default()
                    },
                );
            }
        } else {
            self.gl.flush();
        }
        Ok(())
    }

    fn next_scene(&mut self, tm: &mut TimeManager) -> NextScene {
        if self.should_exit {
            if tm.paused() {
                tm.resume();
            }
            tm.speed = 1.0;
            tm.adjust_time = false;
            match self.mode {
                GameMode::Normal | GameMode::Exercise | GameMode::NoRetry | GameMode::View => NextScene::Pop,
                GameMode::TweakOffset => NextScene::PopWithResult(Box::new(None::<f32>)),
            }
        } else if let Some(next_scene) = self.next_scene.take() {
            tm.speed = 1.0;
            tm.adjust_time = false;
            next_scene
        } else {
            NextScene::None
        }
    }
}
