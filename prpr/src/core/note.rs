use super::{
    chart::ChartSettings, BpmList, CtrlObject, JudgeLine, Matrix, Object, Point, Resource
};
use crate::{
    core::HEIGHT_RATIO, info::ChartFormat, judge::JudgeStatus, parse::RPE_HEIGHT, ui::Ui
};


use macroquad::prelude::*;
use ::rand::{thread_rng, Rng};
pub use crate::{
    judge::HitSound,
};

//const HOLD_PARTICLE_INTERVAL: f32 = 0.15;
const FADEOUT_TIME: f32 = 0.16;
const BAD_TIME: f32 = 0.5;

#[derive(Clone, Debug)]
pub enum NoteKind {
    Click,
    Hold { end_time: f32, end_height: f64, end_speed: f32 },
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
    pub time: f32,
    pub height: f64,
    pub speed: f32,

    pub above: bool,
    pub multiple_hint: bool,
    pub fake: bool,
    pub judge: JudgeStatus,
    pub attr: bool,
}

unsafe impl Sync for Note {}
unsafe impl Send for Note {}

pub struct RenderConfig<'a> {
    pub settings: &'a ChartSettings,
    pub ctrl_obj: &'a mut CtrlObject,
    pub line_height: f64,
    pub appear_before: f32,
    pub invisible_time: f32,
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

fn random_rotate() -> f32 {
    let mut rng = thread_rng();
    let rotation_degrees: f32 = match rng.gen_range(0..4) {
        0 => 0.,
        1 => 90.,
        2 => 180.,
        3 => 270.,
        _ => 0.,
    };
    rotation_degrees
}

impl Note {
    pub fn rotation(&self, line: &JudgeLine) -> f32 {
        line.object.rotation.now() + if self.above { 0. } else { 180. }
    }

    pub fn plain(&self) -> bool {
        !self.fake && !matches!(self.kind, NoteKind::Hold { .. }) && self.object.translation.1.keyframes.len() <= 1
        // && self.ctrl_obj.is_default()
    }

    pub fn update(&mut self, res: &mut Resource, parent_rot: f32, parent_tr: &Matrix, ctrl_obj: &mut CtrlObject, line_height: f64, bpm_list: &mut BpmList, index: usize) {
        self.object.set_time(res.time);
        //let mut _immediate_particle = false;
        let color = if let JudgeStatus::Hold(perfect, ref mut at, ..) = self.judge {
            if res.time >= *at {
                //_immediate_particle = true;
                let beat = 30. / bpm_list.now_bpm(
                    if matches!(res.chart_format, ChartFormat::Pgr) { index as f32 } else { self.time }
                );
                //println!("{} {} {}", index, bpm_list.now_bpm(index as f32), beat);
                *at = res.time + beat / res.config.speed; //HOLD_PARTICLE_INTERVAL
                Some(if perfect && !res.config.all_good && !res.config.all_bad {
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
            let rotation = if res.config.chart_debug { 
                if self.above { 0. } else { 180. } } 
                else { random_rotate() 
            };
            res.with_model(parent_tr * self.now_transform(res, ctrl_obj, 0., 0.), |res| {
                res.emit_at_origin(parent_rot + rotation, color)
            });
        }
    }
    

    pub fn dead(&self) -> bool {
        (!matches!(self.kind, NoteKind::Hold { .. }) || matches!(self.judge, JudgeStatus::Judged)) && self.object.dead()
        // && self.ctrl_obj.dead()
    }

    fn init_ctrl_obj(&self, ctrl_obj: &mut CtrlObject, line_height: f64) {
        ctrl_obj.set_height(((self.height - line_height) as f32 + self.object.translation.1.now() / self.speed) * RPE_HEIGHT / 2.);
    }

    pub fn now_transform(&self, res: &Resource, ctrl_obj: &CtrlObject, base: f32, incline_sin: f32) -> Matrix {
        let incline_val = 1. - incline_sin * (base * res.aspect_ratio + self.object.translation.1.now()) * RPE_HEIGHT / 2. / 360.;
        let mut tr = self.object.now_translation(res);
        tr.x *= incline_val * ctrl_obj.pos.now_opt().unwrap_or(1.);
        tr.y += base;
        let mut scale = self.object.scale.now_with_def(1., 1.);
        scale.x *= ctrl_obj.size.now_opt().unwrap_or(1.);
        self.object.now_rotation().append_nonuniform_scaling(&scale).append_translation(&tr)
    }

    pub fn render(&self, _ui: &mut Ui, res: &mut Resource, config: &mut RenderConfig, bpm_list: &mut BpmList, line_set_debug_alpha: bool) {
        if matches!(self.judge, JudgeStatus::Judged) && !matches!(self.kind, NoteKind::Hold { .. }) {
            return;
        }

        if config.appear_before.is_finite() {
        //if config.appear_before.is_finite() && !matches!(self.kind, NoteKind::Hold { .. }) {
            let beat = bpm_list.beat(self.time);
            let time = bpm_list.time_beats(beat - config.appear_before);
            if time > res.time {
                return;
            }
        }
        
        if config.invisible_time.is_finite() && self.time - config.invisible_time < res.time {
            return;
        }
        let ctrl_obj = &mut config.ctrl_obj;
        self.init_ctrl_obj(ctrl_obj, config.line_height);
        let spd = self.speed * ctrl_obj.y.now_opt().unwrap_or(1.);

        let line_height = config.line_height / res.aspect_ratio as f64 * spd as f64;
        let height = self.height / res.aspect_ratio as f64 * spd as f64;
        let base = (height - line_height) as f32;

        if res.config.aggressive && matches!(res.chart_format, ChartFormat::Pec) && matches!(self.kind, NoteKind::Hold { .. }) {
            let h = if self.time <= res.time { line_height } else { height };
            let bottom = h - line_height;
            if bottom - line_height > 2. / res.config.chart_ratio as f64 {
                return;
            }
        }

        let cover_base = if !config.settings.hold_partial_cover {
            height - line_height
        } else {
            match self.kind {
                NoteKind::Hold { end_time: _,  end_height, end_speed } => {
                    let end_spd = end_speed * ctrl_obj.y.now_opt().unwrap_or(1.);
                    let end_height = end_height / res.aspect_ratio as f64 * end_spd as f64;
                    end_height - line_height
                }
                _ => {
                    height - line_height
                }
            }
        };
        
        let mut color = self.object.now_color();
        color.a *= res.alpha * ctrl_obj.alpha.now_opt().unwrap_or(1.);

        // && ((res.time - FADEOUT_TIME >= self.time) || (self.fake && res.time >= self.time) || (self.time > res.time && base <= -1e-5))
        if !config.draw_below
            && ((res.time - FADEOUT_TIME >= self.time && !matches!(self.kind, NoteKind::Hold { .. })) || (self.time > res.time && cover_base <= -0.001))
            // && self.speed != 0.
        {
            if res.config.chart_debug{
                color.a *= 0.2;
            } else {
                return;
            }
        }
        if line_set_debug_alpha {
            color.a *= 0.4;
        }
        if res.config.fade > 0. {
            let over = res.config.fade * 0.8;
            if base > res.config.fade {
                return;
            } else if base > over {
                color.a *= (res.config.fade - base) / (res.config.fade - over);
            }
        } else if res.config.fade < 0. {
            let fade_out = res.config.fade.abs();
            let over = fade_out * 0.8;
            if base < over {
                return;
            } else if base < fade_out {
                //color.a *= (fade_out - base) / (fade_out - over);
                color.a *= (base - over) / (fade_out - over);
            }
        }

        let scale = (if self.multiple_hint {
            res.res_pack.note_style_mh.click.width() / res.res_pack.note_style.click.width()
        } else {
            1.0
        }) * res.note_width;
        let order = self.kind.order();
        let style = if res.config.double_hint && self.multiple_hint {
            &res.res_pack.note_style_mh
        } else {
            &res.res_pack.note_style
        };
        let draw = |res: &mut Resource, tex: Texture2D| {
            let mut color = color;
            if !config.draw_below {
                color.a *= (self.time - res.time).min(0.) / FADEOUT_TIME + 1.;
            }
            res.with_model(self.now_transform(res, ctrl_obj, base, config.incline_sin), |res| {
                draw_center(res, tex, order, scale, color);
            });
        };
        match self.kind {
            NoteKind::Click => {
                if self.fake && res.time >= self.time {return};
                draw(res, *style.click);
            }
            NoteKind::Hold { end_time, end_height, end_speed } => {
                if self.fake && res.time >= end_time {return};
                res.with_model(self.now_transform(res, ctrl_obj, 0., 0.), |res| {
                    if matches!(self.judge, JudgeStatus::Judged) {
                        // miss
                        color.a *= 0.5;
                    }
                    if res.time >= end_time {
                        return;
                    }
                    let end_spd = end_speed * ctrl_obj.y.now_opt().unwrap_or(1.);
                    if matches!(res.chart_format, ChartFormat::Pgr) && end_spd == 0. {
                        if res.config.chart_debug {
                            color.a *= 0.2;
                        } else {
                            return;
                        }
                    }

                    let end_height = end_height / res.aspect_ratio as f64 * spd as f64;
                    let time = if res.time >= self.time {res.time} else {self.time};

                    //let clip = !config.draw_below && config.settings.hold_partial_cover;
                    let clip = false;

                    let h = if self.time <= res.time { line_height } else { height };
                    let bottom = (h - line_height) as f32; //StartY
                    let top = if matches!(res.chart_format, ChartFormat::Pgr) {
                        let hold_height = (end_height - height) as f32;
                        let hold_line_height = (time - self.time) * end_spd / res.aspect_ratio / HEIGHT_RATIO;
                        bottom + hold_height - hold_line_height
                    } else {
                        (end_height - line_height) as f32
                    };

                    //let max_hold_height = 3. / res.config.chart_ratio / res.aspect_ratio;
                    //let top = if res.config.aggressive && hold_height - hold_line_height >= max_hold_height { bottom + max_hold_height } else { top };

                    //println!("res.time:{:.6}\tend_height:{:.7}\tspd:{}\tend_spd:{:.7}\tline_height:{:.6}\th:{}\tbottom:{:.6}\ttop:{:.6}\thold_height:{} {}", res.time, end_height, spd, end_spd, line_height, h, bottom, top, hold_height, height - h);

                    //if res.time < self.time && bottom < -1e-6 && (!config.settings.hold_partial_cover && !matches!(res.chart_format, ChartFormat::Pgr)) {
                    //    return;
                    //}

                    let style = if res.config.double_hint && self.multiple_hint {
                        &res.res_pack.note_style_mh
                    } else {
                        &res.res_pack.note_style
                    };

                    let tex = &style.hold;
                    let ratio = style.hold_ratio();
                    // body
                    // TODO (end_height - height) is not always total height
                    draw_tex(
                        res,
                        **(if res.res_pack.info.hold_repeat {
                            style.hold_body.as_ref().unwrap()
                        } else {
                            tex
                        }),
                        order,
                        -scale,
                        bottom,
                        color,
                        DrawTextureParams {
                            source: Some({
                                if res.res_pack.info.hold_repeat {
                                    let hold_body = style.hold_body.as_ref().unwrap();
                                    let width = hold_body.width();
                                    let height = hold_body.height();
                                    Rect::new(0., 0., 1., (top - bottom) / scale / 2. * width / height)
                                } else {
                                    style.hold_body_rect()
                                }
                            }),
                            dest_size: Some(vec2(scale * 2., top - bottom)),
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
                            bottom - if res.res_pack.info.hold_compact { hf.y } else { hf.y * 2. },
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
                        top - if res.res_pack.info.hold_compact { hf.y } else { 0. },
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
                if self.fake && res.time >= self.time {return};
                draw(res, *style.flick);
            }
            NoteKind::Drag => {
                if self.fake && res.time >= self.time {return};
                draw(res, *style.drag);
            }
        }
    }
}

pub struct BadNote {
    pub time: f32,
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
                Color::new(0.423529, 0.262745, 0.262745, (self.time - res.time).max(-1.) / BAD_TIME + 1.),
            );
        });
        true
    }
}
