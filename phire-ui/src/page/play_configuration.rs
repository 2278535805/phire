phire::tl_file!("play-config");

use super::{Page, SharedState};
use crate::{get_data, get_data_mut, save_data};
use anyhow::Result;
use macroquad::prelude::*;
use phire::{
    ext::{semi_black, RectExt},
    ui::{DRectButton, Scroll, Slider, Ui},
};
use std::borrow::Cow;

const ITEM_HEIGHT: f32 = 0.15;

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
    perfect_slider: Slider,
    good_slider: Slider,
    bad_slider: Slider,
    reset_btn: DRectButton,
}

impl PlayConfigurationPage {
    pub fn new() -> Self {
        Self {
            scroll: Scroll::new(),
            perfect_slider: Slider::new(0.001..0.2, 0.001),
            good_slider: Slider::new(0.001..0.3, 0.001),
            bad_slider: Slider::new(0.001..0.5, 0.001),
            reset_btn: DRectButton::new(),
        }
    }
}

impl Page for PlayConfigurationPage {
    fn label(&self) -> Cow<'static, str> {
        "CONFIGURATION".into()
    }

    fn exit(&mut self) -> Result<()> {
        save_data()
    }

    fn touch(&mut self, touch: &Touch, s: &mut SharedState) -> Result<bool> {
        let t = s.t;
        if self.scroll.touch(touch, t) {
            return Ok(true);
        }
        let config = &mut get_data_mut().config;
        let mut perfect = config.perfect_judgment as f32;
        if self.perfect_slider.touch(touch, t, &mut perfect).is_some() {
            config.perfect_judgment = perfect.max(0.001) as f64;
            if config.good_judgment <= config.perfect_judgment {
                config.good_judgment = config.perfect_judgment + 0.001;
            }
            if config.bad_judgment <= config.good_judgment {
                config.bad_judgment = config.good_judgment + 0.001;
            }
            return Ok(true);
        }
        let mut good = config.good_judgment as f32;
        if self.good_slider.touch(touch, t, &mut good).is_some() {
            config.good_judgment = good.max(config.perfect_judgment as f32 + 0.001) as f64;
            if config.bad_judgment <= config.good_judgment {
                config.bad_judgment = config.good_judgment + 0.001;
            }
            return Ok(true);
        }
        let mut bad = config.bad_judgment as f32;
        if self.bad_slider.touch(touch, t, &mut bad).is_some() {
            config.bad_judgment = bad.max(config.good_judgment as f32 + 0.001) as f64;
            return Ok(true);
        }
        if self.reset_btn.touch(touch, t) {
            config.perfect_judgment = 0.08;
            config.good_judgment = 0.16;
            config.bad_judgment = 0.22;
            return Ok(true);
        }
        Ok(false)
    }

    fn update(&mut self, s: &mut SharedState) -> Result<()> {
        self.scroll.update(s.t);
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

                    let config = &get_data().config;
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
                        self.bad_slider
                            .render(ui, rr, t, c, config.bad_judgment as f32, format!("{:.0}ms", config.bad_judgment * 1000.));
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
                        let r = Rect::new(cx + cw * 0.2, (ITEM_HEIGHT - 0.08) / 2., cw * 0.6, 0.08);
                        self.reset_btn.render_text(ui, r, t, c.a, tl!("reset"), 0.5, false);
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
