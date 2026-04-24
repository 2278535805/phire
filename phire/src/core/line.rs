use super::{chart::ChartSettings, object::CtrlObject, Anim, AnimFloat, BpmList, Matrix, Note, Object, Point, RenderConfig, Resource, Vector};
use crate::{
    config::Mods,
    core::{NoteKind, anim::AnimFloatF64},
    ext::{NotNanExt, SafeTexture, get_viewport, parse_alpha},
    judge::{JudgeStatus, LIMIT_BAD},
    ui::Ui,
};
use macroquad::prelude::*;
use miniquad::{RenderPass, Texture, TextureParams, TextureWrap};
use nalgebra::Rotation2;
use serde::{Deserialize, Serialize};
use std::cell::RefCell;

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
#[repr(u8)]
pub enum UIElement {
    Pause = 1,
    ComboNumber = 2,
    Combo = 3,
    Score = 4,
    Bar = 5,
    Name = 6,
    Level = 7,
}

impl UIElement {
    pub fn from_u8(val: u8) -> Option<Self> {
        Some(match val {
            1 => Self::Pause,
            2 => Self::ComboNumber,
            3 => Self::Combo,
            4 => Self::Score,
            5 => Self::Bar,
            6 => Self::Name,
            7 => Self::Level,
            _ => return None,
        })
    }
}

pub struct GifFrames {
    /// time of each frame in milliseconds
    frames: Vec<(u128, SafeTexture)>,
    /// milliseconds
    total_time: u128,
}

impl GifFrames {
    pub fn new(frames: Vec<(u128, SafeTexture)>) -> Self {
        let total_time = frames.iter().map(|(time, _)| *time).sum();
        Self { frames, total_time }
    }

    pub fn get_time_frame(&self, time: u128) -> &SafeTexture {
        let mut time = time % self.total_time;
        for (t, frame) in &self.frames {
            if time < *t {
                return frame;
            }
            time -= t;
        }
        &self.frames.last().unwrap().1
    }

    pub fn get_prog_frame(&self, prog: f32) -> &SafeTexture {
        let time = (prog * self.total_time as f32) as u128;
        self.get_time_frame(time)
    }

    pub fn total_time(&self) -> u128 {
        self.total_time
    }
}

#[derive(Default)]
pub enum JudgeLineKind {
    #[default]
    Normal,
    Texture(SafeTexture, String),
    TextureGif(Anim<f32>, GifFrames, String),
    Text(Anim<String>),
    Paint(Anim<f32>, RefCell<(Option<RenderPass>, bool)>),
}

#[derive(Clone)]
pub struct JudgeLineCache {
    update_order: Vec<u32>,
    above_indices: Vec<usize>,
    below_indices: Vec<usize>,
}

impl JudgeLineCache {
    pub fn new(notes: &mut Vec<Note>) -> Self {
        notes.sort_by_key(|it| {
            (
                !it.above,
                it.speed.not_nan(),
                (it.height + it.object.translation.1.now() as f64 * it.speed).not_nan(),
            )
        });
        
        let mut res = Self {
            update_order: Vec::new(),
            above_indices: Vec::new(),
            below_indices: Vec::new(),
        };
        res.reset(notes);
        res
    }

    pub(crate) fn reset(&mut self, notes: &mut Vec<Note>) {
        self.update_order = (0..notes.len() as u32).collect();
        self.above_indices.clear();
        self.below_indices.clear();
        let mut index = 0;
        while notes.get(index).map_or(false, |it| it.above) {
            self.above_indices.push(index);
            let speed = notes[index].speed;
            loop {
                index += 1;
                if !notes.get(index).map_or(false, |it| it.above && it.speed == speed) {
                    break;
                }
            }
        }
        while index != notes.len() {
            self.below_indices.push(index);
            let speed = notes[index].speed;
            loop {
                index += 1;
                if !notes.get(index).map_or(false, |it| it.speed == speed) {
                    break;
                }
            }
        }
    }
}

pub struct JudgeLine {
    pub object: Object,
    pub color: Anim<Color>,
    pub ctrl_obj: RefCell<CtrlObject>,
    pub kind: JudgeLineKind,
    pub height: AnimFloatF64,
    pub incline: AnimFloat,
    pub notes: Vec<Note>,
    pub parent: Option<usize>,
    pub rotate_with_parent: bool,
    pub z_index: i32,
    pub show_below: bool,
    pub attach_ui: Option<UIElement>,

    pub cache: JudgeLineCache,
    pub anchor: [f32; 2],
}

unsafe impl Sync for JudgeLine {}
unsafe impl Send for JudgeLine {}

impl JudgeLine {
    pub fn update(&mut self, res: &mut Resource, tr: Matrix, bpm_list: &mut BpmList, index: usize) {
        // self.object.set_time(res.time); // this is done by chart, chart has to calculate transform for us
        let rot = self.object.rotation.now();
        self.height.set_time(res.time);
        let line_height = self.height.now();
        let mut ctrl_obj = self.ctrl_obj.borrow_mut();
        self.cache.update_order.retain(|id| {
            let note = &mut self.notes[*id as usize];
            note.update(res, rot, &tr, &mut ctrl_obj, line_height, bpm_list, index);
            !note.dead()
        });
        drop(ctrl_obj);
        match &mut self.kind {
            JudgeLineKind::Text(anim) => {
                anim.set_time(res.time);
            }
            JudgeLineKind::Paint(anim, ..) => {
                anim.set_time(res.time);
            }
            JudgeLineKind::TextureGif(anim, ..) => {
                anim.set_time(res.time);
            }
            _ => {}
        }
        self.color.set_time(res.time);

        let not_judge = |index: usize| {
            match self.notes[index].kind {
                NoteKind::Hold { end_time, .. } => {
                    matches!(self.notes[index].judge, JudgeStatus::Judged) && res.time > end_time
                },
                _ => {
                    matches!(self.notes[index].judge, JudgeStatus::Judged)
                },
            }
        };
        self.cache.above_indices.retain_mut(|index| {
            while not_judge(*index) {
                if self
                    .notes
                    .get(*index + 1)
                    .map_or(false, |it| it.above && it.speed == self.notes[*index].speed)
                {
                    *index += 1;
                } else {
                    return false;
                }
            }
            true
        });
        self.cache.below_indices.retain_mut(|index| {
            while not_judge(*index) {
                if self
                    .notes
                    .get(*index + 1)
                    .map_or(false, |it| it.speed == self.notes[*index].speed)
                {
                    *index += 1;
                } else {
                    return false;
                }
            }
            true
        });
    }

    pub fn fetch_pos(&self, res: &Resource, lines: &[JudgeLine]) -> Vector {
        if let Some(parent) = self.parent {
            let parent = &lines[parent];
            let parent_translation = parent.fetch_pos(res, lines);
            return parent_translation + Rotation2::new(parent.fetch_rot(lines).to_radians()) * self.object.now_translation(res);
        }
        self.object.now_translation(res)
    }

    pub fn fetch_rot(&self, lines: &[JudgeLine]) -> f32 {
        let mut rot = self.object.rotation.now();
        if self.rotate_with_parent {
            if let Some(parent) = self.parent {
                rot += lines[parent].fetch_rot(lines);
            }
        }
        rot
    }

    pub fn now_transform(&self, res: &Resource, lines: &[JudgeLine]) -> Matrix {
        Rotation2::new(self.fetch_rot(lines).to_radians())
            .to_homogeneous()
            .append_translation(&self.fetch_pos(res, lines))
    }

    pub fn render(&self, ui: &mut Ui, res: &mut Resource, lines: &[JudgeLine], bpm_list: &mut BpmList, settings: &ChartSettings, id: usize) {
        let alpha = self.object.now_alpha();
        let color = self.color.now_opt();
        res.with_model(self.now_transform(res, lines), |res| {
            res.with_model(self.object.now_scale(), |res| {
                res.apply_model(|res| match &self.kind {
                    JudgeLineKind::Normal => {
                        if res.config.render_line {
                            let mut color = color.unwrap_or(res.judge_line_color);
                            color.a = parse_alpha(color.a * alpha.max(0.0), if res.info.fold_animation { 1.0 } else { res.alpha }, 0.15, res.config.chart_debug_line > 0.);
                            if color.a == 0.0 {
                                return;
                            }
                            let len = if res.info.line_reference_y_axis {
                                res.info.line_length() / res.aspect_ratio
                            } else {
                                res.info.line_length()
                            };
                            let thickness = if res.info.line_reference_y_axis {
                                0.0150 / res.aspect_ratio
                            } else {
                                0.0100
                            };
                            draw_line(-len, 0., len, 0., thickness, color);
                        }
                    }
                    JudgeLineKind::Texture(texture, _) => {
                        if res.config.render_line_extra {
                            let mut color = color.unwrap_or(WHITE);
                            if res.time <= 0. && matches!(color, WHITE) { // some image show pure white before play
                                color = BLACK;
                            }
                            color.a = parse_alpha(alpha.max(0.0), res.alpha, 0.15, res.config.chart_debug_line > 0.);
                            if color.a == 0.0 {
                                return;
                            }
                            // let hf = vec2(texture.width() / res.aspect_ratio, texture.height() / res.aspect_ratio);
                            let hf = vec2(texture.width(), texture.height()); // Sync RPE
                            draw_texture_ex(
                                **texture,
                                -hf.x / 2.,
                                -hf.y / 2.,
                                color,
                                DrawTextureParams {
                                    dest_size: Some(hf),
                                    flip_y: true,
                                    pivot: Some(Vec2::new(self.anchor[0], -self.anchor[1] + 1.)),
                                    ..Default::default()
                                },
                            );
                        }
                    }
                    JudgeLineKind::TextureGif(anim, frames, _) => {
                        if res.config.render_line_extra {
                            let t = anim.now_opt().unwrap_or(0.0);
                            let frame = frames.get_prog_frame(t);
                            let mut color = color.unwrap_or(WHITE);
                            color.a = parse_alpha(alpha.max(0.0), res.alpha, 0.15, res.config.chart_debug_line > 0.);
                            if color.a == 0.0 {
                                return;
                            }
                            let hf = vec2(frame.width(), frame.height());
                            draw_texture_ex(
                                **frame,
                                -hf.x / 2.,
                                -hf.y / 2.,
                                color,
                                DrawTextureParams {
                                    dest_size: Some(hf),
                                    flip_y: true,
                                    pivot: Some(Vec2::new(self.anchor[0], -self.anchor[1] + 1.)),
                                    ..Default::default()
                                },
                            );
                        }
                    }
                    JudgeLineKind::Text(anim) => {
                        if res.config.render_line_extra {
                                let mut color = color.unwrap_or(WHITE);
                            color.a = parse_alpha(alpha.max(0.0), res.alpha, 0.15, res.config.chart_debug_line > 0.);
                            if color.a == 0.0 {
                                return;
                            }
                            let now = anim.now();
                            res.apply_model_of(&Matrix::identity().append_nonuniform_scaling(&Vector::new(1., -1.)), |_| {
                                ui.text(&now).pos(0., 0.).anchor(self.anchor[0], -self.anchor[1] + 1.).size(1.).color(color).multiline().draw();
                            });
                        }
                    }
                    JudgeLineKind::Paint(anim, state) => {
                        if res.config.render_line_extra {
                            let mut color = color.unwrap_or(WHITE);
                            color.a = parse_alpha(alpha.max(0.0) * 2.55, res.alpha, 0.15, res.config.chart_debug_line > 0.);
                            let mut gl = unsafe { get_internal_gl() };
                            let mut guard = state.borrow_mut();
                            let vp = get_viewport();
                            let pass = *guard.0.get_or_insert_with(|| {
                                let ctx = &mut gl.quad_context;
                                let tex = Texture::new_render_texture(
                                    ctx,
                                    TextureParams {
                                        width: vp.2 as _,
                                        height: vp.3 as _,
                                        format: miniquad::TextureFormat::RGBA8,
                                        filter: FilterMode::Linear,
                                        wrap: TextureWrap::Clamp,
                                    },
                                );
                                RenderPass::new(ctx, tex, None)
                            });
                            gl.flush();
                            let old_pass = gl.quad_gl.get_active_render_pass();
                            gl.quad_gl.render_pass(Some(pass));
                            gl.quad_gl.viewport(None);
                            let size = anim.now();
                            if size <= 0. {
                                if guard.1 {
                                    clear_background(Color::default());
                                    guard.1 = false;
                                }
                            } else {
                                ui.fill_circle(0., 0., size / vp.2 as f32 * 2., color);
                                guard.1 = true;
                            }
                            gl.flush();
                            gl.quad_gl.render_pass(old_pass);
                            gl.quad_gl.viewport(Some(vp));
                        }
                    }
                })
            });
            if let JudgeLineKind::Paint(_, state) = &self.kind {
                let guard = state.borrow_mut();
                if guard.1 && res.config.render_line_extra {
                    let ctx = unsafe { get_internal_gl() }.quad_context;
                    let tex = guard.0.as_ref().unwrap().texture(ctx);
                    let top = 1. / res.aspect_ratio;
                    draw_texture_ex(
                        Texture2D::from_miniquad_texture(tex),
                        -1.,
                        -top,
                        WHITE,
                        DrawTextureParams {
                            dest_size: Some(vec2(2., top * 2.)),
                            ..Default::default()
                        },
                    );
                }
            }
            let mut config = RenderConfig {
                settings,
                ctrl_obj: &mut self.ctrl_obj.borrow_mut(),
                line_height: self.height.now(),
                appear_before: f64::INFINITY,
                invisible_time: f64::INFINITY,
                draw_below: self.show_below,
                incline_sin: self.incline.now_opt().map(|it| it.to_radians().sin()).unwrap_or_default(),
            };
            if res.config.has_mod(Mods::FADE_OUT) {
                config.invisible_time = LIMIT_BAD;
            }
            let mut line_set_debug_alpha = false;
            if alpha < 0.0 {
                if !settings.pe_alpha_extension {
                    if res.config.chart_debug_note > 0. {
                        line_set_debug_alpha = true;
                    } else {
                        return;
                    }
                }
                let w = (-alpha).floor() as u32;
                match w {
                    1 => {
                        if res.config.chart_debug_note > 0. {
                            line_set_debug_alpha = true;
                        } else {
                            return;
                        }
                    }
                    2 => {
                        config.draw_below = false;
                    }
                    w if (100..1000).contains(&w) => {
                        config.appear_before = (w as f64 - 100.) / 10.;
                    }
                    w if (1000..2000).contains(&w) => {
                        // TODO unsupported
                    }
                    _ => {}
                }
            }
            let (vw, vh) = (1.0 / res.config.chart_ratio, 1.0 / res.aspect_ratio / res.config.chart_ratio);
            let p = [
                res.screen_to_world(Point::new(-vw, -vh)),
                res.screen_to_world(Point::new(-vw, vh)),
                res.screen_to_world(Point::new(vw, -vh)),
                res.screen_to_world(Point::new(vw, vh)),
            ];
            let height_above = p[0].y.max(p[1].y.max(p[2].y.max(p[3].y))) as f64;
            let height_below = p[0].y.min(p[1].y.min(p[2].y.min(p[3].y))) as f64;
            let agg = res.config.aggressive_chart;
            let mut height = self.height.clone();
            let aspect_ratio = res.aspect_ratio as f64;
            if res.config.note_scale > 0. && res.config.render_note {
                for index in &self.cache.above_indices {
                    let speed = self.notes[*index].speed;
                    for note in self.notes[*index..].iter() {
                        if !note.above || speed != note.speed {
                            break;
                        }
                        if matches!(note.judge, JudgeStatus::Judged) && !matches!(note.kind, NoteKind::Hold { .. }) {
                            continue;
                        }
                        if agg {
                            let line_height = match note.kind {
                                NoteKind::Hold { end_time, .. } => {
                                    let time = if res.time < end_time {
                                        res.time.min(note.time)
                                    } else {
                                        res.time
                                    };
                                    height.set_time(time);
                                    height.now()
                                }
                                _ => {
                                    config.line_height
                                }
                            };
                            let note_height = ((note.height - line_height) * speed + note.object.translation.1.now() as f64) / aspect_ratio;
                            match note.kind {   
                                NoteKind::Hold { end_height, .. } => {
                                    let end_height = ((end_height - line_height) * speed + note.object.translation.1.now() as f64) / aspect_ratio;
                                    if end_height < height_below {
                                        continue;
                                    }
                                    
                                },
                                _ => {
                                    if note_height < height_below {
                                        continue;
                                    }
                                }
                            }
                            if note_height > height_above {
                                break;
                            }
                        }
                        note.render(ui, res, &mut config, bpm_list, line_set_debug_alpha, id, height_above);
                    }
                }

                res.with_model(Matrix::identity().append_nonuniform_scaling(&Vector::new(1.0, -1.0)), |res| {
                    for index in &self.cache.below_indices {
                        let speed = self.notes[*index].speed;
                        for note in self.notes[*index..].iter() {
                            if speed != note.speed {
                                break;
                            }
                            if matches!(note.judge, JudgeStatus::Judged) && !matches!(note.kind, NoteKind::Hold { .. }) {
                                continue;
                            }
                            if agg {
                                let line_height = match note.kind {
                                    NoteKind::Hold { end_time, .. } => {
                                        let time = if res.time < end_time {
                                            res.time.min(note.time)
                                        } else {
                                            res.time
                                        };
                                        height.set_time(time);
                                        height.now()
                                    }
                                    _ => {
                                        config.line_height
                                    }
                                };
                                let note_height = ((note.height - line_height) * speed + note.object.translation.1.now() as f64) / aspect_ratio;
                                match note.kind {   
                                    NoteKind::Hold { end_height, .. } => {
                                        let end_height = ((end_height - line_height) * speed + note.object.translation.1.now() as f64) / aspect_ratio;
                                        if end_height < -height_above {
                                            continue;
                                        }
                                        
                                    },
                                    _ => {
                                        if note_height < -height_above {
                                            continue;
                                        }
                                    }
                                }
                                if note_height > -height_below {
                                    break;
                                }
                            }
                            note.render(ui, res, &mut config, bpm_list, line_set_debug_alpha, id, -height_below);
                        }
                    }
                });
            }
            if res.config.chart_debug_line > 0. {
                res.with_model(Matrix::identity().append_nonuniform_scaling(&Vector::new(1.0, -1.0)), |res| {
                    res.apply_model(|res| {
                        let kind = match &self.kind {
                            JudgeLineKind::Normal => {
                                if !res.config.render_line { return };
                                String::new()
                            },
                            JudgeLineKind::Text(text) => {
                                if !res.config.render_line_extra { return };
                                format!(" text:{}", text.now())
                            },
                            JudgeLineKind::Texture(_, name) => {
                                if !res.config.render_line_extra { return };
                                format!(" img:{}", name)
                            },
                            JudgeLineKind::TextureGif(_, frames, name) => {
                                if !res.config.render_line_extra { return };
                                format!(" gif:{}/{}", name, frames.total_time())
                            },
                            JudgeLineKind::Paint(_, _) => {
                                if !res.config.render_line_extra { return };
                                format!(" paint")
                            },
                        };

                        let parent = if let Some(parent) = self.parent {
                            format!("({})", parent)
                        } else {
                            String::new()
                        };
                        let line_height_ulp_in_f32 = {
                            if !config.line_height.is_nan() & !config.line_height.is_infinite() {
                                f32::EPSILON * config.line_height.abs() as f32
                            } else {
                                0.0
                            }
                        };
                        let line_height_ulp_string = {
                                if line_height_ulp_in_f32 > 0.0018518519 {
                                    format!("(Speed too high! ULP: {:.4})", line_height_ulp_in_f32)
                                } else {
                                    String::new()
                                }
                        };
                        let z_index = {
                            if self.z_index == 0 {
                                String::new()
                            } else {
                                format!(" z:{}", self.z_index)
                            }
                        };
                        let attach_ui = {
                            let num = self.attach_ui.map_or(0, |it| it as u8);
                            if num == 0 {
                                String::new()
                            } else {
                                format!(" a_ui:{}", num)
                            }
                        };
                        let anchor = if self.anchor[0] == 0.5 && self.anchor[1] == 0.5 {
                            String::new()
                        } else {
                            format!(" anc:{} {}", self.anchor[0], self.anchor[1])
                        };
                        let color = if line_height_ulp_in_f32 > 0.018518519 { // 10px error in 1080P
                            Color::new(1., 0., 0., parse_alpha(alpha, res.alpha, 0.15, res.config.chart_debug_line > 0.))
                        } else if line_height_ulp_in_f32 > 0.0018518519 { // 1px error in 1080P
                            Color::new(1., 1., 0., parse_alpha(alpha, res.alpha, 0.15, res.config.chart_debug_line > 0.))
                        } else {
                            Color::new(1., 1., 1., parse_alpha(alpha, res.alpha, 0.15, res.config.chart_debug_line > 0.))
                        };
                        ui.text(format!("[{}]{} h:{:.2}{}{}{}{}{}", id, parent, config.line_height, line_height_ulp_string, z_index, attach_ui, anchor, kind))
                        .pos(0., -res.config.chart_debug_line * 0.1)
                        .anchor(0.5, 1.)
                        .size(res.config.chart_debug_line)
                        .color(color)
                        .draw();
                    });
                });
            }
        });
    }
}
