phire::tl_file!("play-config");

use super::{Page, SharedState};
use crate::{
    client::{sync_active_play_config, Client},
    data::PlayConfig,
    get_data, get_data_mut, save_data,
};
use anyhow::{Context, Result};
use macroquad::prelude::*;
use phire::{
    ext::{poll_future, semi_black, semi_white, LocalTask, RectExt},
    scene::{request_input, return_input, take_input},
    ui::{LoadingParams, DRectButton, RectButton, Scroll, Slider, Ui},
};
use std::borrow::Cow;

const ITEM_HEIGHT: f32 = 0.15;
const BAD_JUDGMENT_RATIO: f64 = 1.125;

fn bad_judgment(good_judgment: f64) -> f64 {
    good_judgment * BAD_JUDGMENT_RATIO
}

fn calculate_rks_factor(perfect_ms: f64, good_ms: f64) -> f64 {
    let x = 0.8 * perfect_ms + 0.225 * good_ms;
    if x > 150. {
        0.
    } else if x > 100. {
        x * x / 7500. - 4. * x / 75. + 5.
    } else {
        let x = x - 100.;
        -x * x * x / 4e6 + 1.
    }
}

pub struct PlayConfigurationPage {
    scroll: Scroll,
    config_btns: Vec<DRectButton>,
    add_btn: DRectButton,
    title_btn: RectButton,
    perfect_slider: Slider,
    good_slider: Slider,
    delete_btn: DRectButton,
    reset_btn: DRectButton,
    save_btn: DRectButton,
    saving: bool,
    sync_task: LocalTask<Result<()>>,
}

impl PlayConfigurationPage {
    pub fn new() -> Self {
        Self {
            scroll: Scroll::new(),
            config_btns: (0..get_data().play_configs.len()).map(|_| DRectButton::new()).collect(),
            add_btn: DRectButton::new(),
            title_btn: RectButton::new(),
            perfect_slider: Slider::new(0.005..0.150, 0.001),
            good_slider: Slider::new(0.010..0.300, 0.001),
            delete_btn: DRectButton::new(),
            reset_btn: DRectButton::new(),
            save_btn: DRectButton::new(),
            saving: false,
            sync_task: None,
        }
    }
}

impl Page for PlayConfigurationPage {
    fn label(&self) -> Cow<'static, str> {
        "CONFIGURATION".into()
    }

    fn exit(&mut self) -> Result<()> {
        save_data()?;
        Ok(())
    }

    fn touch(&mut self, touch: &Touch, s: &mut SharedState) -> Result<bool> {
        let t = s.t;
        if self.scroll.touch(touch, t) {
            return Ok(true);
        }
        {
            let data = get_data_mut();
            for (i, btn) in self.config_btns.iter_mut().enumerate() {
                if btn.touch(touch, t) {
                    data.active_play_config = Some(i);
                    return Ok(true);
                }
            }
        }
        if self.add_btn.touch(touch, t) {
            let data = get_data_mut();
            let index = data.play_configs.len();
            data.play_configs.push(PlayConfig {
                name: format!("{} {index}", tl!("new-config")),
                ..PlayConfig::default()
            });
            data.active_play_config = Some(index);
            self.config_btns.push(DRectButton::new());
            return Ok(true);
        }
        if self.title_btn.touch(touch) {
            let name = get_data().active_play_config().map(|it| it.name.clone()).unwrap_or_default();
            request_input("play-config-rename", &name, tl!("rename"));
            return Ok(true);
        }
        if self.delete_btn.touch(touch, t) {
            let data = get_data_mut();
            let index = data.active_play_config.unwrap_or(0);
            let id = data.play_configs[index].id.clone();
            data.play_configs.remove(index);
            if data.play_configs.is_empty() {
                data.play_configs.push(PlayConfig::default());
                data.active_play_config = Some(0);
            } else {
                data.active_play_config = Some(index.min(data.play_configs.len() - 1));
            }
            let len = data.play_configs.len();
            self.config_btns.resize(len, DRectButton::new());
            if let Some(id) = id {
                if get_data().tokens.is_some() {
                    self.sync_task = Some(Box::pin(async move {
                        Client::delete_play_configuration(&id).await.context(tl!("delete-failed"))
                    }));
                }
            }
            return Ok(true);
        }
        if self.save_btn.touch(touch, t) {
            save_data()?;
            if get_data().tokens.is_some() {
                self.saving = true;
                self.sync_task = Some(Box::pin(async move {
                    sync_active_play_config().await.context(tl!("sync-failed")).map(|_| ())
                }));
            }
            return Ok(true);
        }
        let config = get_data_mut().active_play_config_mut().expect("no play configuration");
        let mut perfect = config.perfect_judgment as f32;
        if self.perfect_slider.touch(touch, t, &mut perfect).is_some() {
            config.perfect_judgment = perfect.max(0.001) as f64;
            if config.good_judgment <= config.perfect_judgment {
                config.good_judgment = config.perfect_judgment + 0.005;
            }
            config.bad_judgment = bad_judgment(config.good_judgment);
            return Ok(true);
        }
        let mut good = config.good_judgment as f32;
        if self.good_slider.touch(touch, t, &mut good).is_some() {
            config.good_judgment = good.max(config.perfect_judgment as f32 + 0.005) as f64;
            config.bad_judgment = bad_judgment(config.good_judgment);
            return Ok(true);
        }
        if self.reset_btn.touch(touch, t) {
            config.perfect_judgment = 0.08;
            config.good_judgment = 0.16;
            config.bad_judgment = bad_judgment(config.good_judgment);
            return Ok(true);
        }
        Ok(false)
    }

    fn update(&mut self, s: &mut SharedState) -> Result<()> {
        self.scroll.update(s.t);
        if let Some(config) = get_data_mut().active_play_config_mut() {
            config.bad_judgment = bad_judgment(config.good_judgment);
        }
        if let Some((id, text)) = take_input() {
            if id == "play-config-rename" {
                let data = get_data_mut();
                if let Some(config) = data.active_play_config_mut() {
                    if !text.trim().is_empty() {
                        config.name = text;
                    }
                }
                save_data()?;
            } else {
                return_input(id, text);
            }
        }
        if let Some(res) = self.sync_task.as_mut().and_then(|task| poll_future(task.as_mut())) {
            self.sync_task = None;
            self.saving = false;
            res?;
        }
        Ok(())
    }

    fn render(&mut self, ui: &mut Ui, s: &mut SharedState) -> Result<()> {
        let t = s.t;
        s.render_fader(ui, |ui, c| {
            let lf = -0.97;
            let mut r = ui.content_rect();
            r.w += r.x - lf;
            r.x = lf;
            ui.fill_path(&r.rounded(0.00), semi_black(0.4 * c.a));
            let r = r.feather(-0.01);
            self.scroll.size((r.w, r.h));
            ui.scope(|ui| {
                ui.dx(r.x);
                ui.dy(r.y);
                self.scroll.render(ui, |ui| {
                    let w = r.w;
                    let mut h = 0.;
                    macro_rules! item {
                        ($($b:tt)*) => {{
                            $($b)*
                            ui.dy(ITEM_HEIGHT);
                            h += ITEM_HEIGHT;
                        }}
                    }
                    let cw = w * 0.6;
                    let cx = (w - cw) / 2.;
                    let rh = ITEM_HEIGHT * 2. / 3.;
                    let rr = Rect::new(cx + cw - 0.3, (ITEM_HEIGHT - rh) / 2., 0.26, rh);

                    let data = get_data();
                    let active = data.active_play_config;
                    let play_configs = &data.play_configs;
                    {
                        let btn_h = 0.08;
                        let btn_w = cw / (play_configs.len() + 1) as f32;
                        let by = (ITEM_HEIGHT - btn_h) / 2.;
                        for (i, btn) in self.config_btns.iter_mut().enumerate() {
                            let r = Rect::new(cx + i as f32 * btn_w, by, btn_w - 0.01, btn_h);
                            let tag = if play_configs[i].id.is_some() { tl!("cloud-tag") } else { tl!("local-tag") };
                            let name = format!("{} - {}", play_configs[i].name, tag);
                            btn.render_text(ui, r, t, c.a, name.as_str(), 0.4, active == Some(i));
                        }
                        let r = Rect::new(cx + play_configs.len() as f32 * btn_w, by, btn_w - 0.01, btn_h);
                        self.add_btn.render_text(ui, r, t, c.a, tl!("add-config"), 0.4, false);
                        ui.dy(ITEM_HEIGHT);
                        h += ITEM_HEIGHT;
                    }
                    {
                        let name = active.and_then(|i| play_configs.get(i)).map(|it| {
                            let tag = if it.id.is_some() { tl!("cloud-tag") } else { tl!("local-tag") };
                            format!("{} - {}", it.name, tag)
                        });
                        let r = Rect::new(cx + cw * 0.15, (ITEM_HEIGHT - 0.1) / 2., cw * 0.7, 0.1);
                        self.title_btn.set(ui, r);
                        ui.text(name.unwrap_or_default())
                            .pos(cx + cw / 2., ITEM_HEIGHT / 2.)
                            .anchor(0.5, 0.5)
                            .no_baseline()
                            .size(0.65)
                            .color(c)
                            .draw();
                        ui.dy(ITEM_HEIGHT);
                        h += ITEM_HEIGHT;
                    }
                    let config = data.active_play_config().expect("no play configuration");
                    item! {
                        render_title(ui, c, cx, tl!("perfect"), None);
                        self.perfect_slider
                            .render(ui, rr, t, c, config.perfect_judgment as f32, format!("{:.0}ms", config.perfect_judgment * 1000.));
                    }
                    item! {
                        render_title(ui, c, cx, tl!("good"), None);
                        self.good_slider
                            .render(ui, rr, t, c, config.good_judgment as f32, format!("{:.0}ms", config.good_judgment * 1000.));
                    }
                    item! {
                        render_title(ui, c, cx, tl!("bad"), None);
                        let bad_judgment = bad_judgment(config.good_judgment);
                        ui.text(format!("{:.0}ms", bad_judgment * 1000.))
                            .pos(cx + cw - 0.04, ITEM_HEIGHT / 2.)
                            .anchor(1., 0.5)
                            .no_baseline()
                            .color(c)
                            .size(0.6)
                            .draw();
                    }
                    item! {
                        render_title(ui, c, cx, tl!("rks-factor"), Some(tl!("rks-factor-sub")));
                        let factor = calculate_rks_factor(config.perfect_judgment * 1000., config.good_judgment * 1000.);
                        ui.text(format!("{:.3}", factor))
                            .pos(cx + cw - 0.04, ITEM_HEIGHT / 2.)
                            .anchor(1., 0.5)
                            .no_baseline()
                            .size(0.55)
                            .color(c)
                            .draw();
                    }
                    item! {
                        let bh = 0.08;
                        let by = (ITEM_HEIGHT - bh) / 2.;
                        let gap = 0.02;
                        let bw = (cw - gap * 2.) / 3.;
                        self.delete_btn.render_text(ui, Rect::new(cx, by, bw, bh), t, c.a, tl!("delete"), 0.5, false);
                        self.reset_btn
                            .render_text(ui, Rect::new(cx + bw + gap, by, bw, bh), t, c.a, tl!("reset"), 0.5, false);
                        let save_r = Rect::new(cx + (bw + gap) * 2., by, bw, bh);
                        if self.saving {
                            self.save_btn.render_text(ui, save_r, t, c.a, "", 0.5, false);
                            ui.loading(
                                save_r.center().x,
                                save_r.center().y,
                                t,
                                semi_white(c.a),
                                LoadingParams { radius: 0.03, width: 0.008, ..Default::default() },
                            );
                        } else {
                            self.save_btn.render_text(ui, save_r, t, c.a, tl!("save"), 0.5, false);
                        }
                    }
                    (w, h)
                });
            });
        });
        Ok(())
    }
}

fn render_title<'a>(
    ui: &mut Ui,
    c: Color,
    x: f32,
    title: impl Into<Cow<'a, str>>,
    subtitle: Option<Cow<'a, str>>,
) -> f32 {
    const TITLE_SIZE: f32 = 0.6;
    const SUBTITLE_SIZE: f32 = 0.35;
    const LEFT: f32 = 0.06;
    const PAD: f32 = 0.01;
    const SUB_MAX_WIDTH: f32 = 1.4;
    if let Some(subtitle) = subtitle {
        let title = title.into();
        let r1 = ui.text(Cow::clone(&title)).size(TITLE_SIZE).measure();
        let r2 = ui
            .text(Cow::clone(&subtitle))
            .size(SUBTITLE_SIZE)
            .max_width(SUB_MAX_WIDTH)
            .no_baseline()
            .measure();
        let h = r1.h + PAD + r2.h;
        let r1 = ui
            .text(subtitle)
            .pos(x + LEFT, (ITEM_HEIGHT + h) / 2.)
            .anchor(0., 1.)
            .size(SUBTITLE_SIZE)
            .max_width(SUB_MAX_WIDTH)
            .color(Color { a: c.a * 0.6, ..c })
            .draw()
            .right();
        let r2 = ui
            .text(title)
            .pos(x + LEFT, (ITEM_HEIGHT - h) / 2.)
            .no_baseline()
            .size(TITLE_SIZE)
            .color(c)
            .draw()
            .right();
        r1.max(r2)
    } else {
        ui.text(title.into())
            .pos(x + LEFT, ITEM_HEIGHT / 2.)
            .anchor(0., 0.5)
            .no_baseline()
            .size(TITLE_SIZE)
            .color(c)
            .draw()
            .right()
    }
}
