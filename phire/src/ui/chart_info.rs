crate::tl_file!("chart_info");

use super::{InlineInputBtn, Ui};
use crate::{ext::parse_time, info::ChartInfo, scene::show_message};
use anyhow::Result;
use macroquad::prelude::*;
use std::{borrow::Cow, collections::HashMap};

struct ChartInfoInputs {
    name: InlineInputBtn,
    charter: InlineInputBtn,
    composer: InlineInputBtn,
    illustrator: InlineInputBtn,
    level: InlineInputBtn,
    preview_time: InlineInputBtn,
    offset: InlineInputBtn,
    aspect_ratio: InlineInputBtn,
    score_total: InlineInputBtn,
    line_length: InlineInputBtn,
    hold_particle_interval_ratio: InlineInputBtn,
    tip: InlineInputBtn,
    intro: InlineInputBtn,
}

impl ChartInfoInputs {
    fn new() -> Self {
        fn input() -> InlineInputBtn {
            let mut input = InlineInputBtn::new();
            input.set_centered();
            input
        }
        Self {
            name: input(),
            charter: input(),
            composer: input(),
            illustrator: input(),
            level: input(),
            preview_time: input(),
            offset: input(),
            aspect_ratio: input(),
            score_total: input(),
            line_length: input(),
            hold_particle_interval_ratio: input(),
            tip: input(),
            intro: input(),
        }
    }

    fn is_active(&self) -> bool {
        self.name.is_active()
            || self.charter.is_active()
            || self.composer.is_active()
            || self.illustrator.is_active()
            || self.level.is_active()
            || self.preview_time.is_active()
            || self.offset.is_active()
            || self.aspect_ratio.is_active()
            || self.score_total.is_active()
            || self.line_length.is_active()
            || self.hold_particle_interval_ratio.is_active()
            || self.tip.is_active()
            || self.intro.is_active()
    }

    fn touch(&mut self, touch: &Touch) {
        self.name.touch(touch);
        self.charter.touch(touch);
        self.composer.touch(touch);
        self.illustrator.touch(touch);
        self.level.touch(touch);
        self.preview_time.touch(touch);
        self.offset.touch(touch);
        self.aspect_ratio.touch(touch);
        self.score_total.touch(touch);
        self.line_length.touch(touch);
        self.hold_particle_interval_ratio.touch(touch);
        self.tip.touch(touch);
        self.intro.touch(touch);
    }

    fn update(&mut self) {
        self.name.update();
        self.charter.update();
        self.composer.update();
        self.illustrator.update();
        self.level.update();
        self.preview_time.update();
        self.offset.update();
        self.aspect_ratio.update();
        self.score_total.update();
        self.line_length.update();
        self.hold_particle_interval_ratio.update();
        self.tip.update();
        self.intro.update();
    }
}

pub struct ChartInfoEdit {
    pub info: ChartInfo,
    pub chart: Option<String>,
    pub music: Option<String>,
    pub illustration: Option<String>,
    inputs: ChartInfoInputs,
}

impl Clone for ChartInfoEdit {
    fn clone(&self) -> Self {
        Self {
            info: self.info.clone(),
            chart: self.chart.clone(),
            music: self.music.clone(),
            illustration: self.illustration.clone(),
            inputs: ChartInfoInputs::new(),
        }
    }
}

impl ChartInfoEdit {
    pub fn new(info: ChartInfo) -> Self {
        Self {
            info,
            chart: None,
            music: None,
            illustration: None,
            inputs: ChartInfoInputs::new(),
        }
    }

    pub async fn to_patches(&self) -> Result<HashMap<String, Vec<u8>>> {
        let mut res = HashMap::new();
        res.insert("info.yml".to_owned(), serde_yaml::to_string(&self.info)?.into_bytes());
        #[cfg(not(target_arch = "wasm32"))]
        {
            if let Some(chart) = &self.chart {
                res.insert(self.info.chart.clone(), tokio::fs::read(chart).await?);
            }
            if let Some(music) = &self.music {
                res.insert(self.info.music.clone(), tokio::fs::read(music).await?);
            }
            if let Some(illustration) = &self.illustration {
                res.insert(self.info.illustration.clone(), tokio::fs::read(illustration).await?);
            }
        }
        Ok(res)
    }

    pub fn is_active(&self) -> bool {
        self.inputs.is_active()
    }

    pub fn touch(&mut self, touch: &Touch, t: f32) {
        self.inputs.touch(touch);
        let info = &self.info;
        self.inputs.name.activate(touch, t, &info.name);
        self.inputs.charter.activate(touch, t, &info.charter);
        self.inputs.composer.activate(touch, t, &info.composer);
        self.inputs.illustrator.activate(touch, t, &info.illustrator);
        self.inputs.level.activate(touch, t, &info.level);
        let preview = format!("{} - {}", format_time(info.preview_start), format_time(info.preview_end.unwrap_or(info.preview_start + 15.)));
        self.inputs.preview_time.activate(touch, t, &preview);
        self.inputs.offset.activate(touch, t, &format!("{:.3}", info.offset));
        self.inputs.aspect_ratio.activate(touch, t, &format!("{:.5}", info.aspect_ratio));
        self.inputs.score_total.activate(touch, t, &format!("{}", info.score_total));
        self.inputs.line_length.activate(touch, t, &format!("{}", info.line_length));
        self.inputs.hold_particle_interval_ratio.activate(touch, t, &format!("{}", info.hold_particle_interval_ratio));
        self.inputs.tip.activate(touch, t, info.tip.as_deref().unwrap_or(""));
        self.inputs.intro.activate(touch, t, &info.intro);
    }

    pub fn update(&mut self) {
        if let Some(text) = self.inputs.name.confirm() {
            self.info.name = text;
        }
        if let Some(text) = self.inputs.charter.confirm() {
            self.info.charter = text;
        }
        if let Some(text) = self.inputs.composer.confirm() {
            self.info.composer = text;
        }
        if let Some(text) = self.inputs.illustrator.confirm() {
            self.info.illustrator = text;
        }
        if let Some(text) = self.inputs.level.confirm() {
            self.info.level = text;
        }
        if let Some(text) = self.inputs.preview_time.confirm() {
            match parse_preview(&text) {
                Err(err) => {
                    show_message(err).error();
                }
                Ok((st, en)) => {
                    self.info.preview_start = st;
                    self.info.preview_end = Some(en);
                }
            }
        }
        if let Some(text) = self.inputs.offset.confirm() {
            match text.parse::<f64>() {
                Err(_) => {
                    show_message(tl!("illegal-input")).error();
                }
                Ok(value) => {
                    self.info.offset = value;
                }
            }
        }
        if let Some(text) = self.inputs.aspect_ratio.confirm() {
            match parse_aspect_ratio(&text) {
                None => {
                    show_message(tl!("illegal-input")).error();
                }
                Some(value) => {
                    self.info.aspect_ratio = value;
                }
            }
        }
        if let Some(text) = self.inputs.score_total.confirm() {
            match text.parse::<u32>() {
                Err(_) => {
                    show_message(tl!("illegal-input")).error();
                }
                Ok(value) => {
                    self.info.score_total = value;
                }
            }
        }
        if let Some(text) = self.inputs.line_length.confirm() {
            match text.parse::<f32>() {
                Err(_) => {
                    show_message(tl!("illegal-input")).error();
                }
                Ok(value) => {
                    self.info.line_length = value;
                }
            }
        }
        if let Some(text) = self.inputs.hold_particle_interval_ratio.confirm() {
            match text.parse::<f32>() {
                Err(_) => {
                    show_message(tl!("illegal-input")).error();
                }
                Ok(value) => {
                    self.info.hold_particle_interval_ratio = value;
                }
            }
        }
        if let Some(text) = self.inputs.tip.confirm() {
            self.info.tip = if text.is_empty() { None } else { Some(text) };
        }
        if let Some(text) = self.inputs.intro.confirm() {
            self.info.intro = text;
        }
        self.inputs.update();
    }
}

fn format_time(t: f64) -> String {
    use std::fmt::Write;
    let mut s = String::new();
    let it = t as u32;
    write!(&mut s, "{:02}:{:02}:{:05.2}", it / 3600, (it / 60) % 60, t % 60.).unwrap();
    s
}

fn parse_preview(string: &str) -> Result<(f64, f64), Cow<'static, str>> {
    let (st, en) = string.split_once(['-', '—']).ok_or_else(|| tl!("illegal-input"))?;
    let st = parse_time(st.trim()).ok_or_else(|| tl!("invalid time"))?;
    let en = parse_time(en.trim()).ok_or_else(|| tl!("invalid time"))?;
    if st + 1. > en {
        return Err(tl!("preview-too-short"));
    }
    if st + 20. < en {
        return Err(tl!("preview-too-long"));
    }
    Ok((st, en))
}

fn parse_aspect_ratio(string: &str) -> Option<f32> {
    let value = if let Some((w, h)) = string.split_once([':', '：']) {
        w.trim().parse::<f32>().ok()? / h.trim().parse::<f32>().ok()?
    } else {
        string.parse().ok()?
    };
    (value.is_finite() && value > 0.0).then_some(value)
}

fn input_row(ui: &mut Ui, input: &mut InlineInputBtn, label: Cow<'static, str>, value: &str, len: f32, t: f32) -> Rect {
    let r = ui.text(Cow::clone(&label)).anchor(1., 0.).size(0.47).draw();
    let lf = r.x;
    let r = Rect::new(0.02, r.y - 0.01, len, r.h + 0.02);
    input.render(ui, r, t, WHITE, &label, value);
    Rect::new(lf, r.y, r.right() - lf, r.h)
}

pub fn render_chart_info(ui: &mut Ui, edit: &mut ChartInfoEdit, width: f32, t: f32) -> (f32, f32) {
    let mut sy = 0.02;
    ui.scope(|ui| {
        let s = 0.01;
        ui.dx(0.01);
        ui.dy(sy);
        macro_rules! dy {
            ($dy:expr) => {{
                let dy = $dy;
                sy += dy;
                ui.dy(dy);
            }};
        }
        let r = ui.text(tl!("edit-chart")).size(0.8).draw();
        dy!(r.h + 0.04);
        let rt = 0.22;
        ui.dx(rt);
        let len = width - rt - 0.04;
        let info = &mut edit.info;
        let inputs = &mut edit.inputs;
        let r = input_row(ui, &mut inputs.name, tl!("chart-name"), &info.name, len, t);
        dy!(r.h + s);
        let r = input_row(ui, &mut inputs.charter, tl!("author"), &info.charter, len, t);
        dy!(r.h + s);
        let r = input_row(ui, &mut inputs.composer, tl!("composer"), &info.composer, len, t);
        dy!(r.h + s);
        let r = input_row(ui, &mut inputs.illustrator, tl!("illustrator"), &info.illustrator, len, t);
        dy!(r.h + s + 0.02);

        let r = input_row(ui, &mut inputs.level, tl!("level-displayed"), &info.level, len, t);
        dy!(r.h + s);

        ui.dx(-rt);
        let r = ui.slider(tl!("diff"), 0.0..20.0, 0.1, &mut info.difficulty, Some(width - 0.2));
        dy!(r.h + s + 0.01);
        ui.dx(rt);

        let preview = format!("{} - {}", format_time(info.preview_start), format_time(info.preview_end.unwrap_or(info.preview_start + 15.)));
        let r = input_row(ui, &mut inputs.preview_time, tl!("preview-time"), &preview, len, t);
        dy!(r.h + s);
        dy!(ui.scope(|ui| {
            ui.text(tl!("ps")).anchor(1., 0.).size(0.35).draw();
            ui.text(tl!("preview-hint")).pos(0.02, 0.).size(0.35).max_width(len).multiline().draw().h + 0.03
        }));

        let offset = format!("{:.3}", info.offset);
        let r = input_row(ui, &mut inputs.offset, tl!("offset"), &offset, len, t);
        dy!(r.h + s);

        let aspect_ratio = format!("{:.5}", info.aspect_ratio);
        let r = input_row(ui, &mut inputs.aspect_ratio, tl!("aspect-ratio"), &aspect_ratio, len, t);
        dy!(r.h + s);
        dy!(ui.scope(|ui| {
            ui.text(tl!("ps")).anchor(1., 0.).size(0.35).draw();
            ui.text(tl!("aspect-hint")).pos(0.02, 0.).size(0.35).max_width(len).multiline().draw().h + 0.03
        }));

        let score_total = format!("{}", info.score_total);
        let r = input_row(ui, &mut inputs.score_total, tl!("score-total"), &score_total, len, t);
        dy!(r.h + s);

        let line_length = format!("{}", info.line_length);
        let r = input_row(ui, &mut inputs.line_length, tl!("line-length"), &line_length, len, t);
        dy!(r.h + s);

        let hold_particle_interval_ratio = format!("{}", info.hold_particle_interval_ratio);
        let r = input_row(
            ui,
            &mut inputs.hold_particle_interval_ratio,
            tl!("hold-particle-interval-ratio"),
            &hold_particle_interval_ratio,
            len,
            t,
        );
        dy!(r.h + s);

        ui.dx(0.01);
        let r = ui.checkbox(tl!("force-aspect-ratio"), &mut info.force_aspect_ratio);
        dy!(r.h + s);
        let r = ui.checkbox(tl!("hold-partial-cover"), &mut info.hold_partial_cover);
        dy!(r.h + s);
        let r = ui.checkbox(tl!("negative-length-hold"), &mut info.negative_length_hold);
        dy!(r.h + s);
        let r = ui.checkbox(tl!("note-uniform-scale"), &mut info.note_uniform_scale);
        dy!(r.h + s);
        let r = ui.checkbox(tl!("fold-animation"), &mut info.fold_animation);
        dy!(r.h + s);
        ui.dx(-0.01);

        ui.dx(-rt);
        let r = ui.slider(tl!("dim"), 0.0..1.0, 0.05, &mut info.background_dim, Some(width - 0.2));
        dy!(r.h + s + 0.01);
        ui.dx(rt);

        #[cfg(not(target_arch = "wasm32"))]
        {
            use crate::scene::{request_file, return_file, take_file};
            let mut choose_file = |id: &str, label: Cow<'static, str>, value: &str| {
                let r = ui.text(label).size(0.47).anchor(1., 0.).draw();
                let r = Rect::new(0.02, r.y - 0.01, len, r.h + 0.02);
                if ui.button(id, r, value) {
                    request_file(id);
                }
                dy!(r.h + s);
            };
            choose_file("chart", tl!("chart-file"), &info.chart);
            choose_file("music", tl!("music-file"), &info.music);
            choose_file("illustration", tl!("illu-file"), &info.illustration);
            if let Some((id, file)) = take_file() {
                match id.as_str() {
                    "chart" => {
                        edit.chart = Some(file);
                    }
                    "music" => {
                        edit.music = Some(file);
                    }
                    "illustration" => {
                        edit.illustration = Some(file);
                    }
                    _ => return_file(id, file),
                }
            }
        }

        let r = input_row(ui, &mut inputs.tip, tl!("tip"), info.tip.as_deref().unwrap_or(""), len, t);
        dy!(r.h + s);

        input_row(ui, &mut inputs.intro, tl!("intro"), &info.intro, len, t);
        ui.dx(-0.02);
    });
    (width, sy)
}
