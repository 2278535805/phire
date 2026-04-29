crate::tl_file!("parser" ptl);

use super::{process_lines, RPE_TWEEN_MAP};
use crate::{
    core::{
        Anim, AnimFloat, AnimFloatF64, AnimVector, BezierTween, BpmList, Chart, ChartExtra, ChartSettings, ClampedTween, CtrlObject, EPS, GifFrames, HEIGHT_RATIO, HitSoundMap, JudgeLine, JudgeLineCache, JudgeLineKind, Keyframe, Note, NoteKind, Object, StaticTween, Triple, TweenFunction, Tweenable, UIElement
    },
    ext::{NotNanExt, SafeTexture},
    fs::FileSystem,
    judge::{HitSound, JudgeStatus}
};
use anyhow::{Context, Result};
use image::{codecs::gif, AnimationDecoder, DynamicImage, ImageError};
use macroquad::prelude::{Color, WHITE};
use ordered_float::NotNan;
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

fn i32_one() -> i32 {
    1
}

fn f64_one() -> f64 {
    1.
}

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
    easing_type: i32,
    start: T,
    end: T,
    start_time: Triple,
    end_time: Triple,
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
pub struct RPESpeedEvent {
    start_time: Triple,
    end_time: Triple,
    start: f64,
    end: f64,
    #[serde(default = "f32_zero")]
    easing_left: f32,
    #[serde(default = "f32_one")]
    easing_right: f32,
    #[serde(default = "i32_one")]
    easing_type: i32,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RPEEventLayer {
    alpha_events: Option<Vec<RPEEvent>>,
    move_x_events: Option<Vec<RPEEvent>>,
    move_y_events: Option<Vec<RPEEvent>>,
    rotate_events: Option<Vec<RPEEvent>>,
    speed_events: Option<Vec<RPESpeedEvent>>,
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
    text_events: Option<Vec<RPEEvent<String>>>,
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

type BezierMap = FxHashMap<(u16, i16, i16), Rc<dyn TweenFunction>>;

fn bezier_key<T>(event: &RPEEvent<T>) -> (u16, i16, i16) {
    let p = &event.bezier_points;
    let int = |p: f32| (p * 100.).round() as i16;
    ((int(p[0]) * 100 + int(p[1])) as u16, int(p[2]), int(p[3]))
}

fn parse_events<T: Tweenable, V: Clone + Into<T>>(
    r: &mut BpmList,
    rpe: &[RPEEvent<V>],
    default: Option<T>,
    bezier_map: &BezierMap,
) -> Result<Anim<T>> {
    let mut kfs = Vec::with_capacity(rpe.len() * 2 + 1);
    if let Some(default) = default {
        if !rpe.is_empty() && rpe[0].start_time.beats() != 0.0 {
            kfs.push(Keyframe::new(0.0, default, 0));
        }
    }
    for e in rpe {
        kfs.push(Keyframe {
            time: r.time(&e.start_time),
            value: e.start.clone().into(),
            tween: {
                let tween = RPE_TWEEN_MAP.get(e.easing_type.max(1) as usize).copied().unwrap_or(RPE_TWEEN_MAP[0]);
                if e.bezier != 0 {
                    Rc::clone(&bezier_map[&bezier_key(e)])
                } else if e.easing_left.abs() < EPS as f32 && (e.easing_right - 1.0).abs() < EPS as f32 {
                    StaticTween::get_rc(tween)
                } else {
                    Rc::new(ClampedTween::new(tween, e.easing_left..e.easing_right))
                }
            },
        });
        kfs.push(Keyframe::new(r.time(&e.end_time), e.end.clone().into(), 0));
    }
    Ok(Anim::new(kfs))
}

fn parse_speed_events(r: &mut BpmList, rpe: &[RPEEventLayer], max_time: f64) -> Result<AnimFloatF64> {
    let rpe: Vec<&Vec<RPESpeedEvent>> = rpe.iter().filter_map(|it| it.speed_events.as_ref()).collect();
    if rpe.is_empty() {
        // TODO or is it?
        return Ok(AnimFloatF64::default());
    };
    let anis: Vec<AnimFloatF64> = rpe
        .into_iter()
        .map(|it| {
            let mut kfs = Vec::with_capacity(it.len() * 2);
            for e in it {
                let start_beats = e.start_time.beats();
                let end_beats = e.end_time.beats();
                let tween = e.easing_type.max(1) as usize;
                let tween_map = {
                    let tween = RPE_TWEEN_MAP.get(tween).copied().unwrap_or(RPE_TWEEN_MAP[0]);
                    if e.easing_left.abs() < EPS as f32 && (e.easing_right - 1.0).abs() < EPS as f32 {
                        StaticTween::get_rc(tween)
                    } else {
                        Rc::new(ClampedTween::new(tween, e.easing_left..e.easing_right))
                    }
                };
                kfs.push(Keyframe::new(r.time_beats(start_beats), e.start, 2));
                if tween > 1 { // wtf rpe
                    debug!("Speed event segmented: {} - {}", e.start_time.display(), e.end_time.display());
                    let mut now_beats = start_beats;
                    while end_beats - now_beats > 0.03125 {
                        now_beats += 0.03125;
                        let t = (now_beats - start_beats) / (end_beats - start_beats);
                        let now = f64::tween(&e.start, &e.end, tween_map.y(t as f32));
                        kfs.push(Keyframe::new(r.time_beats(now_beats), now, 2));
                    }
                }
                kfs.push(Keyframe::new(r.time_beats(end_beats), e.end, 0));
            }
            AnimFloatF64::new(kfs)
        })
        .collect();
    let mut pts: Vec<NotNan<f64>> = anis.iter().flat_map(|it| it.keyframes.iter().map(|it| it.time.not_nan())).collect();
    pts.push(max_time.not_nan());
    pts.sort();
    pts.dedup();
    let mut sani = AnimFloatF64::chain(anis);
    sani.map_value(|v| v * SPEED_RATIO);
    for i in 0..(pts.len() - 1) {
        let now_time = *pts[i];
        let next_time = *pts[i + 1];
        sani.set_time(now_time);
        let speed = sani.now();
        sani.set_time(next_time.next_down());
        let end_speed = sani.now();
        if speed.signum() * end_speed.signum() < 0. {
            pts.push(f64::tween(&now_time, &next_time, (speed / (speed - end_speed)) as f32).not_nan());
        }
    }
    pts.sort();
    pts.dedup();
    let mut kfs = Vec::with_capacity(pts.len());
    let mut height = 0.0;
    for i in 0..(pts.len() - 1) {
        let now_time = *pts[i];
        let next_time = *pts[i + 1];
        sani.set_time(now_time);
        let speed = sani.now();
        // this can affect a lot! do not use end_time...
        // using end_time causes Hold tween (x |-> 0) to be recognized as Linear tween (x |-> x)
        sani.set_time(next_time.next_down());
        let end_speed = sani.now();
        kfs.push(if (speed - end_speed).abs() < EPS {
            Keyframe::new(now_time, height, 2)
        } else if speed.abs() > end_speed.abs() {
            Keyframe {
                time: now_time,
                value: height,
                tween: Rc::new(ClampedTween::new(7 /*quadOut*/, 0.0..(1. - end_speed / speed) as f32)),
            }
        } else {
            Keyframe {
                time: now_time,
                value: height,
                tween: Rc::new(ClampedTween::new(6 /*quadIn*/, (speed / end_speed) as f32..1.)),
            }
        });
        height += (speed + end_speed) * (next_time - now_time) / 2.;
    }
    kfs.push(Keyframe::new(max_time, height, 0));
    Ok(AnimFloatF64::new(kfs))
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
            tween: {
                let tween = RPE_TWEEN_MAP.get(e.easing_type.max(1) as usize).copied().unwrap_or(RPE_TWEEN_MAP[0]);
                if e.bezier != 0 {
                    Rc::clone(&bezier_map[&bezier_key(e)])
                } else if e.easing_left.abs() < EPS as f32 && (e.easing_right - 1.0).abs() < EPS as f32 {
                    StaticTween::get_rc(tween)
                } else {
                    Rc::new(ClampedTween::new(tween, e.easing_left..e.easing_right))
                }
            },
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
            .zip(vals.into_iter())
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
) -> Result<JudgeLine> {
    let mut line_texture_map: FxHashMap<String, SafeTexture> = FxHashMap::default();
    let event_layers: Vec<_> = rpe.event_layers.into_iter().flatten().collect();
    let r = &mut BpmList::new(bpm_list.into_iter().map(|it| (it.start_time.beats(), it.bpm / rpe.bpm_factor)).collect());

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
    let mut height = parse_speed_events(r, &event_layers, max_time)?;
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
                                factor
                                    * if rpe.texture == "line.png"
                                        && rpe
                                            .extended
                                            .as_ref()
                                            .map_or(true, |it| it.text_events.as_ref().map_or(true, |it| it.is_empty()))
                                        && rpe.attach_ui.is_none()
                                    {
                                        0.5
                                    } else {
                                        1.
                                    },
                                bezier_map,
                            )?,
                            parse(r, &e.scale_y_events, factor, bezier_map)?,
                        ))
                    })
                    .transpose()?
                    .unwrap_or_default()
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
                    JudgeLineKind::Text(parse_events(r, events, Some(String::new()), bezier_map).with_context(|| ptl!("text-events-parse-failed"))?)
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
            } else {
                if let Some(texture) = line_texture_map.get(&rpe.texture) {
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
            }
        } else {
            if let Some(texture) = line_texture_map.get(&rpe.texture) {
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

fn get_bezier_map(rpe: &RPEChart) -> BezierMap {
    let mut map = FxHashMap::default();
    for line in &rpe.judge_line_list {
        for event_layer in line.event_layers.iter().flatten() {
            for event in event_layer
                .alpha_events
                .iter()
                .chain(event_layer.move_x_events.iter())
                .chain(event_layer.move_y_events.iter())
                .chain(event_layer.rotate_events.iter())
                .flatten()
            {
                add_bezier(&mut map, event);
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
    for (id, line) in rpe.judge_line_list.into_iter().enumerate() {
        let name = line.name.clone();
        lines.push(
            parse_judge_line(bpm_list.clone(), line, max_time, fs, &bezier_map, &mut hitsounds)
                .await
                .with_context(move || ptl!("judge-line-location-name", "jlid" => id, "name" => name))?,
        );
    }
    process_lines(&mut lines);
    Ok(Chart::new(rpe.meta.offset as f64 / 1000.0, lines, r, ChartSettings::default(), extra, hitsounds))
}
