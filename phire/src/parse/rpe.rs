crate::tl_file!("parser" ptl);

use super::{process_lines, RPE_TWEEN_MAP};
use crate::{
    core::{
        Anim, AnimFloat, AnimFloatF64, AnimVector, BezierTween, BpmList, Chart, ChartExtra, ChartSettings, ClampedTween, CtrlObject, EPS, GeneralIntegralTween, GifFrames, HEIGHT_RATIO, HitSoundMap, IntegralClampedTween, IntegralStaticTween, JudgeLine, JudgeLineCache, JudgeLineKind, Keyframe, Note, NoteKind, Object, SpeedIntegralTween, StaticTween, TextData, Triple, TweenFunction, Tweenable, UIElement, Vector
    },
    ext::{NotNanExt, SafeTexture},
    fs::FileSystem,
    judge::{HitSound, JudgeStatus},
    ui::{FontArc, TextPainter},
};
use anyhow::{Context, Result};
use image::{codecs::gif, AnimationDecoder, DynamicImage, ImageError};
use macroquad::prelude::{Color, WHITE};
use rustc_hash::FxHashMap;
use sasa::AudioClip;
use serde::{Deserialize, Serialize};
use std::{cell::RefCell, collections::HashMap, future::IntoFuture, rc::Rc, str::FromStr, time::Duration};
use tracing::debug;

pub const RPE_WIDTH: f32 = 1350.;
pub const RPE_HEIGHT: f32 = 900.;
const SPEED_RATIO: f64 = 10. / 45. / HEIGHT_RATIO;

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RPEBpmItem {
    bpm: f64,
    start_time: Triple,
}

// serde is weird...
fn f32_zero() -> f32 {
    0.
}

fn f32_one() -> f32 {
    1.
}

fn f64_one() -> f64 {
    1.
}

fn i32_one() -> i32 {
    1
}

type BezierMap = FxHashMap<(u16, i16, i16), Rc<dyn TweenFunction>>;

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RPEEvent<T = f32> {
    #[serde(default = "f32_zero")]
    easing_left: f32,
    #[serde(default = "f32_one")]
    easing_right: f32,
    #[serde(default)]
    bezier: u8,
    #[serde(default)]
    bezier_points: [f32; 4],
    #[serde(default = "i32_one")]
    easing_type: i32,
    start: T,
    end: T,
    start_time: Triple,
    end_time: Triple,
}

impl<T> RPEEvent<T> {
    fn bezier_key(&self) -> (u16, i16, i16) {
        let p = &self.bezier_points;
        let int = |p: f32| (p * 100.).round() as i16;
        ((int(p[0]) * 100 + int(p[1])) as u16, int(p[2]), int(p[3]))
    }

    pub fn tween(&self, bezier_map: &BezierMap) -> Rc<dyn TweenFunction> {
        let tween = RPE_TWEEN_MAP.get(self.easing_type.max(1) as usize).copied().unwrap_or(RPE_TWEEN_MAP[0]);
        let left = self.easing_left.clamp(0., 1.);
        let right = self.easing_right.clamp(0., 1.);
        if self.bezier != 0 {
            Rc::clone(&bezier_map[&self.bezier_key()])
        } else if tween <= 2 || (left.abs() < EPS as f32 && (right - 1.0).abs() < EPS as f32) || left >= right {
            StaticTween::get_rc(tween)
        } else {
            Rc::new(ClampedTween::new(tween, left..right))
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RPETextEvent {
    #[serde(default = "f32_zero")]
    easing_left: f32,
    #[serde(default = "f32_one")]
    easing_right: f32,
    #[serde(default)]
    bezier: u8,
    #[serde(default)]
    bezier_points: [f32; 4],
    #[serde(default = "i32_one")]
    easing_type: i32,
    start: String,
    end: String,
    start_time: Triple,
    end_time: Triple,
    #[serde(default)]
    font: Option<String>,
}

impl RPETextEvent {
    fn bezier_key(&self) -> (u16, i16, i16) {
        let p = &self.bezier_points;
        let int = |p: f32| (p * 100.).round() as i16;
        ((int(p[0]) * 100 + int(p[1])) as u16, int(p[2]), int(p[3]))
    }

    pub fn tween(&self, bezier_map: &BezierMap) -> Rc<dyn TweenFunction> {
        let tween = RPE_TWEEN_MAP.get(self.easing_type.max(1) as usize).copied().unwrap_or(RPE_TWEEN_MAP[0]);
        let left = self.easing_left.clamp(0., 1.);
        let right = self.easing_right.clamp(0., 1.);
        if self.bezier != 0 {
            Rc::clone(&bezier_map[&self.bezier_key()])
        } else if tween <= 2 || (left.abs() < EPS as f32 && (right - 1.0).abs() < EPS as f32) || left >= right {
            StaticTween::get_rc(tween)
        } else {
            Rc::new(ClampedTween::new(tween, left..right))
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RPECtrlEvent {
    easing: u8,
    x: f64,
    #[serde(flatten)]
    value: HashMap<String, f32>,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RPEEventLayer {
    alpha_events: Option<Vec<RPEEvent>>,
    move_x_events: Option<Vec<RPEEvent>>,
    move_y_events: Option<Vec<RPEEvent>>,
    rotate_events: Option<Vec<RPEEvent>>,
    speed_events: Option<Vec<RPEEvent<f64>>>,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct RGBColor(u8, u8, u8);

impl Default for RGBColor {
    fn default() -> Self {
        Self(255, 255, 255)
    }
}

impl From<RGBColor> for Color {
    fn from(RGBColor(r, g, b): RGBColor) -> Self {
        Self::from_rgba(r, g, b, 255)
    }
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RPEExtendedEvents {
    color_events: Option<Vec<RPEEvent<RGBColor>>>,
    text_events: Option<Vec<RPETextEvent>>,
    scale_x_events: Option<Vec<RPEEvent>>,
    scale_y_events: Option<Vec<RPEEvent>>,
    incline_events: Option<Vec<RPEEvent>>,
    paint_events: Option<Vec<RPEEvent>>,
    gif_events: Option<Vec<RPEEvent>>,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RPENote {
    // TODO above == 0? what does that even mean?
    #[serde(rename = "type")]
    kind: u8,
    above: u8,
    start_time: Triple,
    end_time: Triple,
    position_x: f32,
    y_offset: f32,
    alpha: u16,               // some alpha has 256...
    hitsound: Option<String>, // TODO implement this feature
    size: f32,
    speed: f64,
    is_fake: u8,
    visible_time: f64,
    #[serde(default, rename = "tint")]
    color: RGBColor,
    #[serde(rename = "tintHitEffects")]
    hit_fx_color: Option<RGBColor>,
    #[serde(default="f64_one", rename = "judgeArea")]
    judge_scale: f64,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RPEJudgeLine {
    // TODO group
    #[serde(rename = "Name")]
    name: String,
    #[serde(rename = "Texture")]
    texture: String,
    #[serde(rename = "father")]
    parent: Option<isize>,
    #[serde(default, rename = "rotateWithFather")]
    rotate_with_parent: bool,
    anchor: Option<[f32; 2]>,
    #[serde(default="f64_one", rename = "bpmfactor")]
    bpm_factor: f64,
    event_layers: Vec<Option<RPEEventLayer>>,
    extended: Option<RPEExtendedEvents>,
    notes: Option<Vec<RPENote>>,
    is_cover: u8,
    #[serde(default)]
    z_order: i32,
    #[serde(rename = "attachUI")]
    attach_ui: Option<UIElement>,

    #[serde(default)]
    pos_control: Vec<RPECtrlEvent>,
    #[serde(default)]
    size_control: Vec<RPECtrlEvent>,
    #[serde(default)]
    alpha_control: Vec<RPECtrlEvent>,
    #[serde(default)]
    y_control: Vec<RPECtrlEvent>,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RPEMetadata {
    #[serde(rename = "RPEVersion")]
    #[allow(unused)] rpe_version: i32,
    offset: i32,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RPEChart {
    #[serde(rename = "META")]
    meta: RPEMetadata,
    #[serde(rename = "BPMList")]
    bpm_list: Vec<RPEBpmItem>,
    judge_line_list: Vec<RPEJudgeLine>,
}

fn parse_events<T: Tweenable, V: Clone + Into<T>>(
    r: &mut BpmList,
    rpe: &[RPEEvent<V>],
    default: Option<T>,
    bezier_map: &BezierMap,
) -> Result<Anim<T>> {
    let mut kfs = Vec::with_capacity(rpe.len() * 2 + 1);
    if let Some(default) = default {
        if !rpe.is_empty() && rpe[0].start_time.beats() > 0.0 {
            kfs.push(Keyframe::new(0.0, default, 0));
        }
    }
    for e in rpe {
        kfs.push(Keyframe {
            time: r.time(&e.start_time),
            value: e.start.clone().into(),
            tween: e.tween(bezier_map),
        });
        kfs.push(Keyframe::new(r.time(&e.end_time), e.end.clone().into(), 0));
    }
    Ok(Anim::new(kfs))
}

fn parse_text_events(
    r: &mut BpmList,
    rpe: &[RPETextEvent],
    default: Option<TextData>,
    bezier_map: &BezierMap,
    font_cache: &HashMap<String, usize>,
) -> Result<Anim<TextData>> {
    let mut kfs = Vec::with_capacity(rpe.len() * 2 + 1);
    if let Some(default) = default {
        if !rpe.is_empty() && rpe[0].start_time.beats() > 0.0 {
            kfs.push(Keyframe::new(0.0, default, 0));
        }
    }
    for e in rpe {
        let font_id = e.font.as_ref().and_then(|path| font_cache.get(path)).copied();
        kfs.push(Keyframe {
            time: r.time(&e.start_time),
            value: TextData { text: e.start.clone(), font_id },
            tween: e.tween(bezier_map),
        });
        kfs.push(Keyframe::new(r.time(&e.end_time), TextData { text: e.end.clone(), font_id }, 0));
    }
    Ok(Anim::new(kfs))
}

fn speed_linear_tween(start_speed: f64, end_speed: f64) -> Rc<dyn TweenFunction> {
    if (start_speed - end_speed).abs() < EPS {
        StaticTween::get_rc(2)
    } else if start_speed.abs() > end_speed.abs() {
        Rc::new(ClampedTween::new(7 /*quadOut*/, 0.0..(1. - end_speed / start_speed) as f32))
    } else {
        Rc::new(ClampedTween::new(6 /*quadIn*/, (start_speed / end_speed) as f32..1.))
    }
}

fn speed_segment_tween(start_speed: f64, end_speed: f64, tween: Rc<dyn TweenFunction>) -> (Rc<dyn TweenFunction>, f64) {
    let (tween, total) = {
        let int_tween: Rc<dyn TweenFunction> = if let Some(s) = tween.as_any().downcast_ref::<StaticTween>() {
            IntegralStaticTween::get_rc(s.0)
        } else if let Some(s) = tween.as_any().downcast_ref::<ClampedTween>() {
            Rc::new(IntegralClampedTween::new(s.0, s.1.clone()))
        } else {
            Rc::new(GeneralIntegralTween::new(tween))
        };
        SpeedIntegralTween::try_create(int_tween, end_speed - start_speed, start_speed)
    }
    .unwrap_or_else(|| (speed_linear_tween(start_speed, end_speed), (start_speed + end_speed) / 2.));
    (tween, total)
}

fn parse_speed_events(r: &mut BpmList, rpe: &[RPEEventLayer], bezier_map: &BezierMap, max_time: f64) -> Result<AnimFloatF64> {
    let layers: Vec<_> = rpe.iter().filter_map(|it| it.speed_events.as_ref()).collect();
    if layers.is_empty() {
        return Ok(AnimFloatF64::default());
    }
    let mut anis = Vec::new();
    for layer in layers {
        if layer.is_empty() {
            continue;
        }
        let mut events = layer.iter().collect::<Vec<_>>();
        events.sort_by_key(|it| it.start_time.beats().not_nan());

        let mut kfs = vec![Keyframe::new(0.0, 0.0, 2)];
        let mut height = 0f64;
        let mut push_kf = |start_time: f64, end_time: f64, tween: Rc<dyn TweenFunction>, factor: f64| {
            if end_time - start_time <= EPS {
                return;
            }
            if let Some(last) = kfs.last_mut() {
                if (last.time - start_time).abs() < EPS {
                    last.value = height;
                    last.tween = tween;
                } else {
                    kfs.push(Keyframe {
                        time: start_time,
                        value: height,
                        tween,
                    });
                }
            }
            height += factor * (end_time - start_time);
        };

        let mut cursor = 0.0;
        let mut last_speed = 0.0;
        for event in events {
            let start_time = r.time(&event.start_time).max(cursor);
            let end_time = r.time(&event.end_time).max(start_time);
            let start_speed = event.start * SPEED_RATIO;
            let end_speed = event.end * SPEED_RATIO;

            push_kf(cursor, start_time, StaticTween::get_rc(2), last_speed);
            if end_time > start_time + EPS {
                if event.easing_type == 0 {
                    push_kf(start_time, end_time, StaticTween::get_rc(2), start_speed);
                } else if event.easing_type <= 1 {
                    if start_speed * end_speed < 0. {
                        let x = start_speed / (start_speed - end_speed);
                        let mid = f64::tween(&start_time, &end_time, x as f32);
                        for (start_time, end_time, start, end) in [(start_time, mid, start_speed, 0.), (mid, end_time, 0., end_speed)] {
                            let factor = start.midpoint(end);
                            let tween = speed_linear_tween(start, end);
                            push_kf(start_time, end_time, tween, factor);
                        }
                    } else {
                        let factor = start_speed.midpoint(end_speed);
                        let tween = speed_linear_tween(start_speed, end_speed);
                        push_kf(start_time, end_time, tween, factor);
                    }
                } else {
                    let (tween, factor) = speed_segment_tween(start_speed, end_speed, event.tween(bezier_map));
                    push_kf(start_time, end_time, tween, factor);
                }
            }
            cursor = end_time;
            last_speed = end_speed;
        }

        push_kf(cursor, max_time, StaticTween::get_rc(2), last_speed);
        if let Some(last) = kfs.last() {
            if (last.time - max_time).abs() > EPS {
                kfs.push(Keyframe::new(max_time, height, 0));
            }
        }
        anis.push(AnimFloatF64::new(kfs));
    }
    if anis.is_empty() {
        return Ok(AnimFloatF64::default());
    }
    Ok(AnimFloatF64::chain(anis))
}

fn parse_gif_events<V: Clone + Into<f32>>(r: &mut BpmList, rpe: &[RPEEvent<V>], bezier_map: &BezierMap, gif: &GifFrames) -> Result<AnimFloat> {
    let mut kfs = Vec::with_capacity(rpe.len() * 3);
    kfs.push(Keyframe::new(0.0, 0.0, 2));
    let mut next_rep_time: u128 = 0;
    for e in rpe {
        while r.time(&e.start_time) as f32 > next_rep_time as f32 / 1000. {
            kfs.push(Keyframe::new(next_rep_time as f64 / 1000., 1.0, 0));
            kfs.push(Keyframe::new(next_rep_time as f64 / 1000., 0.0, 2));
            next_rep_time += gif.total_time();
        }
        let stop_prog = 1. - (next_rep_time as f32 - r.time(&e.start_time) as f32 * 1000.) / gif.total_time() as f32;
        kfs.push(Keyframe::new(r.time(&e.start_time), stop_prog, 0));
        kfs.push(Keyframe {
            time: r.time(&e.start_time),
            value: e.start.clone().into(),
            tween: e.tween(bezier_map),
        });
        kfs.push(Keyframe::new(r.time(&e.end_time), e.end.clone().into(), 2));
        next_rep_time = (r.time(&e.end_time) as f32 * 1000. + gif.total_time() as f32 * (1. - e.end.clone().into())).round() as u128;
    }

    // TODO maybe a better approach?
    const GIF_MAX_TIME: f32 = 2000.;
    while GIF_MAX_TIME > next_rep_time as f32 / 1000. {
        kfs.push(Keyframe::new(next_rep_time as f64 / 1000., 1.0, 0));
        kfs.push(Keyframe::new(next_rep_time as f64 / 1000., 0.0, 2));
        next_rep_time += gif.total_time();
    }
    Ok(Anim::new(kfs))
}

async fn parse_notes(
    r: &mut BpmList,
    rpe: Vec<RPENote>,
    fs: &mut dyn FileSystem,
    height: &mut AnimFloatF64,
    hitsounds: &mut HitSoundMap,
) -> Result<Vec<Note>> {
    let mut notes = Vec::with_capacity(rpe.len());
    for note in rpe {
        let time: f64 = r.time(&note.start_time);
        height.set_time(time);
        let note_height = height.now();
        let y_offset = note.y_offset * 2. / RPE_HEIGHT * note.speed as f32;
        let kind = match note.kind {
            1 => NoteKind::Click,
            2 => {
                let end_time = r.time(&note.end_time);
                height.set_time(end_time);
                NoteKind::Hold {
                    end_time,
                    end_height: height.now(),
                    end_speed: None,
                }
            }
            3 => NoteKind::Flick,
            4 => NoteKind::Drag,
            _ => ptl!(bail "unknown-note-type", "type" => note.kind),
        };
        let hitsound = match note.hitsound {
            Some(s) => {
                match s.trim() {
                    "tap.mp3" | "tap.ogg" => HitSound::Click,
                    "drag.mp3" | "drag.ogg" => HitSound::Drag,
                    "flick.mp3" | "flick.ogg" => HitSound::Flick,
                    _ => {
                        if hitsounds.get(&s).is_none() {
                            if let Ok(data) = fs.load_file(&s).await {
                                hitsounds.insert(s.clone(), AudioClip::new(data)?);
                            } else {
                                ptl!(bail "hitsound-missing", "name" => s);
                            }
                        }
                        HitSound::Custom(String::from_str(&s)?)
                    }
                }
            }
            None => HitSound::default_from_kind(&kind),
        };
        notes.push(Note {
            object: Object {
                alpha: if note.visible_time >= time {
                    if note.alpha >= 255 {
                        AnimFloat::default()
                    } else {
                        AnimFloat::fixed(note.alpha as f32 / 255.)
                    }
                } else {
                    let alpha = note.alpha.min(255) as f32 / 255.;
                    AnimFloat::new(vec![Keyframe::new(0.0, 0.0, 0), Keyframe::new(time - note.visible_time, alpha, 0)])
                },
                translation: AnimVector(AnimFloat::fixed(note.position_x / (RPE_WIDTH / 2.)), AnimFloat::fixed(y_offset)),
                scale: if note.size == 1.0 {
                    AnimVector::default()
                } else {
                    AnimVector(AnimFloat::fixed(note.size), AnimFloat::fixed(note.size))
                },
                rotation: AnimFloat::default(),
            },
            kind,
            hitsound,
            time,
            height: note_height,
            speed: note.speed,

            above: note.above == 1,
            multiple_hint: false,
            fake: note.is_fake != 0,
            judge: JudgeStatus::NotJudged,
            judge_scale: note.judge_scale,
            color: {
                let color = Color::from(note.color);
                if matches!(color, WHITE) {
                    Anim::default()
                } else {
                    Anim::fixed(color)
                }
            },
            hit_fx_color: {
                if let Some(color) = note.hit_fx_color {
                    Anim::fixed(Color::from(color))
                } else {
                    Anim::default()
                }
            },
            protected: false,
        })
    }
    Ok(notes)
}

fn parse_ctrl_events(rpe: &[RPECtrlEvent], key: &str) -> AnimFloat {
    let vals: Vec<_> = rpe.iter().map(|it| it.value[key]).collect();
    if rpe.is_empty() || (rpe.len() == 2 && rpe[0].easing == 1 && (vals[0] - 1.).abs() < 1e-4) {
        return AnimFloat::default();
    }
    AnimFloat::new(
        rpe.iter()
            .zip(vals)
            .map(|(it, val)| Keyframe::new(it.x, val, RPE_TWEEN_MAP.get(it.easing.max(1) as usize).copied().unwrap_or(RPE_TWEEN_MAP[0])))
            .collect(),
    )
}

async fn parse_judge_line(
    bpm_list: Vec<RPEBpmItem>,
    rpe: RPEJudgeLine,
    max_time: f64,
    fs: &mut dyn FileSystem,
    bezier_map: &BezierMap,
    hitsounds: &mut HitSoundMap,
    font_cache: &mut HashMap<String, usize>,
    fonts: &mut Vec<RefCell<TextPainter>>,
) -> Result<JudgeLine> {
    let mut line_texture_map: FxHashMap<String, SafeTexture> = FxHashMap::default();
    let event_layers: Vec<_> = rpe.event_layers.into_iter().flatten().collect();
    let r = &mut BpmList::new(bpm_list.into_iter().map(|it| (it.start_time.beats(), it.bpm / rpe.bpm_factor)).collect());

    if let Some(extended) = &rpe.extended {
        if let Some(text_events) = &extended.text_events {
            for event in text_events {
                if let Some(font_path) = &event.font {
                    if !font_cache.contains_key(font_path) {
                        let font_data = fs.load_file(font_path).await.with_context(|| format!("failed to load font: {font_path}"))?;
                        let font_arc = FontArc::try_from_vec(font_data).map_err(|err| anyhow::anyhow!("failed to parse font: {err}"))?;
                        let painter = TextPainter::new(font_arc);
                        let id = fonts.len();
                        fonts.push(RefCell::new(painter));
                        font_cache.insert(font_path.clone(), id);
                    }
                }
            }
        }
    }

    fn events_with_factor(
        r: &mut BpmList,
        event_layers: &[RPEEventLayer],
        get: impl Fn(&RPEEventLayer) -> &Option<Vec<RPEEvent>>,
        factor: f32,
        desc: &str,
        bezier_map: &BezierMap,
    ) -> Result<AnimFloat> {
        let anis: Vec<_> = event_layers
            .iter()
            .filter_map(|it| get(it).as_ref().map(|es| parse_events(r, es, None, bezier_map)))
            .collect::<Result<_>>()
            .with_context(|| ptl!("type-events-parse-failed", "type" => desc))?;
        let mut res = AnimFloat::chain(anis);
        if res.is_default() {
            return Ok(AnimFloat::fixed(0.0));
        }
        res.map_value(|v| v * factor);
        Ok(res)
    }
    let mut height = parse_speed_events(r, &event_layers, bezier_map, max_time)?;
    let mut notes = parse_notes(r, rpe.notes.unwrap_or_default(), fs, &mut height, hitsounds).await?;
    let cache = JudgeLineCache::new(&mut notes);
    Ok(JudgeLine {
        object: Object {
            alpha: events_with_factor(r, &event_layers, |it| &it.alpha_events, 1. / 255., "alpha", bezier_map)?,
            rotation: events_with_factor(r, &event_layers, |it| &it.rotate_events, -1., "rotate", bezier_map)?,
            translation: AnimVector(
                events_with_factor(r, &event_layers, |it| &it.move_x_events, 2. / RPE_WIDTH, "move X", bezier_map)?,
                events_with_factor(r, &event_layers, |it| &it.move_y_events, 2. / RPE_HEIGHT, "move Y", bezier_map)?,
            ),
            scale: {
                fn parse(r: &mut BpmList, opt: &Option<Vec<RPEEvent>>, factor: f32, bezier_map: &BezierMap) -> Result<AnimFloat> {
                    let mut res = opt
                        .as_ref()
                        .map(|it| parse_events(r, it, None, bezier_map))
                        .transpose()?
                        .unwrap_or(
                            if factor == 1. {
                                Anim::default()
                            } else {
                                Anim::fixed(1.0)
                            }
                        );
                    res.map_value(|v| v * factor);
                    Ok(res)
                }
                let factor = if rpe.texture == "line.png" { 1. } else { 2. / RPE_WIDTH };
                rpe.extended
                    .as_ref()
                    .map(|e| -> Result<_> {
                        Ok(AnimVector(
                            parse(
                                r,
                                &e.scale_x_events,
                                factor,
                                bezier_map,
                            )?,
                            parse(r, &e.scale_y_events, factor, bezier_map)?,
                        ))
                    })
                    .transpose()?
                    .unwrap_or(AnimVector::fixed(Vector::new(factor, factor)))
            },
        },
        color: if let Some(events) = rpe.extended.as_ref().and_then(|e| e.color_events.as_ref()) {
            parse_events(r, events, Some(WHITE), bezier_map).with_context(|| ptl!("color-events-parse-failed"))?
        } else {
            Anim::default()
        },
        ctrl_obj: RefCell::new(CtrlObject {
            alpha: parse_ctrl_events(&rpe.alpha_control, "alpha"),
            size: parse_ctrl_events(&rpe.size_control, "size"),
            pos: parse_ctrl_events(&rpe.pos_control, "pos"),
            y: parse_ctrl_events(&rpe.y_control, "y"),
        }),
        height,
        incline: if let Some(events) = rpe.extended.as_ref().and_then(|e| e.incline_events.as_ref()) {
            parse_events(r, events, Some(0.), bezier_map).with_context(|| ptl!("incline-events-parse-failed"))?
        } else {
            AnimFloat::default()
        },
        notes,
        kind: if rpe.texture == "line.png" {
            if let Some(events) = rpe.extended.as_ref().and_then(|e| e.paint_events.as_ref()) {
                JudgeLineKind::Paint(
                    parse_events(r, events, Some(-1.), bezier_map).with_context(|| ptl!("paint-events-parse-failed"))?,
                    RefCell::default(),
                )
            } else if let Some(extended) = rpe.extended.as_ref() {
                if let Some(events) = extended.text_events.as_ref() {
                    JudgeLineKind::Text(parse_text_events(r, events, Some(TextData::default()), bezier_map, font_cache).with_context(|| ptl!("text-events-parse-failed"))?)
                } else {
                    JudgeLineKind::Normal
                }
            } else {
                JudgeLineKind::Normal
            }
        } else if let Some(extended) = rpe.extended.as_ref() {
            if let Some(events) = extended.gif_events.as_ref() {
                let data = fs
                    .load_file(&rpe.texture)
                    .await
                    .with_context(|| ptl!("gif-load-failed", "path" => rpe.texture.clone()))?;
                let frames = GifFrames::new(
                    tokio::spawn(async move {
                        let decoder = gif::GifDecoder::new(&data[..])?;
                        debug!("decoding gif");
                        Ok::<std::vec::Vec<_>, ImageError>(decoder.into_frames().collect())
                    })
                    .into_future()
                    .await??
                    .into_iter()
                    .map(|frame| -> (u128, SafeTexture) {
                        let frame = frame.unwrap();
                        let delay: Duration = frame.delay().into();
                        (delay.as_millis(), SafeTexture::from(DynamicImage::ImageRgba8(frame.into_buffer())))
                    })
                    .collect(),
                );
                debug!("gif decoded");
                let events = parse_gif_events(r, events, bezier_map, &frames).with_context(|| ptl!("gif-events-parse-failed"))?;
                JudgeLineKind::TextureGif(events, frames, rpe.texture.clone())
            } else if let Some(texture) = line_texture_map.get(&rpe.texture) {
                JudgeLineKind::Texture(texture.clone(), rpe.texture.clone())
            } else {
                let texture = SafeTexture::from(image::load_from_memory(
                    &fs.load_file(&rpe.texture)
                        .await
                        .with_context(|| ptl!("illustration-load-failed", "path" => rpe.texture.clone()))?,
                )?)
                .with_mipmap();
                line_texture_map.insert(rpe.texture.clone(), texture.clone());
                JudgeLineKind::Texture(
                    texture,
                    rpe.texture.clone(),
                )
            }
        } else if let Some(texture) = line_texture_map.get(&rpe.texture) {
            JudgeLineKind::Texture(texture.clone(), rpe.texture.clone())
        } else {
            let texture = SafeTexture::from(image::load_from_memory(
                &fs.load_file(&rpe.texture)
                    .await
                    .with_context(|| ptl!("illustration-load-failed", "path" => rpe.texture.clone()))?,
            )?)
            .with_mipmap();
            line_texture_map.insert(rpe.texture.clone(), texture.clone());
            JudgeLineKind::Texture(
                texture,
                rpe.texture.clone(),
            )
        },
        parent: {
            let parent = rpe.parent.unwrap_or(-1);
            if parent == -1 {
                None
            } else {
                Some(parent as usize)
            }
        },
        rotate_with_parent: rpe.rotate_with_parent,
        anchor: rpe.anchor.unwrap_or([0.5, 0.5]),
        z_index: rpe.z_order,
        show_below: rpe.is_cover != 1,
        attach_ui: rpe.attach_ui,

        cache,
    })
}

fn add_bezier<T>(map: &mut BezierMap, event: &RPEEvent<T>) {
    if event.bezier != 0 {
        let p = &event.bezier_points;
        let int = |p: f32| (p * 100.).round() as i16;
        map.entry(((int(p[0]) * 100 + int(p[1])) as u16, int(p[2]), int(p[3])))
            .or_insert_with(|| Rc::new(BezierTween::new((p[0], p[1]), (p[2], p[3]))));
    }
}

fn add_text_bezier(map: &mut BezierMap, event: &RPETextEvent) {
    if event.bezier != 0 {
        let p = &event.bezier_points;
        let int = |p: f32| (p * 100.).round() as i16;
        map.entry(((int(p[0]) * 100 + int(p[1])) as u16, int(p[2]), int(p[3])))
            .or_insert_with(|| Rc::new(BezierTween::new((p[0], p[1]), (p[2], p[3]))));
    }
}

macro_rules! process_bezier {
    ($event_layer:expr, $map:expr, $($field:ident),*) => {
        $(
            for event in $event_layer.$field.iter().flatten() {
                add_bezier($map, event);
            }
        )*
    };
}

fn get_bezier_map(rpe: &RPEChart) -> BezierMap {
    let mut map = FxHashMap::default();
    for line in &rpe.judge_line_list {
        for event_layer in line.event_layers.iter().flatten() {
            process_bezier!(event_layer, &mut map, alpha_events, move_x_events, move_y_events, rotate_events);
        }
        if let Some(ext_layer) = &line.extended {
            process_bezier!(ext_layer, &mut map, paint_events, scale_x_events, scale_y_events, gif_events, incline_events, color_events);
            for event in ext_layer.text_events.iter().flatten() {
                add_text_bezier(&mut map, event);
            }
        }
    }
    map
}

pub async fn parse_rpe(source: &str, fs: &mut dyn FileSystem, extra: ChartExtra) -> Result<Chart> {
    let rpe: RPEChart = serde_json::from_str(source).with_context(|| ptl!("json-parse-failed"))?;
    let bezier_map = get_bezier_map(&rpe);
    let bpm_list = rpe.bpm_list;
    let mut r = BpmList::new(bpm_list.clone().into_iter().map(|it| (it.start_time.beats(), it.bpm)).collect());
    fn vec<T>(v: &Option<Vec<T>>) -> impl Iterator<Item = &T> {
        v.iter().flat_map(|it| it.iter())
    }
    let mut hitsounds = FxHashMap::default();
    #[rustfmt::skip]
    let max_time = *rpe
        .judge_line_list
        .iter()
        .map(|line| {
            line.notes.as_ref().map(|notes| {
                notes
                    .iter()
                    .map(|note| r.time(&note.end_time).not_nan())
                    .max()
                    .unwrap_or_default()
            }).unwrap_or_default().max(
                line.event_layers.iter().filter_map(|it| it.as_ref().map(|layer| {
                    vec(&layer.alpha_events)
                        .chain(vec(&layer.move_x_events))
                        .chain(vec(&layer.move_y_events))
                        .chain(vec(&layer.rotate_events))
                        .map(|it| r.time(&it.end_time).not_nan())
                        .max().unwrap_or_default()
                })).max().unwrap_or_default()
            ).max(
                line.extended.as_ref().map(|e| {
                    vec(&e.scale_x_events)
                        .chain(vec(&e.scale_y_events))
                        .map(|it| r.time(&it.end_time).not_nan())
                        .max().unwrap_or_default()
                        .max(vec(&e.text_events).map(|it| r.time(&it.end_time).not_nan()).max().unwrap_or_default())
                }).unwrap_or_default()
            )
        })
        .max().unwrap_or_default() + 1.;
    // don't want to add a whole crate for a mere join_all...
    let mut lines = Vec::with_capacity(rpe.judge_line_list.len());
    let mut font_cache: HashMap<String, usize> = HashMap::new();
    let mut fonts: Vec<RefCell<TextPainter>> = Vec::new();
    for (id, line) in rpe.judge_line_list.into_iter().enumerate() {
        let name = line.name.clone();
        lines.push(
            parse_judge_line(bpm_list.clone(), line, max_time, fs, &bezier_map, &mut hitsounds, &mut font_cache, &mut fonts)
                .await
                .with_context(move || ptl!("judge-line-location-name", "jlid" => id, "name" => name))?,
        );
    }
    process_lines(&mut lines);
    Ok(Chart::new(rpe.meta.offset as f64 / 1000.0, lines, r, ChartSettings::default(), extra, hitsounds, fonts))
}
