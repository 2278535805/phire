use super::{
    chart::ChartSettings, BpmList, CtrlObject, JudgeLine, Matrix, Object, Point, Resource, Vector
};
use crate::{
    core::{Anim, HEIGHT_RATIO}, ext::{parse_alpha}, judge::JudgeStatus, parse::RPE_HEIGHT, ui::Ui
};


use macroquad::prelude::*;
pub use crate::{
    judge::HitSound,
};

const FADEOUT_TIME: f64 = 0.16;
const BAD_TIME: f64 = 0.5;

#[derive(Clone, Debug)]
pub enum NoteKind {
    Click,
    Hold { end_time: f64, end_height: f64, end_speed: Option<f64> },
    Flick,
    Drag,
}

impl NoteKind {
    pub fn order(&self) -> i8 {
        match self {
            Self::Hold { .. } => 0,
            Self::Drag => 1,
            Self::Click => 2,
            Self::Flick => 3,
        }
    }
}

pub struct Note {
    pub object: Object,
    pub kind: NoteKind,
    pub hitsound: HitSound,
    pub time: f64,
    pub height: f64,
    pub speed: f64,

    pub above: bool,
    pub multiple_hint: bool,
    pub fake: bool,
    pub judge: JudgeStatus,
    pub judge_scale: f64,
    pub color: Anim<Color>,
    pub hit_fx_color: Anim<Color>,
    pub protected: bool,
}

unsafe impl Sync for Note {}
unsafe impl Send for Note {}

pub struct RenderConfig<'a> {
    pub settings: &'a ChartSettings,
    pub ctrl_obj: &'a mut CtrlObject,
    pub line_height: f64,
    pub appear_before: f64,
    pub invisible_time: f64,
    pub draw_below: bool,
    pub incline_sin: f32,
}

fn draw_tex(res: &Resource, texture: Texture2D, order: i8, x: f32, y: f32, color: Color, mut params: DrawTextureParams, clip: bool) {
    let Vec2 { x: w, y: h } = params.dest_size.unwrap();
    if h < 0. {
        return;
    }
    let mut p = [Point::new(x, y), Point::new(x + w, y), Point::new(x + w, y + h), Point::new(x, y + h)];
    if clip {
        if y + h <= 0. {
            return;
        }
        if y <= 0. {
            let r = -y / (y + h);
            p[0].y = 0.;
            p[1].y = 0.;
            let mut source = params.source.unwrap_or_else(|| Rect::new(0., 0., 1., 1.));
            source.y += source.h * r;
            params.source = Some(source);
        }
    }
    params.flip_y = true;
    draw_tex_pts(res, texture, order, p, color, params);
}
fn draw_tex_pts(res: &Resource, texture: Texture2D, order: i8, p: [Point; 4], color: Color, params: DrawTextureParams) {
    let mut p = p.map(|it| res.world_to_screen(it));
    if p[0].x.min(p[1].x.min(p[2].x.min(p[3].x))) > 1. / res.config.chart_ratio
        || p[0].x.max(p[1].x.max(p[2].x.max(p[3].x))) < -1. / res.config.chart_ratio
        || p[0].y.min(p[1].y.min(p[2].y.min(p[3].y))) > 1. / res.config.chart_ratio
        || p[0].y.max(p[1].y.max(p[2].y.max(p[3].y))) < -1. / res.config.chart_ratio
    {
        return;
    }
    let Rect { x: sx, y: sy, w: sw, h: sh } = params.source.unwrap_or(Rect { x: 0., y: 0., w: 1., h: 1. });

    if params.flip_x {
        p.swap(0, 1);
        p.swap(2, 3);
    }
    if params.flip_y {
        p.swap(0, 3);
        p.swap(1, 2);
    }

    #[rustfmt::skip]
    let vertices = [
        Vertex::new(p[0].x, p[0].y, 0., sx     , sy     , color),
        Vertex::new(p[1].x, p[1].y, 0., sx + sw, sy     , color),
        Vertex::new(p[2].x, p[2].y, 0., sx + sw, sy + sh, color),
        Vertex::new(p[3].x, p[3].y, 0., sx     , sy + sh, color),
    ];
    res.note_buffer
        .borrow_mut()
        .push((order, texture.raw_miniquad_texture_handle().gl_internal_id()), vertices);
}

fn draw_center(res: &Resource, tex: Texture2D, order: i8, scale: f32, color: Color) {
    let hf = vec2(scale, tex.height() * scale / tex.width());
    draw_tex(
        res,
        tex,
        order,
        -hf.x,
        -hf.y,
        color,
        DrawTextureParams {
            dest_size: Some(hf * 2.),
            ..Default::default()
        },
        false,
    );
}

impl Note {
    pub fn rotation(&self, line: &JudgeLine) -> f32 {
        line.object.rotation.now() + if self.above { 0. } else { 180. }
    }

    pub fn update(&mut self, res: &mut Resource, parent_rot: f32, parent_tr: &Matrix, ctrl_obj: &mut CtrlObject, line_height: f64, bpm_list: &mut BpmList, index: usize) {
        if self.time < res.config.play_start_time || res.disable_hit_fx {
            return;
        }
        self.object.set_time(res.time);
        let color = if let JudgeStatus::Hold(perfect, ref mut at, ..) = self.judge {
            if res.time >= *at {
                let beat = 30. / bpm_list.now_bpm(
                    if bpm_list.per_line_bpm_storage { index as f64 } else { self.time }
                );
                //println!("{} {} {}", index, bpm_list.now_bpm(index as f32), beat);
                *at = res.time + beat * res.info.hold_particle_interval_ratio as f64 / res.config.speed as f64; //HOLD_PARTICLE_INTERVAL
                Some(if let Some(color) = self.hit_fx_color.now_opt() {
                    color
                } else if perfect && !res.config.all_good && !res.config.all_bad {
                    res.res_pack.info.fx_perfect()
                } else {
                    res.res_pack.info.fx_good()
                })
            } else {
                None
            }
        } else {
            None
        };

        if let Some(color) = color {
            self.init_ctrl_obj(ctrl_obj, line_height);
            let rotation = if self.above { 0. } else { 180. };
            res.with_model(parent_tr * self.now_transform(res, ctrl_obj, 0., 0., false, false), |res| {
                res.emit_at_origin(parent_rot + rotation, color)
            });
        }
    }
    

    pub fn dead(&self) -> bool {
        (!matches!(self.kind, NoteKind::Hold { .. }) || matches!(self.judge, JudgeStatus::Judged)) && self.object.dead()
        // && self.ctrl_obj.dead()
    }

    fn init_ctrl_obj(&self, ctrl_obj: &mut CtrlObject, line_height: f64) {
        ctrl_obj.set_height((self.height - line_height + self.object.translation.1.now() as f64 / self.speed) * RPE_HEIGHT as f64 / 2.);
    }

    pub fn now_transform(&self, res: &Resource, ctrl_obj: &CtrlObject, base: f32, incline_sin: f32, can_scale_x: bool, can_scale_y: bool) -> Matrix {
        let incline_val = 1. - incline_sin * (base * res.aspect_ratio + self.object.translation.1.now()) * RPE_HEIGHT / 2. / 360.;
        let mut tr = self.object.now_translation(res);
        tr.x *= incline_val * ctrl_obj.pos.now_opt().unwrap_or(1.);
        tr.y += base;
        let mut scale = self.object.scale.now_with_def(1.0, 1.0);
        if !can_scale_x {
            scale.x = 1.0;
        };
        scale.x *= ctrl_obj.size.now_opt().unwrap_or(1.0);
        if !res.info.note_uniform_scale || !can_scale_y {
            scale.y = 1.0;
        };
        scale.y *= ctrl_obj.size.now_opt().unwrap_or(1.0);
        self.object.now_rotation().append_nonuniform_scaling(&scale).append_translation(&tr)
    }

    pub fn render(&self, ui: &mut Ui, res: &mut Resource, config: &mut RenderConfig, bpm_list: &mut BpmList, line_set_debug_alpha: bool, line_id: usize, height_above: f64, height_below: f64) {
        if config.appear_before.is_finite() {
        //if config.appear_before.is_finite() && !matches!(self.kind, NoteKind::Hold { .. }) {
            let beat = bpm_list.beat(self.time);
            let time = bpm_list.time_beats(beat - config.appear_before);
            if time > res.time {
                return;
            }
        }

        let ctrl_obj = &mut config.ctrl_obj;
        self.init_ctrl_obj(ctrl_obj, config.line_height);
        let mut color = self.color.now_opt().unwrap_or(WHITE);
        let alpha = self.object.now_alpha().max(0.);
        color.a = parse_alpha(color.a * alpha, 1.0, 0.2, res.config.chart_debug_note > 0.);

        if config.invisible_time.is_finite() && self.time - config.invisible_time < res.time {
            if res.config.chart_debug_note > 0. {
                color.a *= 0.2;
            } else {
                return;
            }
        }

        let aspect_ratio = res.aspect_ratio as f64;
        let spd = self.speed * ctrl_obj.y.now_opt().unwrap_or(1.) as f64;
        let line_height = config.line_height / aspect_ratio * spd;
        let height = self.height / aspect_ratio * spd;
        let base = height - line_height;

        let cover_base = if !res.info.hold_partial_cover {
            height + self.object.translation.1.now() as f64 / aspect_ratio - line_height
        } else {
            match self.kind {
                NoteKind::Hold { end_time: _,  end_height, end_speed: _ } => {
                    let end_height = end_height / aspect_ratio;
                    end_height + self.object.translation.1.now() as f64 / aspect_ratio - line_height
                }
                _ => {
                    height + self.object.translation.1.now() as f64 / aspect_ratio - line_height
                }
            }
        };

        if res.config.alpha_tint {
            if color.a <= 0.5 {
                color.r *= 0.6;
                color.g *= 0.8;
                color.b *= 1.0;
            } else if color.a < 1.0 {
                color.r *= 1.0;
                color.g *= 0.7;
                color.b *= 0.9;
            }
            color.a = res.alpha;
        } else {
            color.a *= parse_alpha(ctrl_obj.alpha.now_opt().unwrap_or(1.), res.alpha, 0.2, res.config.chart_debug_note > 0.);
        }

        // && ((res.time - FADEOUT_TIME >= self.time) || (self.fake && res.time >= self.time) || (self.time > res.time && base <= -1e-5))
        if !config.draw_below
            && ((res.time - FADEOUT_TIME >= self.time && !matches!(self.kind, NoteKind::Hold { .. })) || (self.time > res.time && cover_base <= -0.001))
            // && self.speed != 0.
        {
            if res.config.chart_debug_note > 0. {
                color.a *= 0.2;
            } else {
                return;
            }
        }
        if line_set_debug_alpha {
            color.a *= 0.4;
        }
        if res.config.fade > 0. {
            let base = base as f32;
            let over = res.config.fade * 0.8;
            if base > res.config.fade {
                return;
            } else if base > over {
                color.a *= (res.config.fade - base) / (res.config.fade - over);
            }
        } else if res.config.fade < 0. {
            let base = base as f32;
            let fade_out = res.config.fade.abs();
            let over = fade_out * 0.8;
            if base < over {
                return;
            } else if base < fade_out {
                color.a *= (base - over) / (fade_out - over);
            }
        }

        let scale = (if res.config.render_double_hint && self.multiple_hint {
            res.res_pack.note_style_mh.click.width() / res.res_pack.note_style.click.width()
        } else {
            1.0
        }) * res.note_width;
        let order = self.kind.order();
        let style = if res.config.render_double_hint && self.multiple_hint {
            &res.res_pack.note_style_mh
        } else {
            &res.res_pack.note_style
        };
        let draw = |res: &mut Resource, tex: Texture2D| {
            let mut color = color;
            if !config.draw_below {
                color.a *= ((self.time - res.time).min(0.) / FADEOUT_TIME + 1.) as f32;
            }
            res.with_model(self.now_transform(res, ctrl_obj, base as f32, config.incline_sin, true, true), |res| {
                if res.config.aggressive_note {
                    let pt = res.world_to_screen(Point::default());
                    let roughly_pos = ((pt.x * 200.0).round() as i32, (pt.y * 200.0).round() as i32);
                    let count = res.note_pos_map.entry(roughly_pos).or_insert(0);
                    if *count < 2 {
                        *count += 1;
                    } else {
                        return;
                    }
                }
                draw_center(res, tex, order, scale, color);
            });
        };
        match self.kind {
            NoteKind::Click => {
                if self.fake && res.time >= self.time { return };
                draw(res, *style.click);
            }
            NoteKind::Hold { end_time, end_height, end_speed } => {
                if self.fake && res.time >= end_time { return };
                res.with_model(self.now_transform(res, ctrl_obj, 0., 0., true, false), |res| {
                    if matches!(self.judge, JudgeStatus::Judged) {
                        // miss
                        color.a *= 0.5;
                    }
                    if res.time >= end_time {
                        return;
                    }

                    let end_height = end_height / aspect_ratio * spd;
                    let time = if res.time >= self.time { res.time } else { self.time };

                    //let clip = !config.draw_below && config.settings.hold_partial_cover;
                    let clip = false;

                    let h = if self.time <= res.time { line_height } else { height };
                    let bottom = h - line_height; //StartY
                    let top = if let Some(end_spd) = end_speed {
                        let end_spd = end_spd * ctrl_obj.y.now_opt().unwrap_or(1.) as f64;
                        if end_spd == 0. {
                            if res.config.chart_debug_note > 0. {
                                color.a *= 0.2;
                            } else {
                                return;
                            }
                        }

                        let hold_height = end_height - height;
                        let hold_line_height = (time - self.time) * end_spd / aspect_ratio / HEIGHT_RATIO;
                        bottom + hold_height - hold_line_height
                    } else {
                        end_height - line_height
                    };

                    let style = if res.config.render_double_hint && self.multiple_hint {
                        &res.res_pack.note_style_mh
                    } else {
                        &res.res_pack.note_style
                    };

                    let tex = &style.hold;
                    let ratio = style.hold_ratio();
                    // body
                    draw_tex(
                        res,
                        **(if res.res_pack.info.hold_repeat {
                            style.hold_body.as_ref().unwrap()
                        } else {
                            tex
                        }),
                        order,
                        -scale,
                        bottom as f32,
                        color,
                        DrawTextureParams {
                            source: Some({
                                if res.res_pack.info.hold_repeat {
                                    let hold_body = style.hold_body.as_ref().unwrap();
                                    let width = hold_body.width();
                                    let height = hold_body.height();
                                    Rect::new(0., 0., 1., (top - bottom) as f32 / scale / 2. * width / height)
                                } else {
                                    style.hold_body_rect()
                                }
                            }),
                            dest_size: Some(vec2(scale * 2., (top - bottom) as f32)),
                            ..Default::default()
                        },
                        clip,
                    );
                    // head
                    if res.time < self.time || res.res_pack.info.hold_keep_head {
                        let r = style.hold_head_rect();
                        let hf = vec2(scale, r.h / r.w * scale * ratio);
                        draw_tex(
                            res,
                            **tex,
                            order,
                            -scale,
                            bottom as f32 - if res.res_pack.info.hold_compact { hf.y } else { hf.y * 2. },
                            color,
                            DrawTextureParams {
                                source: Some(r),
                                dest_size: Some(hf * 2.),
                                ..Default::default()
                            },
                            clip,
                        );
                    }
                    // tail
                    let r = style.hold_tail_rect();
                    let hf = vec2(scale, r.h / r.w * scale * ratio);
                    draw_tex(
                        res,
                        **tex,
                        order,
                        -scale,
                        top as f32 - if res.res_pack.info.hold_compact { hf.y } else { 0. },
                        color,
                        DrawTextureParams {
                            source: Some(r),
                            dest_size: Some(hf * 2.),
                            ..Default::default()
                        },
                        clip,
                    );
                });
            }
            NoteKind::Flick => {
                if self.fake && res.time >= self.time { return };
                draw(res, *style.flick);
            }
            NoteKind::Drag => {
                if self.fake && res.time >= self.time { return };
                draw(res, *style.drag);
            }
        }
        if res.config.chart_debug_note > 0. {
            match self.kind {
                NoteKind::Hold { end_time, end_height, end_speed } => {
                    if cover_base > height_above || res.time >= end_time {
                        return;
                    }
                    let above = if self.above { "" } else { " below" };
                    let fake = if self.fake { " fake" } else { "" };
                    let bottom = if self.time <= res.time { 0. } else { height - line_height };
                    let speed = if self.speed == 1.0 && end_speed.is_none() {
                        String::new()
                    } else {
                        let end_spd = match end_speed {
                            Some(spd) => format!("({})", spd),
                            None => "".to_string(),
                        };
                        format!(" v: {}{}", self.speed, end_spd)
                    };
                    res.with_model(self.now_transform(res, ctrl_obj, bottom as f32, config.incline_sin, false, false), |res: &mut Resource| {
                        res.with_model(Matrix::new_nonuniform_scaling(&Vector::new(1.0, if self.above { -1.0 } else { 1.0 })), |res: &mut Resource| {
                            res.apply_model(|res| {
                                ui.text(format!("[{}] t:{:.2}({:.2}) h:{:.2}({:.2})[{:.2}]\n{}{}{}", line_id, self.time, end_time, self.height, end_height, base, speed, above, fake))
                                    .pos(0., if self.above { res.config.chart_debug_note * 0.2 } else { -res.config.chart_debug_note * 0.2 })
                                    .anchor(0.0, 0.)
                                    .size(res.config.chart_debug_note)
                                    .color(Color::new(1., 1., 1., color.a))
                                    .centered_multiline()
                                    .draw();
                            });
                        });
                    });
                }
                _ => {
                    if cover_base > height_above || cover_base < height_below || res.time >= self.time {
                        return;
                    }
                    let above = if self.above { "" } else { " below" };
                    let fake = if self.fake { " fake" } else { "" };
                    let speed = if self.speed == 1. {
                        String::new()
                    } else {
                        format!(" v: {}", self.speed)
                    };
                    res.with_model(self.now_transform(res, ctrl_obj, base as f32, config.incline_sin, false, false), |res: &mut Resource| {
                        res.with_model(Matrix::new_nonuniform_scaling(&Vector::new(1.0, if self.above { -1.0 } else { 1.0 })), |res: &mut Resource| {
                            res.apply_model(|res| {
                                ui.text(format!("[{}] t:{:.2} h:{:.2}[{:.2}]\n{}{}{}", line_id, self.time, self.height, base, speed, above, fake))
                                    .pos(0., res.config.chart_debug_note * 0.15)
                                    .anchor(0.0, 0.)
                                    .size(res.config.chart_debug_note)
                                    .color(Color::new(1., 1., 1., color.a))
                                    .centered_multiline()
                                    .draw();
                            });
                        });
                    });
                }
            }
        }
    }
}

pub struct BadNote {
    pub time: f64,
    pub kind: NoteKind,
    pub matrix: Matrix,
}

impl BadNote {
    pub fn render(&self, res: &mut Resource) -> bool {
        if res.time > self.time + BAD_TIME {
            return false;
        }
        res.with_model(self.matrix, |res| {
            let style = &res.res_pack.note_style;
            draw_center(
                res,
                match &self.kind {
                    NoteKind::Click => *style.click,
                    NoteKind::Drag => *style.drag,
                    NoteKind::Flick => *style.flick,
                    _ => unreachable!(),
                },
                self.kind.order(),
                res.note_width,
                Color::new(0.423529, 0.262745, 0.262745, ((self.time - res.time).max(-1.) / BAD_TIME + 1.) as f32),
            );
        });
        true
    }
}
