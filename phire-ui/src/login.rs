phire::tl_file!("login");

use crate::{
    client::{download_play_configurations_with_owner, Client, LoginParams, User, UserManager},
    get_data_mut,
    page::Fader,
    save_data,
};
use anyhow::Result;
use macroquad::prelude::*;
use once_cell::sync::Lazy;
use phire::{
    ext::{semi_black, RectExt},
    scene::{show_error, show_message},
    task::Task,
    ui::{DRectButton, Dialog, InlineInputBox, Ui},
};
use regex::Regex;
use std::{borrow::Cow, future::Future};

static EMAIL_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"\A[a-z0-9!#$%&'*+/=?^_‘{|}~-]+(?:\.[a-z0-9!#$%&'*+/=?^_‘{|}~-]+)*@(?:[a-z0-9](?:[a-z0-9-]*[a-z0-9])?\.)+[a-z0-9](?:[a-z0-9-]*[a-z0-9])?\z",
    )
    .unwrap()
});

fn validate_username(username: &str) -> Option<Cow<'static, str>> {
    if !(4..=12).contains(&username.chars().count()) {
        return Some(tl!("name-length-req"));
    }
    if username.chars().any(|it| it != '_' && it != '-' && !it.is_alphanumeric()) {
        return Some(tl!("name-has-illegal-char"));
    }
    None
}

pub struct Login {
    fader: Fader,
    show: bool,

    input_email: DRectButton,
    input_email_box: InlineInputBox,
    input_pwd: DRectButton,
    input_pwd_box: InlineInputBox,
    input_reg_email: DRectButton,
    input_reg_email_box: InlineInputBox,
    input_reg_name: DRectButton,
    input_reg_name_box: InlineInputBox,
    input_reg_pwd: DRectButton,
    input_reg_pwd_box: InlineInputBox,

    btn_to_reg: DRectButton,
    btn_to_login: DRectButton,
    btn_reg: DRectButton,
    btn_login: DRectButton,

    t_email: String,
    t_pwd: String,
    t_reg_email: String,
    t_reg_name: String,
    t_reg_pwd: String,

    start_time: f32,
    in_reg: bool,

    task: Option<(&'static str, Task<Result<Option<User>>>)>,
}

impl Login {
    const TIME: f32 = 0.7;

    pub fn new() -> Self {
        Self {
            fader: Fader::new().with_distance(-0.4).with_time(0.5),
            show: false,

            input_email: DRectButton::new().with_delta(-0.002),
            input_email_box: InlineInputBox::new(),
            input_pwd: DRectButton::new().with_delta(-0.002),
            input_pwd_box: InlineInputBox::new(),
            input_reg_email: DRectButton::new().with_delta(-0.002),
            input_reg_email_box: InlineInputBox::new(),
            input_reg_name: DRectButton::new().with_delta(-0.002),
            input_reg_name_box: InlineInputBox::new(),
            input_reg_pwd: DRectButton::new().with_delta(-0.002),
            input_reg_pwd_box: InlineInputBox::new(),

            btn_to_reg: DRectButton::new(),
            btn_to_login: DRectButton::new(),
            btn_reg: DRectButton::new(),
            btn_login: DRectButton::new(),

            t_email: String::new(),
            t_pwd: String::new(),
            t_reg_email: String::new(),
            t_reg_name: String::new(),
            t_reg_pwd: String::new(),

            start_time: f32::NAN,
            in_reg: false,

            task: None,
        }
    }

    #[inline]
    fn start(&mut self, desc: &'static str, future: impl Future<Output = Result<Option<User>>> + Send + 'static) {
        self.task = Some((desc, Task::new(future)));
    }

    pub fn enter(&mut self, t: f32) {
        self.fader.sub(t);
    }

    pub fn dismiss(&mut self, t: f32) {
        self.show = false;
        self.fader.back(t);
    }

    fn register(&mut self) -> Option<Cow<'static, str>> {
        let email = self.t_reg_email.clone();
        let name = self.t_reg_name.clone();
        let pwd = self.t_reg_pwd.clone();
        if let Some(error) = validate_username(&name) {
            show_message(error).error();
        }
        if !EMAIL_REGEX.is_match(&email) {
            return Some(tl!("illegal-email"));
        }
        if !(8..=32).contains(&pwd.len()) {
            return Some(tl!("pwd-length-req"));
        }
        self.start("register", async move {
            Client::register(&email, &name, &pwd).await?;
            Ok(None)
        });
        None
    }

    pub fn touch(&mut self, touch: &Touch, t: f32) -> bool {
        if self.fader.transiting() || self.task.is_some() || !self.start_time.is_nan() {
            return true;
        }
        if self.show {
            if self.input_email_box.is_active() {
                if self.input_email_box.touch(touch) {
                    self.t_email = self.input_email_box.confirm();
                }
                return true;
            }
            if self.input_pwd_box.is_active() {
                if self.input_pwd_box.touch(touch) {
                    self.t_pwd = self.input_pwd_box.confirm();
                }
                return true;
            }
            if self.input_reg_email_box.is_active() {
                if self.input_reg_email_box.touch(touch) {
                    self.t_reg_email = self.input_reg_email_box.confirm();
                }
                return true;
            }
            if self.input_reg_name_box.is_active() {
                if self.input_reg_name_box.touch(touch) {
                    self.t_reg_name = self.input_reg_name_box.confirm();
                }
                return true;
            }
            if self.input_reg_pwd_box.is_active() {
                if self.input_reg_pwd_box.touch(touch) {
                    self.t_reg_pwd = self.input_reg_pwd_box.confirm();
                }
                return true;
            }
            if !Ui::dialog_rect().contains(touch.position) && touch.phase == TouchPhase::Started {
                self.dismiss(t);
                return true;
            }
            if self.input_email.touch(touch, t) {
                self.input_email_box.activate(&self.t_email, false, false);
                return true;
            }
            if self.input_pwd.touch(touch, t) {
                self.input_pwd_box.activate(&self.t_pwd, false, true);
                return true;
            }
            if self.input_reg_email.touch(touch, t) {
                self.input_reg_email_box.activate(&self.t_reg_email, false, false);
                return true;
            }
            if self.input_reg_name.touch(touch, t) {
                self.input_reg_name_box.activate(&self.t_reg_name,false, false);
                return true;
            }
            if self.input_reg_pwd.touch(touch, t) {
                self.input_reg_pwd_box.activate(&self.t_reg_pwd, false, true);
                return true;
            }
            if self.btn_to_reg.touch(touch, t) || self.btn_to_login.touch(touch, t) {
                self.start_time = t;
                return true;
            }
            if self.btn_reg.touch(touch, t) {
                if let Some(error) = self.register() {
                    show_message(error).error();
                }
                return true;
            }
            if self.btn_login.touch(touch, t) {
                let email = self.t_email.clone();
                let pwd = self.t_pwd.clone();
                self.start("login", async move {
                    Client::login(LoginParams::Password {
                        email: &email,
                        password: &pwd,
                    })
                    .await?;
                    let me = Client::get_me().await?;
                    download_play_configurations_with_owner(me.id).await?;
                    Ok(Some(me))
                });
                return true;
            }
            return true;
        }
        false
    }

    pub fn update(&mut self, t: f32) -> Result<()> {
        if let Some(done) = self.fader.done(t) {
            self.show = !done;
        }
        if self.input_email_box.is_active() {
            let _ = self.input_email_box.update();
        }
        if self.input_pwd_box.is_active() {
            self.input_pwd_box.update();
        }
        if self.input_reg_email_box.is_active() {
            let _ = self.input_reg_email_box.update();
        }
        if self.input_reg_name_box.is_active() {
            let _ = self.input_reg_name_box.update();
        }
        if self.input_reg_pwd_box.is_active() {
            self.input_reg_pwd_box.update();
        }
        if let Some((action, task)) = &mut self.task {
            if let Some(res) = task.take() {
                match res {
                    Err(err) => show_error(err.context(tl!("action-failed", "action" => *action))),
                    Ok(user) => {
                        if let Some(user) = user {
                            UserManager::request(user.id);
                            get_data_mut().me = Some(user);
                            save_data()?;
                        }
                        self.t_pwd.clear();
                        show_message(tl!("action-success", "action" => *action)).ok();
                        if *action == "register" {
                            Dialog::simple(tl!("email-sent")).show();
                            self.t_reg_email.clear();
                            self.t_reg_name.clear();
                            self.t_reg_pwd.clear();
                            self.start_time = t;
                        }
                        if *action == "login" {
                            self.dismiss(t);
                        }
                    }
                }
                self.task = None;
            }
        }
        Ok(())
    }

    pub fn render(&mut self, ui: &mut Ui, t: f32) {
        self.fader.reset();
        if self.show || self.fader.transiting() {
            let Login {
                ref mut input_email, ref mut input_email_box,
                ref mut input_pwd, ref mut input_pwd_box,
                ref mut input_reg_email, ref mut input_reg_email_box,
                ref mut input_reg_name, ref mut input_reg_name_box,
                ref mut input_reg_pwd, ref mut input_reg_pwd_box,
                ref mut btn_to_reg, ref mut btn_to_login,
                ref mut btn_reg, ref mut btn_login,
                ref t_email, ref t_pwd,
                ref t_reg_email, ref t_reg_name, ref t_reg_pwd,
                ref mut fader, ref mut show, ref mut start_time, ref mut in_reg, ..
            } = self;
            let p = if *show { 1. } else { -fader.progress(t) };
            ui.fill_rect(ui.screen_rect(), semi_black(p * 0.7));
            fader.for_sub(|f| {
                f.render(ui, t, |ui, c| {
                    let wr = Ui::dialog_rect();
                    ui.fill_path(&wr.rounded(0.02), Color { a: c.a, ..ui.background() });
                    ui.scissor(wr, |ui| {
                        let p = (if start_time.is_nan() {
                            if *in_reg { 0. } else { -1. }
                        } else {
                            let p = ((t - *start_time) / Login::TIME).clamp(0., 1.);
                            let p = 1. - (1. - p).powi(3);
                            let res = if *in_reg { -p } else { p - 1. };
                            if p >= 1. {
                                *in_reg = !*in_reg;
                                *start_time = f32::NAN;
                            }
                            res
                        }) * wr.h;
                        ui.dy(p);

                        let r = ui.text(tl!("register")).pos(wr.x + 0.045, wr.y + 0.037).size(1.1).color(c).draw();
                        let pad = 0.035;
                        let mut r = Rect::new(wr.x + pad, r.bottom() + 0.05, wr.w - pad * 2., 0.1);
                        if input_reg_email_box.is_active() {
                            input_reg_email_box.render(ui, r, c.a, &tl!("email"));
                        } else {
                            input_reg_email.render_input(ui, r, t, c.a, t_reg_email, tl!("email"), 0.62);
                        }
                        r.y += r.h + 0.02;
                        if input_reg_name_box.is_active() {
                            input_reg_name_box.render(ui, r, c.a, &tl!("username"));
                        } else {
                            input_reg_name.render_input(ui, r, t, c.a, t_reg_name, tl!("username"), 0.62);
                        }
                        r.y += r.h + 0.02;
                        if input_reg_pwd_box.is_active() {
                            input_reg_pwd_box.render(ui, r, c.a, &tl!("password"));
                        } else {
                            input_reg_pwd.render_input(ui, r, t, c.a, "•".repeat(t_reg_pwd.len()), tl!("password"), 0.62);
                        }
                        let h = 0.09;
                        let pad = 0.05;
                        let mut r = Rect::new(wr.x + pad, wr.bottom() - h - 0.04, (wr.w - pad) / 2. - pad, h);
                        btn_to_login.render_text(ui, r, t, c.a, tl!("back-login"), 0.66, false);
                        r.x += r.w + pad;
                        btn_reg.render_text(ui, r, t, c.a, tl!("register"), 0.66, false);

                        ui.dy(wr.h);
                        let r = ui.text(tl!("login")).pos(wr.x + 0.045, wr.y + 0.037).size(1.1).color(c).draw();
                        let r = ui
                            .text(tl!("login-sub"))
                            .pos(r.x + 0.006, r.bottom() + 0.032)
                            .size(0.4)
                            .color(Color { a: c.a * 0.6, ..c })
                            .draw();
                        let pad = 0.037;
                        let mut r = Rect::new(wr.x + pad, r.bottom() + 0.06, wr.w - pad * 2., 0.1);
                        if input_email_box.is_active() {
                            input_email_box.render(ui, r, c.a, &tl!("email"));
                        } else {
                            input_email.render_input(ui, r, t, c.a, t_email, tl!("email"), 0.62);
                        }
                        r.y += r.h + 0.04;
                        if input_pwd_box.is_active() {
                            input_pwd_box.render(ui, r, c.a, &tl!("password"));
                        } else {
                            input_pwd.render_input(ui, r, t, c.a, "•".repeat(t_pwd.len()), tl!("password"), 0.62);
                        }

                        let h = 0.09;
                        let pad = 0.05;
                        let mut r = Rect::new(wr.x + pad, wr.bottom() - h - 0.04, (wr.w - pad) / 2. - pad, h);
                        btn_to_reg.render_text(ui, r, t, c.a, tl!("register"), 0.66, false);
                        r.x += r.w + pad;
                        btn_login.render_text(ui, r, t, c.a, tl!("login"), 0.66, false);
                    });
                });
            });
        }
        if self.task.is_some() {
            ui.full_loading_simple(t);
        }
    }
}
