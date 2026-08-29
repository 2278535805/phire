use crate::{
    config::Config, core::{BadNote, Chart, NOTE_WIDTH_RATIO_BASE, Note, NoteKind, Point, Resource, Vector}, ext::{NotNanExt, get_frame_latency, get_viewport},
};
use anyhow::Result;
use macroquad::prelude::{
    utils::{register_input_subscriber, repeat_all_miniquad_input},
    *,
};
use macroquad::miniquad::{EventHandler, MouseButton};
use once_cell::sync::Lazy;
use rustc_hash::{FxHashMap, FxHashSet};
use sasa::{PlaySfxParams, Sfx};
use serde::{Deserialize, Serialize};
use std::{cell::RefCell, collections::HashMap, num::FpCategory};

pub const FLICK_SPEED_THRESHOLD: f32 = 0.8;
pub const LIMIT_PERFECT: f64 = 0.08;
pub const LIMIT_GOOD: f64 = 0.18;
pub const LIMIT_BAD: f64 = 0.22;
pub const UP_TOLERANCE: f64 = 0.05;
pub const DIST_FACTOR: f64 = 0.2;
const LATE_OFFSET: f64 = 0.13;

pub fn play_sfx(sfx: &mut Sfx, amplifier: f32) {
    let _ = sfx.play(PlaySfxParams {
        amplifier,
    });
}

#[cfg(all(not(target_os = "windows"), not(target_os = "ios")))]
fn get_uptime() -> f64 {
    let mut time = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    let ret = unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut time) };
    assert!(ret == 0);
    time.tv_sec as f64 + time.tv_nsec as f64 * 1e-9
}

#[cfg(target_os = "ios")]
fn get_uptime() -> f64 {
    use crate::objc::*;
    unsafe {
        let process_info: ObjcId = msg_send![class!(NSProcessInfo), processInfo];
        msg_send![process_info, systemUptime]
    }
}

#[cfg(target_os = "windows")]
fn get_uptime() -> f64 {
    static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    let start = START.get_or_init(std::time::Instant::now);
    start.elapsed().as_secs_f64()
}

#[derive(Debug, Clone)]
pub enum HitSound {
    None,
    Click,
    Flick,
    Drag,
    Custom(String),
}

impl HitSound {
    pub fn play(&self, res: &mut Resource) {
        if res.config.volume_sfx < 1e-2 {
            return;
        }
        match self {
            HitSound::None => {}
            HitSound::Click => {
                if res.config.high_precision_sfx && res.config.autoplay() {
                    return;
                }
                if check_hitsound(&mut res.played_hitsounds_count, "click") {
                    play_sfx(&mut res.sfx_click, res.config.volume_sfx)
                }
            },
            HitSound::Flick => {
                if res.config.high_precision_sfx && res.config.autoplay() {
                    return;
                }
                if check_hitsound(&mut res.played_hitsounds_count, "flick") {
                    play_sfx(&mut res.sfx_flick, res.config.volume_sfx)
                }
            },
            HitSound::Drag => {
                if res.config.high_precision_sfx && res.config.autoplay() {
                    return;
                }
                if check_hitsound(&mut res.played_hitsounds_count, "drag") {
                    play_sfx(&mut res.sfx_drag, res.config.volume_sfx)
                }
            },
            HitSound::Custom(s) => {
                if let Some(sfx) = res.extra_sfxs.get_mut(s) {
                    if check_hitsound(&mut res.played_hitsounds_count, s) {
                        play_sfx(sfx, res.config.volume_sfx)
                    }
                }
            }
        }
    }

    pub fn default_from_kind(kind: &NoteKind) -> Self {
        match kind {
            NoteKind::Click => HitSound::Click,
            NoteKind::Flick => HitSound::Flick,
            NoteKind::Drag => HitSound::Drag,
            NoteKind::Hold { .. } => HitSound::Click,
        }
    }
}

fn check_hitsound(map: &mut HashMap<String, u8, rustc_hash::FxBuildHasher>, sfx: &str) -> bool {
    let count = map.entry(sfx.to_string()).or_insert(0);
    if *count < 5 {
        *count += 1;
        true
    } else {
        false
    }
}

pub const REPLAY_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReplayPhase {
    Started,
    Moved,
    Stationary,
    Ended,
    Cancelled,
}

impl From<TouchPhase> for ReplayPhase {
    fn from(value: TouchPhase) -> Self {
        match value {
            TouchPhase::Started => Self::Started,
            TouchPhase::Moved => Self::Moved,
            TouchPhase::Stationary => Self::Stationary,
            TouchPhase::Ended => Self::Ended,
            TouchPhase::Cancelled => Self::Cancelled,
        }
    }
}

impl From<ReplayPhase> for TouchPhase {
    fn from(value: ReplayPhase) -> Self {
        match value {
            ReplayPhase::Started => Self::Started,
            ReplayPhase::Moved => Self::Moved,
            ReplayPhase::Stationary => Self::Stationary,
            ReplayPhase::Ended => Self::Ended,
            ReplayPhase::Cancelled => Self::Cancelled,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ReplayTouch {
    pub id: u64,
    pub phase: ReplayPhase,
    pub position: [f32; 2],
    /// `None` means the event time is not precisely known (same frame as the touch state)
    pub time: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayFrame {
    /// raw `res.time` snapshot when this frame was recorded
    pub time: f64,
    pub touches: Vec<ReplayTouch>,
    /// number of key press events received this frame
    pub keys_down: u32,
    /// net key press/release delta of this frame (tracks held keys for holds)
    pub key_delta: i32,
    /// touch ids whose flick got triggered this frame
    pub flicks: Vec<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayHit {
    pub time: f64,
    pub kind: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReplayData {
    pub version: u32,
    /// playback speed used during recording
    pub speed: f32,
    pub frames: Vec<ReplayFrame>,
    pub hits: Vec<ReplayHit>,
}

impl ReplayData {
    pub fn to_json(&self) -> Result<String> {
        Ok(serde_json::to_string(self)?)
    }

    pub fn from_json(s: &str) -> Result<Self> {
        let data: Self = serde_json::from_str(s)?;
        if data.version != REPLAY_VERSION {
            anyhow::bail!("unsupported replay version {}", data.version);
        }
        Ok(data)
    }

    pub fn save(&self, path: &str) -> Result<()> {
        std::fs::write(path, self.to_json()?)?;
        Ok(())
    }

    pub fn load(path: &str) -> Result<Self> {
        Self::from_json(&String::from_utf8(std::fs::read(path)?)?)
    }
}

#[derive(Default)]
pub struct ReplayRecorder {
    pub frames: Vec<ReplayFrame>,
}

pub struct ReplayPlayer {
    pub data: ReplayData,
    pub frame_index: usize,
    pub keys_down: u32,
    pub current_touches: Vec<Touch>,
}

pub struct FlickTracker {
    threshold: f32,
    last_point: Point,
    last_delta: Option<Vector>,
    last_time: f32,
    flicked: bool,
    stopped: bool,
}

impl FlickTracker {
    pub fn new(_dpi: u32, time: f32, point: Point) -> Self {
        // TODO maybe a better approach?
        let dpi = 275;
        Self {
            threshold: FLICK_SPEED_THRESHOLD * dpi as f32 / 386.,
            last_point: point,
            last_delta: None,
            last_time: time,
            flicked: false,
            stopped: true,
        }
    }

    pub fn push(&mut self, time: f32, position: Point) {
        let delta = position - self.last_point;
        self.last_point = position;
        if let Some(last_delta) = &self.last_delta {
            let dt = time - self.last_time;
            let speed = delta.dot(last_delta) / dt;
            if speed < self.threshold {
                self.stopped = true;
            }
            if self.stopped && !self.flicked {
                self.flicked = delta.magnitude() / dt >= self.threshold * 2.;
            }
            // if speed < self.threshold || self.stopped {
            // self.stopped = delta.magnitude() / dt < self.threshold * 5.;
            // self.flicked = self.threshold <= speed;
            // if self.flicked {
            // warn!("new flick!");
            // }
            // }
        }
        self.last_delta = Some(delta.normalize());
        self.last_time = time;
    }
}

#[derive(Debug)]
pub enum JudgeStatus {
    NotJudged,
    PreJudge,
    Judged,
    Hold(bool, f64, f64, bool, f64), // perfect, at, diff, pre-judge, up-time
}

#[repr(u8)]
#[derive(Debug, Copy, Clone, Serialize)]
pub enum Judgement {
    Perfect,
    Good,
    Bad,
    Miss,
}

#[cfg(not(feature = "closed"))]
#[derive(Default)]
pub(crate) struct JudgeInner {
    perfect_diffs: Vec<f64>,
    good_diffs: Vec<f64>,
    bad_diffs: Vec<f64>,

    combo: u32,
    max_combo: u32,
    counts: [u32; 4],
    num_of_notes: u32,
}

#[cfg(not(feature = "closed"))]
impl JudgeInner {
    pub fn new(num_of_notes: u32) -> Self {
        Self {
            perfect_diffs: Vec::new(),
            good_diffs: Vec::new(),
            bad_diffs: Vec::new(),

            combo: 0,
            max_combo: 0,
            counts: [0; 4],
            num_of_notes,
        }
    }

    pub fn commit(&mut self, what: Judgement, diff: f64) {
        use Judgement::*;
        if matches!(what, Judgement::Perfect) {
            self.perfect_diffs.push(diff);
        }
        if matches!(what, Judgement::Good) {
            self.good_diffs.push(diff);
        }
        if matches!(what, Judgement::Bad) {
            self.bad_diffs.push(diff);
        }
        self.counts[what as usize] += 1;
        match what {
            Perfect | Good => {
                self.combo += 1;
                if self.combo > self.max_combo {
                    self.max_combo = self.combo;
                }
            }
            _ => {
                self.combo = 0;
            }
        }
    }

    pub fn commit_diff(&mut self, what: Judgement) {
        self.counts[what as usize] += 1;
        match what {
            Judgement::Perfect | Judgement::Good => {
                self.combo += 1;
                if self.combo > self.max_combo {
                    self.max_combo = self.combo;
                }
            }
            _ => {
                self.combo = 0;
            }
        }
    }

    pub fn reset(&mut self) {
        self.combo = 0;
        self.max_combo = 0;
        self.counts = [0; 4];
        self.perfect_diffs.clear();
        self.good_diffs.clear();
        self.bad_diffs.clear();
    }

    pub fn accuracy(&self) -> f64 {
        (self.counts[0] as f64 + self.counts[1] as f64 * 0.65) / self.num_of_notes as f64
    }

    pub fn real_time_accuracy(&self) -> f64 {
        let cnt = self.counts.iter().sum::<u32>();
        if cnt == 0 {
            return 1.;
        }
        (self.counts[0] as f64 + self.counts[1] as f64 * 0.65) / cnt as f64
    }

    pub fn score(&self) -> f64 {
        if self.counts[0] == self.num_of_notes {
            1_000_000.0
        } else {
            ((
                0.9 * self.accuracy() + self.max_combo as f64 / self.num_of_notes as f64 * 0.1
            ) * 1_000_000.0).round()
        }
    }

    pub fn result(&self, track_complete: bool) -> PlayResult {
        let early = self.good_diffs.iter().filter(|it| **it < 0.).count() as u32;
        let n = self.perfect_diffs.len() + self.good_diffs.len() + self.bad_diffs.len();
        let std = if n == 0 {
            LIMIT_BAD as f32
        } else {
            let n = n as f64;
            let all_diffs = self.perfect_diffs.iter().chain(self.good_diffs.iter()).chain(self.bad_diffs.iter());
            let mean = all_diffs.clone().sum::<f64>() / n;
            let variance = all_diffs.map(|d| (d - mean).powi(2)).sum::<f64>() / n;
            (variance.sqrt() * 1000.) as f32
        };
        PlayResult {
            score: self.score(),
            accuracy: self.accuracy(),
            max_combo: self.max_combo,
            num_of_notes: self.num_of_notes,
            counts: self.counts,
            early,
            late: self.good_diffs.len() as u32 - early,
            std,
            track_complete,
        }
    }

    pub fn combo(&self) -> u32 {
        self.combo
    }

    pub fn counts(&self) -> [u32; 4] {
        self.counts
    }

    pub fn is_vaild(&self) -> bool {
        self.combo == 0
        || self.perfect_diffs.len() + self.good_diffs.len() + self.bad_diffs.len() > 0
    }
}

#[cfg(feature = "closed")]
use crate::inner::*;

#[repr(C)]
pub struct Judge {
    // notes of each line in order
    // LinkedList::drain_filter is unstable...
    pub notes: Vec<(Vec<u32>, usize)>,
    pub trackers: FxHashMap<u64, FlickTracker>,
    pub last_time: f64,

    pub limit_perfect: f64,
    pub limit_good: f64,
    pub limit_bad: f64,

    key_down_count: u32,

    pub(crate) inner: JudgeInner,
    pub judgements: RefCell<Vec<(f64, u32, u32, Result<Judgement, bool>)>>,

    pub replay_recorder: Option<ReplayRecorder>,
    pub replay_player: Option<ReplayPlayer>,
}

static SUBSCRIBER_ID: Lazy<usize> = Lazy::new(register_input_subscriber);
thread_local! {
    static TOUCHES: RefCell<TouchStatus> = RefCell::default();
    static WHEEL: RefCell<(f32, f32)> = RefCell::default();
}

pub fn take_wheel() -> (f32, f32) {
    WHEEL.with(|it| std::mem::take(&mut *it.borrow_mut()))
}

impl Judge {
    pub fn new(chart: &Chart) -> Self {
        let notes = chart
            .lines
            .iter()
            .map(|line| {
                let mut idx: Vec<u32> = (0..(line.notes.len() as u32)).filter(|it| !line.notes[*it as usize].fake).collect();
                idx.sort_unstable_by_key(|id| line.notes[*id as usize].time.not_nan());
                (idx, 0)
            })
            .collect();
        Self {
            notes,
            trackers: FxHashMap::with_capacity_and_hasher(16, Default::default()),
            last_time: 0.,

            limit_perfect: LIMIT_PERFECT,
            limit_good: LIMIT_GOOD,
            limit_bad: LIMIT_BAD,

            key_down_count: 0,

            inner: JudgeInner::new(chart.lines.iter().map(|it| it.notes.iter().filter(|it| !it.fake).count() as u32).sum()),
            judgements: RefCell::new(Vec::new()),

            replay_recorder: None,
            replay_player: None,
        }
    }

    pub fn start_recording(&mut self) {
        self.replay_recorder = Some(ReplayRecorder::default());
    }

    /// Collects recorded frames and resolves hit sounds from the judgement log.
    /// The recorder itself stays armed (emptied) so that a retry of the same scene
    /// instance keeps recording from scratch.
    pub fn stop_recording(&mut self, chart: &Chart, speed: f32) -> ReplayData {
        let frames = std::mem::take(&mut self.replay_recorder.get_or_insert_with(ReplayRecorder::default).frames);
        let judgements = std::mem::take(&mut *self.judgements.borrow_mut());
        let mut hits: Vec<ReplayHit> = judgements
            .iter()
            .filter_map(|(t, line_id, note_id, res)| {
                let note = &chart.lines[*line_id as usize].notes[*note_id as usize];
                if note.fake {
                    return None;
                }
                let kind = match res {
                    Err(_) => "click".to_owned(), // hold
                    Ok(Judgement::Bad) | Ok(Judgement::Miss) => return None,
                    Ok(_) => {
                        if matches!(note.kind, NoteKind::Hold { .. }) {
                            return None;
                        }
                        match &note.hitsound {
                            HitSound::Click => "click".to_owned(),
                            HitSound::Drag => "drag".to_owned(),
                            HitSound::Flick => "flick".to_owned(),
                            HitSound::Custom(name) => name.clone(),
                            HitSound::None => return None,
                        }
                    }
                };
                Some(ReplayHit { time: *t, kind })
            })
            .collect();
        hits.sort_by(|a, b| a.time.total_cmp(&b.time));
        ReplayData {
            version: REPLAY_VERSION,
            speed,
            frames,
            hits,
        }
    }

    pub fn load_replay(&mut self, data: ReplayData) {
        self.replay_player = Some(ReplayPlayer {
            data,
            frame_index: 0,
            keys_down: 0,
            current_touches: Vec::new(),
        });
    }

    pub fn set_limits(&mut self, perfect: f64, good: f64, bad: f64) {
        self.limit_perfect = perfect;
        self.limit_good = good;
        self.limit_bad = bad;
    }

    pub fn reset(&mut self) {
        self.notes.iter_mut().for_each(|it| it.1 = 0);
        self.trackers.clear();
        self.inner.reset();
        self.judgements.borrow_mut().clear();
        if let Some(recorder) = &mut self.replay_recorder {
            recorder.frames.clear();
        }
        if let Some(player) = &mut self.replay_player {
            player.frame_index = 0;
            player.keys_down = 0;
            player.current_touches.clear();
        }
    }

    pub fn commit(&mut self, t: f64, what: Judgement, line_id: u32, note_id: u32, diff: f64) {
        self.judgements.borrow_mut().push((t, line_id, note_id, Ok(what)));
        self.inner.commit(what, diff);
    }

    #[inline]
    pub fn accuracy(&self) -> f64 {
        self.inner.accuracy()
    }

    #[inline]
    pub fn real_time_accuracy(&self) -> f64 {
        self.inner.real_time_accuracy()
    }

    #[inline]
    pub fn score(&self) -> f64 {
        self.inner.score()
    }

    pub(crate) fn on_new_frame() {
        let mut handler = Handler {
            status: TouchStatus::default(),
            wheel: (0., 0.),
        };
        repeat_all_miniquad_input(&mut handler, *SUBSCRIBER_ID);
        handler.finalize();
        TOUCHES.with(|it| {
            *it.borrow_mut() = handler.status;
        });
        WHEEL.with(|it| {
            *it.borrow_mut() = handler.wheel;
        });
    }

    fn rotate_vec2(vec: Vec2, angle_rad: f32) -> Vec2 {
        let cos_theta = angle_rad.cos();
        let sin_theta = angle_rad.sin();

        Vec2::new(
            vec.x * cos_theta - vec.y * sin_theta,
            vec.x * sin_theta + vec.y * cos_theta,
        )
    }

    fn touch_transform(flip_x: bool, scale: f32, angle: f32, low_resolution_mode: bool) -> impl Fn(&mut Touch) {
        let vp = get_viewport();
        move |touch| {
            let p = if low_resolution_mode {
                vec2(touch.position.x / 2., touch.position.y / 2.)
            } else {
                touch.position
            };
            touch.position = vec2(
                (p.x - vp.0 as f32) / vp.2 as f32 * 2. - 1.,
                ((p.y - (vp.3 as f32 - (vp.1 + vp.3) as f32)) / vp.3 as f32 * 2. - 1.) / (vp.2 as f32 / vp.3 as f32),
            );
            if flip_x {
                touch.position.x *= -1.;
            }
            touch.position = Self::rotate_vec2(touch.position, angle);
            touch.position /= scale;
        }
    }

    pub fn get_touches(scale: f32, low_resolution_mode: bool) -> Vec<Touch> {
        TOUCHES.with(|it| {
            let guard = it.borrow();
            let tr = Self::touch_transform(false, scale, 0., low_resolution_mode);
            guard
                .touches
                .iter()
                .cloned()
                .map(|mut it| {
                    tr(&mut it);
                    it
                })
                .collect()
        })
    }

    pub fn update(&mut self, res: &mut Resource, chart: &mut Chart, bad_notes: &mut Vec<BadNote>, angle: f32) {
        res.played_hitsounds_count.clear();
        if res.config.autoplay() {
            self.auto_play_update(res, chart);
            return;
        }
        let x_diff_max: f64 = if res.config.full_scrrn_judge() {
            2. / res.config.chart_ratio as f64
        } else {
            0.21 / (16. / 9.) * 2.
        };
        let spd = res.config.speed as f64;

        let uptime = get_uptime();

        let t = if res.config.auto_tweak_offset {
            res.time - (res.config.judge_offset + get_frame_latency(&res.frame_times)) * res.config.speed as f64
        } else {
            res.time - res.config.judge_offset * res.config.speed as f64
        };
        // TODO optimize
        let mut touches: HashMap<u64, Touch> = {
            let mut touches: Vec<Touch> = touches().into_iter().map(|t| Touch { id: t.id, phase: t.phase, position: t.position, time: f64::NEG_INFINITY }).collect();
            let btn = MouseButton::Left;
            let id = button_to_id(btn);
            if is_mouse_button_pressed(btn) {
                let p = mouse_position();
                touches.push(Touch {
                    id,
                    phase: TouchPhase::Started,
                    position: vec2(p.0, p.1),
                    time: f64::NEG_INFINITY,
                });
            } else if is_mouse_button_down(btn) {
                let p = mouse_position();
                touches.push(Touch {
                    id,
                    phase: TouchPhase::Moved,
                    position: vec2(p.0, p.1),
                    time: f64::NEG_INFINITY,
                });
            } else if is_mouse_button_released(btn) {
                let p = mouse_position();
                touches.push(Touch {
                    id,
                    phase: TouchPhase::Ended,
                    position: vec2(p.0, p.1),
                    time: f64::NEG_INFINITY,
                });
            }
            let tr = Self::touch_transform(res.config.flip_x(), res.config.chart_ratio, angle, res.config.low_resolution_mode);
            touches
                .into_iter()
                .map(|mut it| {
                    tr(&mut it);
                    (it.id, it)
                })
                .collect()
        };
        let (events, keys_down) = TOUCHES.with(|it| {
            let guard = it.borrow();
            (guard.touches.clone(), guard.keys_down)
        });
        let key_delta = TOUCHES.with(|it| it.borrow().key_delta);
        self.key_down_count = self.key_down_count.saturating_add_signed(key_delta);
        let mut frame_flicks: Vec<u64> = Vec::new();
        {
            fn to_local(Vec2 { x, y }: Vec2) -> Point {
                Point::new(x / screen_width() * 2. - 1., y / screen_height() * 2. - 1.)
            }
            let delta = ((t / spd - self.last_time)) / (events.len() + 1) as f64;
            let mut t = self.last_time;
            for Touch {
                id,
                phase,
                position: p,
                time,
            } in events.into_iter()
            {
                t += delta;
                let t = t as f32;
                let p = to_local(p);
                match phase {
                    TouchPhase::Started => {
                        self.trackers.insert(id, FlickTracker::new(res.dpi, t, p));
                        touches
                            .entry(id)
                            .or_insert_with(|| Touch {
                                id,
                                phase: TouchPhase::Started,
                                position: vec2(p.x, p.y),
                                time,
                            })
                            .phase = TouchPhase::Started;
                    }
                    TouchPhase::Moved | TouchPhase::Stationary => {
                        if let Some(tracker) = self.trackers.get_mut(&id) {
                            let was_flicked = tracker.flicked;
                            tracker.push(t, p);
                            if !was_flicked && tracker.flicked {
                                frame_flicks.push(id);
                            }
                        }
                    }
                    TouchPhase::Ended | TouchPhase::Cancelled => {
                        self.trackers.remove(&id);
                    }
                }
            }
        }
        let touches: Vec<Touch> = touches
            .into_values()
            .map(|mut it| {
                it.time = if it.time.is_infinite() {
                    f64::NEG_INFINITY
                } else {
                    t - (uptime - it.time) * spd
                };
                it
            })
            .collect();
        if let Some(recorder) = &mut self.replay_recorder {
            // normalize the aspect-dependent y back to screen-relative coordinates so
            // replays render correctly on viewports with a different aspect ratio
            let vp = get_viewport();
            let ar = vp.2 as f32 / vp.3 as f32;
            recorder.frames.push(ReplayFrame {
                time: res.time - res.config.judge_offset,
                touches: touches
                    .iter()
                    .map(|it| ReplayTouch {
                        id: it.id,
                        phase: it.phase.into(),
                        position: [it.position.x, it.position.y * ar],
                        time: if it.time.is_infinite() { None } else { Some(it.time) },
                    })
                    .collect(),
                keys_down,
                key_delta,
                flicks: frame_flicks,
            });
        }
        self.run_judgement(res, chart, bad_notes, t, spd, x_diff_max, touches, keys_down);
    }

    pub fn update_replay(&mut self, res: &mut Resource, chart: &mut Chart, bad_notes: &mut Vec<BadNote>) {
        res.played_hitsounds_count.clear();
        let spd = res.config.speed as f64;
        let x_diff_max: f64 = if res.config.full_scrrn_judge() {
            2. / res.config.chart_ratio as f64
        } else {
            0.21 / (16. / 9.) * 2.
        };
        let now = res.time;

        let mut keys_down = 0u32;
        if let Some(player) = self.replay_player.as_mut() {
            // ids whose Started phase appeared within this consumed batch; their press
            // state must survive until judgement even if later frames show movement
            let mut fresh_started: FxHashSet<u64> = FxHashSet::default();
            let mut presses = 0u32;
            let mut held_delta = 0i32;
            while player.frame_index < player.data.frames.len() && player.data.frames[player.frame_index].time <= now {
                let frame = player.data.frames[player.frame_index].clone();
                for rt in &frame.touches {
                    if rt.phase == ReplayPhase::Started {
                        self.trackers
                            .entry(rt.id)
                            .or_insert_with(|| FlickTracker::new(res.dpi, frame.time as _, Point::new(rt.position[0], rt.position[1])));
                    }
                }
                for id in &frame.flicks {
                    self.trackers
                        .entry(*id)
                        .or_insert_with(|| FlickTracker::new(res.dpi, frame.time as _, Point::new(0., 0.)));
                    if let Some(tracker) = self.trackers.get_mut(id) {
                        tracker.flicked = true;
                    }
                }
                for rt in &frame.touches {
                    match rt.phase {
                        ReplayPhase::Started => {
                            fresh_started.insert(rt.id);
                            player.current_touches.retain(|it| it.id != rt.id);
                            player.current_touches.push(Touch {
                                id: rt.id,
                                phase: TouchPhase::Started,
                                position: vec2(rt.position[0], rt.position[1]),
                                time: rt.time.unwrap_or(f64::NEG_INFINITY),
                            });
                        }
                        ReplayPhase::Ended | ReplayPhase::Cancelled => {
                            fresh_started.remove(&rt.id);
                            player.current_touches.retain(|it| it.id != rt.id);
                            self.trackers.remove(&rt.id);
                        }
                        phase => {
                            if let Some(it) = player.current_touches.iter_mut().find(|it| it.id == rt.id) {
                                if !fresh_started.contains(&rt.id) {
                                    it.phase = phase.into();
                                    it.position = vec2(rt.position[0], rt.position[1]);
                                }
                            } else {
                                player.current_touches.push(Touch {
                                    id: rt.id,
                                    phase: phase.into(),
                                    position: vec2(rt.position[0], rt.position[1]),
                                    time: rt.time.unwrap_or(f64::NEG_INFINITY),
                                });
                            }
                        }
                    }
                }
                // a finger absent from this frame's snapshot has been lifted
                let present: FxHashSet<u64> = frame.touches.iter().map(|it| it.id).collect();
                player.current_touches.retain(|it| present.contains(&it.id));
                presses += frame.keys_down;
                held_delta += frame.key_delta;
                player.frame_index += 1;
            }
            // every press of the batch must reach the hit loop, and the held-key count
            // is maintained via the recorded deltas just like during live play
            keys_down = presses;
            player.keys_down = player.keys_down.saturating_add_signed(held_delta);
        }
        let touches = self.replay_touches();
        self.key_down_count = self
            .replay_player
            .as_ref()
            .map(|it| it.keys_down)
            .unwrap_or(keys_down);
        self.run_judgement(res, chart, bad_notes, now, spd, x_diff_max, touches, keys_down);
        if let Some(player) = self.replay_player.as_mut() {
            for it in player.current_touches.iter_mut() {
                it.phase = TouchPhase::Stationary;
            }
        }
        self.last_time = now / spd;
    }

    /// Replay touch states converted into the current viewport's judge coordinate space.
    /// Stored positions are aspect-normalized; the y axis is re-scaled to whatever
    /// viewport the replay is running on.
    pub fn replay_touches(&self) -> Vec<Touch> {
        let Some(player) = &self.replay_player else {
            return Vec::new();
        };
        let vp = get_viewport();
        let ar = vp.2 as f32 / vp.3 as f32;
        player
            .current_touches
            .iter()
            .map(|it| {
                let mut t = it.clone();
                t.position.y /= ar;
                t
            })
            .collect()
    }

    fn run_judgement(
        &mut self,
        res: &mut Resource,
        chart: &mut Chart,
        bad_notes: &mut Vec<BadNote>,
        t: f64,
        spd: f64,
        x_diff_max: f64,
        touches: Vec<Touch>,
        keys_down: u32,
    ) {
        // pos[line][touch]
        let mut pos = Vec::<Vec<Option<Point>>>::with_capacity(chart.lines.len());
        for id in 0..chart.lines.len() {
            chart.lines[id].object.set_time(t);
            let inv = chart.lines[id].now_transform(res, &chart.lines).try_inverse().unwrap();
            pos.push(
                touches
                    .iter()
                    .map(|touch| {
                        let p = touch.position;
                        let p = inv.transform_point(&Point::new(p.x, -p.y));
                        fn ok(f: f32) -> bool {
                            matches!(f.classify(), FpCategory::Zero | FpCategory::Subnormal | FpCategory::Normal)
                        }
                        if ok(p.x) && ok(p.y) {
                            Some(p)
                        } else {
                            None
                        }
                    })
                    .collect(),
            );
        }
        let time_of = |touch: &Touch| {
            if touch.time.is_infinite() {
                t
            } else {
                touch.time
            }
        };
        let mut judgements = Vec::new();
        // clicks & flicks
        for (id, touch) in touches.iter().enumerate() {
            let click = touch.phase == TouchPhase::Started;
            let flick =
                matches!(touch.phase, TouchPhase::Moved | TouchPhase::Stationary) && self.trackers.get_mut(&touch.id).is_some_and(|it| it.flicked);
            if !(click || flick) {
                continue;
            }
            let t = time_of(touch);
            let mut closest = (None, x_diff_max, self.limit_bad, self.limit_bad + (x_diff_max / NOTE_WIDTH_RATIO_BASE - 1.).max(0.) * DIST_FACTOR, 0.);
            for (line_id, ((line, pos), (idx, st))) in chart.lines.iter_mut().zip(pos.iter()).zip(self.notes.iter_mut()).enumerate() {
                let Some(pos) = pos[id] else { continue; };
                for id in &idx[*st..] {
                    let note = &mut line.notes[*id as usize];
                    if !matches!(note.judge, JudgeStatus::NotJudged | JudgeStatus::PreJudge) {
                        continue;
                    }
                    if !click && matches!(note.kind, NoteKind::Click | NoteKind::Hold { .. }) {
                        continue;
                    }
                    let dt = (note.time - t) / spd;
                    if dt.abs() >= closest.3.abs() {
                        break;
                    }
                    // let dt = if dt < 0. { (dt + EARLY_OFFSET).min(0.).abs() } else { dt };
                    let x = &mut note.object.translation.0;
                    x.set_time(t);
                    let posx = pos.x;
                    let dist = (x.now() - posx).abs() as f64;
                    if dist > (x_diff_max - NOTE_WIDTH_RATIO_BASE) + NOTE_WIDTH_RATIO_BASE * note.judge_scale {
                        continue;
                    }
                    if dt.abs() >
                        if matches!(note.kind, NoteKind::Click) {
                            self.limit_bad // LIMIT_BAD - LIMIT_PERFECT * (dist - 0.9).max(0.)
                        } else {
                            self.limit_good
                        }
                    {
                        continue;
                    }
                    let dist_key = if res.config.full_scrrn_judge() {
                        (dist / NOTE_WIDTH_RATIO_BASE - 1.).max(0.) * 0.01
                    } else {
                        (dist / NOTE_WIDTH_RATIO_BASE - 1.).max(0.) * DIST_FACTOR
                    };
                    let key = if matches!(note.kind, NoteKind::Flick | NoteKind::Drag) { // Low Priority
                        dt.abs() + self.limit_bad
                    } else if dt < -self.limit_good { // Prevent Late Bad
                        dt.abs()
                    } else if dt < 0.0 {
                        (dt + LATE_OFFSET).min(0.0).abs() // Protect Late Good
                    } else {
                        dt.abs()
                    };
                    let key = key + dist_key;
                    if key < closest.3 {
                        closest = (Some((line_id, *id)), dist, dt, key, posx);
                    }
                }
            }
            if let (Some((line_id, id)), _, dt, _, posx) = closest {
                let can_protect_note = |note: &mut Note| {
                    let x = &mut note.object.translation.0;
                    x.set_time(t);
                    let judge_time = t - note.time;
                    matches!(note.kind, NoteKind::Drag | NoteKind::Flick)
                        && (-self.limit_good..=self.limit_bad).contains(&judge_time)
                        && (x.now() - posx).abs() as f64 <= (x_diff_max - NOTE_WIDTH_RATIO_BASE) + NOTE_WIDTH_RATIO_BASE * note.judge_scale // note_dist <= x_diff_max
                        && !note.protected
                        && !note.fake
                };
                let lines = &mut chart.lines;
                if matches!(lines[line_id].notes[id as usize].kind, NoteKind::Drag) {
                    // debug!("reject by drag");
                    continue;
                }
                if click {
                    if dt > self.limit_perfect {
                        let mut any = false;
                        lines.iter_mut()
                            .flat_map(|line| line.notes.iter_mut())
                            .for_each(|note| {
                                if can_protect_note(note) {
                                    note.protected = true;
                                    any = true;
                                }
                            });
                        if any {
                            continue;
                        }
                    }
                    // click & hold
                    let note = &mut lines[line_id].notes[id as usize];
                    let dt = dt.abs();
                    if matches!(note.kind, NoteKind::Flick) {
                        // debug!("reject by flick");
                        continue; // to next loop
                    }
                    if dt <= self.limit_good || matches!(note.kind, NoteKind::Hold { .. }) {
                        match note.kind {
                            NoteKind::Click => {
                                note.judge = JudgeStatus::Judged;
                                judgements.push((if dt <= self.limit_perfect { Judgement::Perfect } else { Judgement::Good }, line_id, id, Some(t)));
                                #[cfg(feature = "play")]
                                if res.config.health_mode.is_some() {
                                    res.health.on_judge(Judgement::Perfect);
                                }
                            }
                            NoteKind::Hold { .. } => {
                                play_sfx(&mut res.sfx_click, res.config.volume_sfx);
                                self.judgements.borrow_mut().push((t, line_id as _, id, Err(dt <= self.limit_perfect)));
                                note.judge = JudgeStatus::Hold(dt <= self.limit_perfect, t, t, false, f64::INFINITY);
                            }
                            _ => unreachable!(),
                        };
                    } else {
                        // prevent extra judgements
                        if matches!(note.judge, JudgeStatus::NotJudged) {
                            // keep the note after bad judgement
                            note.judge = JudgeStatus::PreJudge;
                            judgements.push((Judgement::Bad, line_id, id, None));
                            #[cfg(feature = "play")]
                            if res.config.health_mode.is_some() {
                                res.health.on_judge(Judgement::Bad);
                            }
                        }
                    }
                } else {
                    // flick
                    lines[line_id].notes[id as usize].judge = JudgeStatus::PreJudge;
                    if let Some(tracker) = self.trackers.get_mut(&touch.id) {
                        tracker.flicked = false;
                    }
                }
            }
        }
        for _ in 0..keys_down {
            // find the earliest not judged click / hold note
            if let Some((line_id, id)) = chart
                .lines
                .iter()
                .zip(self.notes.iter())
                .enumerate()
                .filter_map(|(line_id, (line, (idx, st)))| {
                    idx[*st..]
                        .iter()
                        .cloned()
                        .find(|id| {
                            let note = &line.notes[*id as usize];
                            matches!(note.judge, JudgeStatus::NotJudged) && matches!(note.kind, NoteKind::Click | NoteKind::Hold { .. })
                        })
                        .map(|id| (line_id, id))
                })
                .min_by_key(|(line_id, id)| chart.lines[*line_id].notes[*id as usize].time.not_nan())
            {
                let note = &mut chart.lines[line_id].notes[id as usize];
                let dt = (t - note.time).abs() / spd;
                if dt <= if matches!(note.kind, NoteKind::Click) { self.limit_bad } else { self.limit_good } {
                    match note.kind {
                        NoteKind::Click => {
                            note.judge = JudgeStatus::Judged;
                            let judge = if dt <= self.limit_perfect {
                                    Judgement::Perfect
                                } else if dt <= self.limit_good {
                                    Judgement::Good
                                } else {
                                    Judgement::Bad
                                };
                            judgements.push((judge, line_id, id, None));
                            #[cfg(feature = "play")]
                            if res.config.health_mode.is_some() {
                                res.health.on_judge(judge);
                            }
                        }
                        NoteKind::Hold { .. } => {
                            note.hitsound.play(res);
                            self.judgements.borrow_mut().push((t, line_id as _, id, Err(dt <= self.limit_perfect)));
                            note.judge = JudgeStatus::Hold(dt <= self.limit_perfect, t, (t - note.time) / spd, false, f64::INFINITY);
                        }
                        _ => unreachable!(),
                    };
                }
            } else {
                break;
            }
        }
        for (line_id, ((line, pos), (idx, st))) in chart.lines.iter_mut().zip(pos.iter()).zip(self.notes.iter()).enumerate() {
            line.object.set_time(t);
            for id in &idx[*st..] {
                let note = &mut line.notes[*id as usize];
                let x_diff_max = (x_diff_max - NOTE_WIDTH_RATIO_BASE) + NOTE_WIDTH_RATIO_BASE * note.judge_scale;
                if let NoteKind::Hold { end_time, .. } = &note.kind {
                    if let JudgeStatus::Hold(.., ref mut pre_judge, ref mut up_time) = note.judge {
                        if (*end_time - t) / spd <= self.limit_bad {
                            *pre_judge = true;
                            continue;
                        }
                        let x = &mut note.object.translation.0;
                        x.set_time(t);
                        let x = x.now();
                        if self.key_down_count == 0 && !pos.iter().any(|it| it.is_some_and(|it| (it.x - x).abs() <= x_diff_max as f32)) {
                            if t > *up_time + UP_TOLERANCE {
                                note.judge = JudgeStatus::Judged;
                                judgements.push((Judgement::Miss, line_id, *id, None));
                                #[cfg(feature = "play")]
                                if res.config.health_mode.is_some() {
                                    res.health.on_judge(Judgement::Miss);
                                }
                            } else if up_time.is_infinite() {
                                *up_time = t;
                            }
                        } else {
                            *up_time = f64::INFINITY;
                        }
                        continue;
                    }
                }
                if !matches!(note.judge, JudgeStatus::NotJudged) {
                    continue;
                }
                // process miss
                let dt = (t - note.time) / spd;
                if dt > self.limit_bad {
                    note.judge = JudgeStatus::Judged;
                    judgements.push((Judgement::Miss, line_id, *id, None));
                    #[cfg(feature = "play")]
                    if res.config.health_mode.is_some() {
                        res.health.on_judge(Judgement::Miss);
                    }
                    continue;
                }
                if -dt > self.limit_bad {
                    break;
                }
                if !matches!(note.kind, NoteKind::Drag) && (self.key_down_count == 0 || !matches!(note.kind, NoteKind::Flick)) {
                    continue;
                }
                let dt = dt.abs();
                let x = &mut note.object.translation.0;
                x.set_time(t);
                let x = x.now();
                if self.key_down_count != 0
                    || pos.iter().any(|it| {
                        it.is_some_and(|it| {
                            let dx = (it.x - x).abs() as f64;
                            dx <= x_diff_max && dt <= (self.limit_bad - self.limit_perfect * (dx - 0.9).max(0.))
                        })
                    })
                {
                    note.judge = JudgeStatus::PreJudge;
                }
            }
        }
        // process pre-judge
        for (line_id, (line, (idx, st))) in chart.lines.iter_mut().zip(self.notes.iter()).enumerate() {
            line.object.set_time(t);
            for id in &idx[*st..] {
                let note = &mut line.notes[*id as usize];
                if let JudgeStatus::Hold(perfect, .., diff, true, _) = note.judge {
                    if let NoteKind::Hold { end_time, .. } = &note.kind {
                        if *end_time <= t {
                            note.judge = JudgeStatus::Judged;
                            let judge = if perfect { Judgement::Perfect } else { Judgement::Good };
                            judgements.push((judge, line_id, *id, Some(diff)));
                            #[cfg(feature = "play")]
                            if res.config.health_mode.is_some() {
                                res.health.on_judge(judge);
                            }
                            continue;
                        }
                    }
                }
                // TODO adjust
                let ghost_t = t + self.limit_good;
                if matches!(note.kind, NoteKind::Click) {
                    if ghost_t < note.time {
                        break;
                    }
                } else if t < note.time {
                    continue;
                }
                if matches!(note.judge, JudgeStatus::PreJudge) {
                    let diff = if let JudgeStatus::Hold(.., diff, _, _) = note.judge {
                        Some(diff)
                    } else {
                        None
                    };
                    note.judge = JudgeStatus::Judged;
                    if !matches!(note.kind, NoteKind::Click) {
                        judgements.push((Judgement::Perfect, line_id, *id, diff));
                        #[cfg(feature = "play")]
                        if res.config.health_mode.is_some() {
                            res.health.on_judge(Judgement::Perfect);
                        }
                    }
                }
            }
        }
        for (judgement, line_id, id, diff) in judgements {
            let line = &mut chart.lines[line_id];
            let note = &mut line.notes[id as usize];
            line.object.set_time(t);
            note.object.set_time(t);
            let line = &chart.lines[line_id];
            let note = &line.notes[id as usize];
            let mut note_transform = note.object.now(res);
            if !note.above {
                note_transform.append_nonuniform_scaling_mut(&Vector::new(1.0, -1.0));
            }
            let line_tr = line.now_transform(res, &chart.lines);
            self.commit(
                t,
                judgement,
                line_id as _,
                id,
                if matches!(judgement, Judgement::Miss) {
                    0.25
                } else if matches!(note.kind, NoteKind::Drag | NoteKind::Flick) {
                    0.
                } else {
                    (diff.unwrap_or(t) - note.time) / spd
                },
            );
            if matches!(note.kind, NoteKind::Hold { .. }) {
                continue;
            }
            if match judgement {
                Judgement::Perfect => {
                    let color = if let Some(color) = note.hit_fx_color.now_opt() {
                        color
                    } else {
                        res.res_pack.info.fx_perfect()
                    };
                    res.with_model(line_tr * note_transform, |res| res.emit_at_origin(note.rotation(line), color));
                    true
                }
                Judgement::Good => {
                    let color = if let Some(color) = note.hit_fx_color.now_opt() {
                        color
                    } else {
                        res.res_pack.info.fx_good()
                    };
                    res.with_model(line_tr * note_transform, |res| res.emit_at_origin(note.rotation(line), color));
                    true
                }
                Judgement::Bad => {
                    if !matches!(note.kind, NoteKind::Hold { .. }) {
                        bad_notes.push(BadNote {
                            time: t,
                            kind: note.kind.clone(),
                            matrix: {
                                let incline_sin = line.incline.now_opt().map(|it| it.to_radians().sin()).unwrap_or_default();
                                let mut note_transform = note.now_transform(
                                    res,
                                    &line.ctrl_obj.borrow_mut(),
                                    (note.height - line.height.now()) as f32 / res.aspect_ratio * note.speed as f32,
                                    incline_sin,
                                    true, true
                                );
                                if !note.above {
                                    note_transform.append_nonuniform_scaling_mut(&Vector::new(1.0, -1.0));
                                }
                                line_tr * note_transform
                            },
                        });
                    }
                    false
                }
                _ => false,
            } {
                note.hitsound.play(res);
            }
        }
        for (line, (idx, st)) in chart.lines.iter().zip(self.notes.iter_mut()) {
            while idx
                .get(*st)
                .is_some_and(|id| matches!(line.notes[*id as usize].judge, JudgeStatus::Judged))
            {
                *st += 1;
            }
        }
        self.last_time = t / spd;
    }

    fn auto_play_update(&mut self, res: &mut Resource, chart: &mut Chart) {
        let t = res.time - res.config.autoplay_judge_offset;
        let (judge_type, judge_type_hold, judge_time, fx_color) = if res.config.all_bad {
            (Judgement::Bad, Judgement::Good, self.limit_bad, Color::new(0., 0., 0., 0.))
        } else if res.config.all_good {
            (Judgement::Good, Judgement::Good, self.limit_good, res.res_pack.info.fx_good())
        } else {
            (Judgement::Perfect, Judgement::Perfect, 0., res.res_pack.info.fx_perfect())
        };
        //let spd = res.config.speed;
        let mut judgements = Vec::new();
        for (line_id, (line, (idx, st))) in chart.lines.iter_mut().zip(self.notes.iter_mut()).enumerate() {
            for id in &idx[*st..] {
                let note = &mut line.notes[*id as usize];
                if let JudgeStatus::Hold(..) = note.judge {
                    if let NoteKind::Hold { end_time, .. } = note.kind {
                        if t >= end_time {
                            note.judge = JudgeStatus::Judged;
                            judgements.push((line_id, *id));
                            #[cfg(feature = "play")]
                            if res.config.health_mode.is_some() {
                                res.health.on_judge(Judgement::Perfect);
                            }
                            continue;
                        }
                    }
                }
                if !matches!(note.judge, JudgeStatus::NotJudged) {
                    continue;
                }
                if note.time > t {
                    break;
                }
                note.judge = if matches!(note.kind, NoteKind::Hold { .. }) {
                    if note.time >= res.config.play_start_time && !res.disable_hit_fx {
                        note.hitsound.play(res);
                    }
                    // self.judgements.borrow_mut().push((t, line_id as _, *id, Err(true)));
                    // JudgeStatus::Hold(true, t, (t - note.time) / spd, false, f32::INFINITY)
                    JudgeStatus::Hold(true, t, judge_time, true, f64::INFINITY)
                } else {
                    judgements.push((line_id, *id));
                    #[cfg(feature = "play")]
                    if res.config.health_mode.is_some() {
                        res.health.on_judge(Judgement::Perfect);
                    }
                    JudgeStatus::Judged
                };
            }
            while idx
                .get(*st)
                .is_some_and(|id| matches!(line.notes[*id as usize].judge, JudgeStatus::Judged))
            {
                *st += 1;
            }
        }
        for (line_id, id) in judgements.into_iter() {
            let line = &chart.lines[line_id];
            let note = &line.notes[id as usize];
            match note.kind {
                NoteKind::Hold { .. } => {
                    self.inner.commit_diff(judge_type_hold);
                }
                _ => {
                    self.inner.commit_diff(judge_type);
                    if note.time >= res.config.play_start_time && !res.disable_hit_fx {
                        let mut note_transform = {
                            // let nt = if matches!(note.kind, NoteKind::Hold { .. }) { t } else { note.time };
                            let nt = note.time;
                            chart.lines[line_id].object.set_time(nt);
                            chart.lines[line_id].notes[id as usize].object.set_time(nt);
                            chart.lines[line_id].notes[id as usize].object.now(res)
                        };
                        let line = &chart.lines[line_id];
                        let note = &line.notes[id as usize];
                        if !note.above {
                            note_transform.append_nonuniform_scaling_mut(&Vector::new(1.0, -1.0));
                        }
                        let color = if let Some(color) = note.hit_fx_color.now_opt() {
                            color
                        } else {
                            if matches!(note.kind, NoteKind::Click { .. }) { fx_color } else { res.res_pack.info.fx_perfect() }
                        };
                        res.with_model(line.now_transform(res, &chart.lines) * note_transform, |res| {
                            res.emit_at_origin(note.rotation(&line), color)
                        });
                        if !(matches!(note.kind, NoteKind::Click { .. }) && res.config.all_bad) {
                            note.hitsound.play(res)
                        }
                    }
                },
            };
        }
    }

    pub fn commit_all(&mut self, chart: &mut Chart) {
        for _ in chart.lines.iter()
            .flat_map(|it| it.notes.iter())
            .filter(|it| !it.fake && matches!(it.judge, JudgeStatus::NotJudged | JudgeStatus::PreJudge))
        {
            self.inner.commit_diff(Judgement::Perfect);
        }
    }

    #[inline]
    pub fn result(&self, track_complete: bool) -> PlayResult {
        self.inner.result(track_complete)
    }

    #[inline]
    pub fn combo(&self) -> u32 {
        self.inner.combo()
    }

    #[inline]
    pub fn counts(&self) -> [u32; 4] {
        self.inner.counts()
    }

    #[inline]
    pub fn is_vaild(&self) -> bool {
        self.inner.is_vaild()
    }
}

#[derive(Default)]
struct TouchStatus {
    touches: Vec<Touch>,
    key_delta: i32,
    keys_down: u32,
}

struct Handler {
    status: TouchStatus,
    wheel: (f32, f32),
}
impl Handler {
    fn finalize(&mut self) {
        if is_mouse_button_down(MouseButton::Left) {
            self.status.touches.push(Touch {
                id: button_to_id(MouseButton::Left),
                phase: TouchPhase::Moved,
                position: mouse_position().into(),
                time: f64::NEG_INFINITY,
            });
        }
    }
}

fn button_to_id(button: MouseButton) -> u64 {
    u64::MAX
        - match button {
            MouseButton::Left => 0,
            MouseButton::Middle => 1,
            MouseButton::Right => 2,
            MouseButton::Unknown => 3,
        }
}

impl EventHandler for Handler {
    fn update(&mut self) {}
    fn draw(&mut self) {}
    fn touch_event(&mut self, phase: miniquad::TouchPhase, id: u64, x: f32, y: f32, time: f64) {
        self.status.touches.push(Touch {
            id,
            phase: phase.into(),
            position: vec2(x, y),
            time,
        });
    }

    fn mouse_wheel_event(&mut self, x: f32, y: f32) {
        self.wheel.0 += x;
        self.wheel.1 += y;
    }

    fn mouse_button_down_event(&mut self, button: MouseButton, x: f32, y: f32) {
        self.status.touches.push(Touch {
            id: button_to_id(button),
            phase: TouchPhase::Started,
            position: vec2(x, y),
            time: f64::NEG_INFINITY,
        });
    }

    fn mouse_button_up_event(&mut self, button: MouseButton, x: f32, y: f32) {
        self.status.touches.push(Touch {
            id: button_to_id(button),
            phase: TouchPhase::Ended,
            position: vec2(x, y),
            time: f64::NEG_INFINITY,
        });
    }

    fn key_down_event(&mut self, _keycode: KeyCode, _keymods: miniquad::KeyMods, repeat: bool) {
        if !repeat {
            self.status.key_delta += 1;
            self.status.keys_down += 1;
        }
    }

    fn key_up_event(&mut self, _keycode: KeyCode, _keymods: miniquad::KeyMods) {
        self.status.key_delta -= 1;
    }
}

#[derive(Default, Clone)]
pub struct PlayResult {
    pub score: f64,
    pub accuracy: f64,
    pub max_combo: u32,
    pub num_of_notes: u32,
    pub counts: [u32; 4],
    pub early: u32,
    pub late: u32,
    pub std: f32,
    pub track_complete: bool,
}

pub fn icon_index(score: u32, full_combo: bool, track_complete: bool) -> usize {
    match (score, full_combo, track_complete) {
        (_, _, false) => 7,
        (x, _, _) if x >= 1000000 => 0,
        (_, true, _) => 1,
        (x, _, _) if x < 700000 => 7,
        (x, _, _) if x < 820000 => 6,
        (x, _, _) if x < 880000 => 5,
        (x, _, _) if x < 920000 => 4,
        (x, _, _) if x < 960000 => 3,
        (_, false, _) => 2,
    }
}
