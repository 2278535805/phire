crate::tl_file!("parser" ptl);

use super::{process_lines, RPE_TWEEN_MAP};
use crate::{
    core::{
        Anim, AnimFloat, AnimFloatF64, AnimVector, BpmList, Chart, ChartExtra, ChartSettings, EPS, JudgeLine, JudgeLineCache, JudgeLineKind, Keyframe, Note, NoteKind, Object, TweenId
    },
    judge::{HitSound, JudgeStatus},
};
use anyhow::{bail, Context, Result};
use rustc_hash::FxHashMap;
use std::cell::RefCell;
use tracing::warn;

#[derive(Default, Clone)]
struct EventCounts {
    speed_events: usize,
    alpha_events: usize,
    move_x_events: usize,
    move_y_events: usize,
    rotate_events: usize,
    notes: usize,
}

fn ensure_counts(counts: &mut Vec<EventCounts>, id: usize) -> &mut EventCounts {
    if counts.len() <= id {
        counts.resize_with(id + 1, EventCounts::default);
    }
    &mut counts[id]
}

fn count_events(source: &str) -> (Vec<EventCounts>, usize) {
    let mut counts: Vec<EventCounts> = Vec::new();
    let mut bpm_count: usize = 0;

    for (index, line) in source.lines().enumerate() {
        if index == 0 {
            continue;
        }

        let mut it = line.split_whitespace();
        let Some(cmd) = it.next() else { continue };

        match cmd.as_bytes() {
            b"bp" => {
                bpm_count += 1;
            }
            [b'n', b'1'..=b'4'] => {
                if let Some(Ok(id)) = it.next().map(str::parse::<usize>) {
                    ensure_counts(&mut counts, id).notes += 1;
                }
            }
            [b'#'] | [b'&'] => {}
            [b'c', kind] => {
                if let Some(Ok(id)) = it.next().map(str::parse::<usize>) {
                    let c = ensure_counts(&mut counts, id);
                    match kind {
                        b'v' => c.speed_events += 1,
                        b'p' => {
                            c.move_x_events += 1;
                            c.move_y_events += 1;
                        }
                        b'd' => c.rotate_events += 1,
                        b'a' => c.alpha_events += 1,
                        b'm' => {
                            c.move_x_events += 1;
                            c.move_y_events += 1;
                        }
                        b'r' => c.rotate_events += 1,
                        b'f' => c.alpha_events += 1,
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }

    (counts, bpm_count)
}


trait Take {
    fn take_f32(&mut self) -> Result<f32>;
    fn take_f64(&mut self) -> Result<f64>;
    fn take_usize(&mut self) -> Result<usize>;
    fn take_tween(&mut self) -> Result<TweenId>;
    fn take_time(&mut self, r: &mut BpmList) -> Result<f64>;
}

impl<'a, T: Iterator<Item = &'a str>> Take for T {
    fn take_f32(&mut self) -> Result<f32> {
        self.next()
            .ok_or_else(|| ptl!(err "unexpected-eol"))
            .and_then(|it| -> Result<f32> { Ok(it.parse()?) })
            .with_context(|| ptl!("expected-f32"))
    }

    fn take_f64(&mut self) -> Result<f64> {
        self.next()
            .ok_or_else(|| ptl!(err "unexpected-eol"))
            .and_then(|it| -> Result<f64> { Ok(it.parse()?) })
            .with_context(|| ptl!("expected-f64"))
    }

    fn take_usize(&mut self) -> Result<usize> {
        self.next()
            .ok_or_else(|| ptl!(err "unexpected-eol"))
            .and_then(|it| -> Result<usize> { Ok(it.parse()?) })
            .with_context(|| ptl!("expected-usize"))
    }

    fn take_tween(&mut self) -> Result<TweenId> {
        self.next()
            .ok_or_else(|| ptl!(err "unexpected-eol"))
            .and_then(|it| -> Result<u8> {
                let t = it.parse::<u8>()?;
                Ok(RPE_TWEEN_MAP.get(t as usize).copied().unwrap_or(RPE_TWEEN_MAP[0]))
            })
            .with_context(|| ptl!("expected-tween"))
    }

    fn take_time(&mut self, r: &mut BpmList) -> Result<f64> {
        self.take_f64().map(|it| r.time_beats(it))
    }
}

struct PECEvent {
    start_time: f64,
    end_time: f64,
    end: f32,
    easing: TweenId,
}

impl PECEvent {
    pub fn new(start_time: f64, end_time: f64, end: f32, tween: TweenId) -> Self {
        Self {
            start_time,
            end_time,
            end,
            easing: tween,
        }
    }

    pub fn single(time: f64, value: f32) -> Self {
        Self::new(time, time, value, 0)
    }
}

#[derive(Default)]
struct PECJudgeLine {
    speed_events: Vec<(f64, f64)>,
    alpha_events: Vec<PECEvent>,
    move_events: (Vec<PECEvent>, Vec<PECEvent>),
    rotate_events: Vec<PECEvent>,
    notes: Vec<Note>,
}

fn sanitize_events(events: &mut [PECEvent], id: usize, desc: &str) {
    events.sort_by(|a, b| {
        a.end_time
            .total_cmp(&b.end_time)
            .then_with(|| a.start_time.total_cmp(&b.start_time))
    });
    let mut last_start = 0.0;
    let mut last_end = f64::NEG_INFINITY;
    for e in events.iter_mut() {
        if e.start_time < last_end {
            warn!(
                judge_line = id,
                "Overlap detected in {desc} events: [{last_start}, {last_end}) and [{}, {}). Clipping the last one to [{last_end}, {})",
                e.start_time, e.end_time, e.end_time
            );
            e.start_time = last_end;
        }
        last_start = e.start_time;
        last_end = e.end_time;
    }
}

fn parse_events(mut events: Vec<PECEvent>, id: usize, desc: &str) -> Result<AnimFloat> {
    sanitize_events(&mut events, id, desc);
    let mut kfs = Vec::with_capacity(events.len() * 2);
    for e in events {
        if e.start_time == e.end_time {
            kfs.push(Keyframe::new(e.start_time, e.end, 0));
        } else {
            if kfs.is_empty() {
                bail!("failed to parse {desc} events: interpolating event found before a concrete value appears");
            }
            kfs.push(Keyframe::new(e.start_time, kfs.last().unwrap().value, e.easing));
            kfs.push(Keyframe::new(e.end_time, e.end, 0));
        }
    }
    Ok(AnimFloat::new(kfs))
}

fn parse_speed_events(pec: &[(f64, f64)], max_time: f64) -> AnimFloatF64 {
    let mut kfs = Vec::with_capacity(pec.len() + 2);
    let mut height = 0.0;
    let mut last_time = 0.0;
    let mut last_speed = 0.0;

    if pec[0].0 >= EPS {
        kfs.push(Keyframe::new(0.0, 0.0, 2));
    }

    for &(time, speed) in pec {
        height += (time - last_time) * last_speed;
        kfs.push(Keyframe::new(time, height, 2));
        last_time = time;
        last_speed = speed;
    }

    kfs.push(Keyframe::new(
        max_time,
        height + (max_time - last_time) * last_speed,
        0,
    ));
    AnimFloatF64::new(kfs)
}

fn parse_judge_line(mut pec: PECJudgeLine, id: usize, max_time: f64) -> Result<JudgeLine> {
    let mut height = parse_speed_events(&pec.speed_events, max_time);
    let mut process_notes = |notes: &mut Vec<Note>| {
        for note in notes {
            height.set_time(note.time);
            note.height = height.now();
            if let NoteKind::Hold {
                end_time,
                end_height,
                end_speed: _,
            } = &mut note.kind
            {
                height.set_time(*end_time);
                *end_height = height.now();
            }
        }
    };
    pec.move_events
        .0
        .iter_mut()
        .for_each(|it| it.end = it.end / 2048. * 2. - 1.);
    pec.move_events
        .1
        .iter_mut()
        .for_each(|it| it.end = it.end / 1400. * 2. - 1.);
    pec.alpha_events.iter_mut().for_each(|it| {
        if it.end >= 0.0 {
            it.end /= 255.;
        }
    });
    process_notes(&mut pec.notes);
    let cache = JudgeLineCache::new(&mut pec.notes);
    Ok(JudgeLine {
        object: Object {
            alpha: parse_events(pec.alpha_events, id, "alpha")?,
            translation: AnimVector(parse_events(pec.move_events.0, id, "move X")?, parse_events(pec.move_events.1, id, "move Y")?),
            rotation: parse_events(pec.rotate_events, id, "rotate")?,
            scale: AnimVector(AnimFloat::fixed(3.91 / 6.), AnimFloat::default()),
        },
        color: Anim::default(),
        ctrl_obj: RefCell::default(),
        kind: JudgeLineKind::Normal,
        height,
        incline: AnimFloat::default(),
        notes: pec.notes,
        parent: None,
        rotate_with_parent: false,
        anchor: [0.5, 0.5],
        z_index: 0,
        show_below: false,
        attach_ui: None,
        scale_on_notes: 0,

        cache,
    })
}

pub fn parse_pec(source: &str, extra: ChartExtra) -> Result<Chart> {
    let (event_counts, bpm_count) = count_events(source);

    let mut offset = None;
    let mut r = None;
    let mut lines: Vec<PECJudgeLine> = event_counts
        .into_iter()
        .map(|c| PECJudgeLine {
            speed_events: Vec::with_capacity(c.speed_events),
            alpha_events: Vec::with_capacity(c.alpha_events),
            move_events: (
                Vec::with_capacity(c.move_x_events),
                Vec::with_capacity(c.move_y_events),
            ),
            rotate_events: Vec::with_capacity(c.rotate_events),
            notes: Vec::with_capacity(c.notes),
        })
        .collect();
    let mut bpm_list = Vec::with_capacity(bpm_count);
    let mut last_line = None;
    let mut max_time: f64 = 0.0;

    fn get_line(lines: &mut Vec<PECJudgeLine>, id: usize) -> &mut PECJudgeLine {
        if lines.len() <= id {
            lines.resize_with(id + 1, PECJudgeLine::default);
        }
        &mut lines[id]
    }

    fn ensure_bpm<'a>(r: &'a mut Option<BpmList>, bpm_list: &mut Vec<(f64, f64)>) -> &'a mut BpmList {
        if r.is_none() {
            *r = Some(BpmList::new(std::mem::take(bpm_list)));
        }
        r.as_mut().unwrap()
    }
    macro_rules! bpm {
        () => {
            ensure_bpm(&mut r, &mut bpm_list)
        };
    }
    macro_rules! last_note {
        () => {{
            let Some(last_line) = last_line else {
                ptl!(bail "no-notes-inserted");
            };
            lines[last_line].notes.last_mut().unwrap()
        }};
    }
    let mut inner = |line: &str| -> Result<()> {
        let mut it = line.split_whitespace();
        if offset.is_none() {
            offset = Some(it.take_f64()? / 1000. - 0.15);
        } else {
            let Some(cmd) = it.next() else {
                return Ok(());
            };

            match cmd.as_bytes() {
                b"bp" => {
                    if r.is_some() {
                        ptl!(bail "bp-error");
                    }
                    bpm_list.push((it.take_f64()?, it.take_f64()?));
                }
                [b'n', digit @ b'1'..=b'4'] => {
                    let r = bpm!();
                    let line_id = it.take_usize()?;
                    last_line = Some(line_id);
                    let line = get_line(&mut lines, line_id);
                    let time = it.take_time(r)?;
                    max_time = max_time.max(time);
                    let kind = match digit {
                        b'1' => NoteKind::Click,
                        b'2' => {
                            let end_time = it.take_time(r)?;
                            max_time = max_time.max(end_time);
                            NoteKind::Hold {
                                end_time,
                                end_height: 0.0,
                                end_speed: None,
                            }
                        }
                        b'3' => NoteKind::Flick,
                        b'4' => NoteKind::Drag,
                        _ => unreachable!(),
                    };
                    let hitsound = HitSound::default_from_kind(&kind);
                    let position_x = it.take_f32()? / 1024.;
                    let above = it.take_usize()? == 1;
                    let fake = match it.take_usize()? {
                        0 => false,
                        1 => true,
                        _ => ptl!(bail "expected-01"),
                    };
                    line.notes.push(Note {
                        object: Object {
                            translation: AnimVector(AnimFloat::fixed(position_x), AnimFloat::default()),
                            ..Default::default()
                        },
                        kind,
                        hitsound,
                        time,
                        height: 0.0,
                        speed: 1.0,

                        above,
                        multiple_hint: false,
                        fake,
                        judge: JudgeStatus::NotJudged,
                        judge_scale: 1.0,
                        color: Anim::default(),
                        hit_fx_color: Anim::default(),
                        protected: false,
                    });
                    if it.next() == Some("#") {
                        last_note!().speed = it.take_f64()?;
                    }
                    if it.next() == Some("&") {
                        let note = last_note!();
                        let size = it.take_f32()?;
                        if (size - 1.0).abs() >= EPS as f32 {
                            note.object.scale.0 = AnimFloat::fixed(size);
                        }
                    }
                }
                [b'#'] => {
                    last_note!().speed = it.take_f64()?;
                }
                [b'&'] => {
                    let note = last_note!();
                    let size = it.take_f32()?;
                    if (size - 1.0).abs() >= EPS as f32 {
                        note.object.scale.0 = AnimFloat::fixed(size);
                    }
                }
                [b'c', kind] => {
                    let r = bpm!();
                    let line = get_line(&mut lines, it.take_usize()?);
                    let time = it.take_time(r)?;
                    match kind {
                        b'v' => {
                            line.speed_events.push((time, it.take_f64()? / 5.85));
                            max_time = max_time.max(time);
                        }
                        b'p' => {
                            let x = it.take_f32()?;
                            let y = it.take_f32()?;
                            line.move_events.0.push(PECEvent::single(time, x));
                            line.move_events.1.push(PECEvent::single(time, y));
                            max_time = max_time.max(time);
                        }
                        b'd' => {
                            line.rotate_events
                                .push(PECEvent::single(time, -it.take_f32()?));
                            max_time = max_time.max(time);
                        }
                        b'a' => {
                            line.alpha_events
                                .push(PECEvent::single(time, it.take_f32()?));
                            max_time = max_time.max(time);
                        }
                        b'm' => {
                            let end_time = it.take_time(r)?;
                            let x = it.take_f32()?;
                            let y = it.take_f32()?;
                            let t = it.take_tween()?;
                            max_time = max_time.max(end_time);
                            line.move_events
                                .0
                                .push(PECEvent::new(time, end_time, x, t));
                            line.move_events
                                .1
                                .push(PECEvent::new(time, end_time, y, t));
                        }
                        b'r' => {
                            let end_time = it.take_time(r)?;
                            let value = -it.take_f32()?;
                            let tween = it.take_tween()?;
                            max_time = max_time.max(end_time);
                            line.rotate_events
                                .push(PECEvent::new(time, end_time, value, tween));
                        }
                        b'f' => {
                            let end_time = it.take_time(r)?;
                            let value = it.take_f32()?;
                            max_time = max_time.max(end_time);
                            line.alpha_events
                                .push(PECEvent::new(time, end_time, value, 2));
                        }
                        _ => ptl!(bail "unknown-command", "cmd" => cmd),
                    }
                }
                _ => ptl!(bail "unknown-command", "cmd" => cmd),
            }
        }
        if let Some(next) = it.next() {
            ptl!(bail "unexpected-extra", "next" => next);
        }
        Ok(())
    };
    for (id, line) in source.lines().enumerate() {
        inner(line).with_context(|| ptl!("line-location", "lid" => id + 1))?;
    }

    let mut result_lines = Vec::with_capacity(lines.len());
    for (id, line) in lines.into_iter().enumerate() {
        result_lines.push(
            parse_judge_line(line, id, max_time + 1.0)
                .with_context(|| ptl!("judge-line-location", "jlid" => id))?,
        );
    }

    process_lines(&mut result_lines);
    ensure_bpm(&mut r, &mut bpm_list);
    Ok(Chart::new(
        offset.unwrap(),
        result_lines,
        r.unwrap(),
        ChartSettings {
            pe_alpha_extension: true,
            ..Default::default()
        },
        extra,
        FxHashMap::default(),
    ))
}
