use std::borrow::Cow;

use super::{Page, SharedState};
use crate::character::Character;
use anyhow::Result;
use macroquad::prelude::*;
use phire::{
    ext::{ScaleType, draw_text_aligned_opt_width, semi_black, semi_white},
    ui::{RectButton, Scroll, Ui},
};

const ITEM_HEIGHT: f32 = 0.10;
const FORM_ITEM_HEIGHT: f32 = 0.08;

pub struct CharacterPage {
    characters: Vec<Character>,
    expanded: Vec<bool>,
    btns: Vec<RectButton>,
    form_btns: Vec<RectButton>,
    scroll: Scroll,
}

impl CharacterPage {
    pub fn new(active_id: String, mut characters: Vec<Character>) -> Result<Self> {
        characters.retain(|c| c.visible);
        let char_count = characters.len();
        let mut expanded = vec![false; char_count];
        if let Some(pos) = characters.iter().position(|c| c.id == active_id) {
            expanded[pos] = true;
        }
        let mut page = Self {
            characters,
            expanded,
            btns: (0..char_count).map(|_| RectButton::new()).collect(),
            form_btns: Vec::new(),
            scroll: Scroll::new(),
        };
        page.rebuild_form_btns();
        Ok(page)
    }

    fn rebuild_form_btns(&mut self) {
        let count: usize = self.characters.iter().enumerate()
            .filter(|(i, c)| self.expanded[*i] && c.form_count() > 1)
            .map(|(_, c)| c.form_count())
            .sum();
        self.form_btns.resize_with(count, RectButton::new);
    }

    fn apply_character(&self, i: usize, s: &mut SharedState) {
        if let Some(character) = self.characters.get(i) {
            s.switch_character(&character.id, &character.current_form().id);
        }
    }
}

impl Page for CharacterPage {
    fn label(&self) -> Cow<'static, str> {
        "CHARACTER".into()
    }

    fn touch(&mut self, touch: &Touch, s: &mut SharedState) -> Result<bool> {
        if self.scroll.touch(touch, s.t) {
            return Ok(true);
        }
        for (i, btn) in self.btns.iter_mut().enumerate() {
            if btn.touch(touch) {
                let is_single_form = self.characters.get(i).map_or(false, |c| c.form_count() <= 1);
                if is_single_form {
                    let raw_idx = self.characters.get(i).and_then(|c| {
                        if c.form_count() != 1 { return None; }
                        c.visible_forms().next().and_then(|first_vis| c.forms.iter().position(|f| f.id == first_vis.id))
                    });
                    if let Some(raw_idx) = raw_idx {
                        self.characters[i].selected_form = raw_idx;
                    }
                    self.apply_character(i, s);
                } else {
                    self.expanded[i] = !self.expanded[i];
                    self.rebuild_form_btns();
                }
                return Ok(true);
            }
        }
        let mut vi = 0;
        for (i, character) in self.characters.iter().enumerate() {
            if !self.expanded[i] || character.form_count() <= 1 {
                continue;
            }
            let raw_indices = character.visible_forms_indices();
            for (fi, &raw_idx) in raw_indices.iter().enumerate() {
                if vi < self.form_btns.len() && self.form_btns[vi].touch(touch) {
                    if let Some(character) = self.characters.get_mut(i) {
                        character.selected_form = raw_idx;
                    }
                    self.apply_character(i, s);
                    return Ok(true);
                }
                vi += 1;
            }
        }
        Ok(true)
    }

    fn on_back_pressed(&mut self, _s: &mut SharedState) -> bool {
        false
    }

    fn update(&mut self, _s: &mut SharedState) -> Result<()> {
        self.scroll.update(_s.t);
        Ok(())
    }

    fn render(&mut self, ui: &mut Ui, s: &mut SharedState) -> Result<()> {
        let active_id = s.character.id.clone();

        s.render_fader(ui, |ui, c| {
            let top = -ui.top;
            draw_rectangle(-1., -top, 2., top * 2., Color::new(0., 0., 0., 0.4 * c.a));

            if let Some(character) = self.characters.iter().find(|ch| ch.id == active_id) {
                let form = character.current_form();
                if let Some(illu) = &form.illu {
                    let r = Rect::new(
                        form.position.0 - form.position.2 * 0.5 - 0.2,
                        form.position.1 - form.position.3 * 0.5,
                        form.position.2,
                        form.position.3,
                    );
                    ui.fill_rect(r, (Texture2D::clone(illu), r, ScaleType::Inside, c));
                }

                let info_x = -0.2;
                let info_y = -0.6 * top;
                let info_w = 0.6;
                let info_h = 0.25;

                ui.fill_rect(Rect::new(info_x - info_w * 0.5, info_y - info_h * 0.5, info_w, info_h), semi_black(0.5 * c.a));

                draw_text_aligned_opt_width(ui,
                    form.name(),
                    info_x, info_y - 0.03,
                    (0.5, 0.5),
                    0.5,
                    Color::new(1., 1., 1., 0.9 * c.a),
                    info_w
                );

                draw_text_aligned_opt_width(ui,
                    form.skill(), info_x, info_y + 0.03,
                    (0.5, 0.5),
                    0.35,
                    Color::new(1., 1., 1., 0.8 * c.a),
                    info_w
                );

                draw_text_aligned_opt_width(ui,
                    &format!("Illustrator: {}", form.illustrator),
                    info_x,
                    info_y + info_h * 0.5 - 0.01,
                    (0.5, 1.0),
                    0.25,
                    Color::new(1., 1., 1., 0.7 * c.a),
                    info_w
                );
            }

            let list_x = 0.45;
            let list_w = 0.5;
            let list_h = 2.0 * top;
            let list_r = Rect::new(list_x, ui.top, list_w, list_h);

            ui.fill_rect(list_r, semi_black(0.5 * c.a));

            let mut total_h = self.characters.len() as f32 * ITEM_HEIGHT;
            for (i, character) in self.characters.iter().enumerate() {
                if self.expanded[i] && character.form_count() > 1 {
                    total_h += character.form_count() as f32 * FORM_ITEM_HEIGHT;
                }
            }

            self.scroll.size((list_w, list_h));
            self.scroll.render(ui, |ui| {
                let mut y = top;
                let mut form_vi = 0;
                for (i, character) in self.characters.iter().enumerate() {
                    let is_expanded = self.expanded[i];
                    let is_active = character.id == active_id;
                    let has_forms = character.form_count() > 1;

                    let r = Rect::new(list_x, y, list_w, ITEM_HEIGHT);
                    let bg_color = if is_active {
                        semi_white(0.2 * c.a)
                    } else {
                        semi_black(0.0)
                    };
                    ui.fill_rect(r, bg_color);
                    self.btns[i].set(ui, r);

                    let text_color = if is_active {
                        Color::new(1., 1., 1., c.a)
                    } else {
                        Color::new(1., 1., 1., 0.7 * c.a)
                    };

                    ui.text(character.list_name())
                        .pos(list_x + 0.02, y + ITEM_HEIGHT * 0.5)
                        .anchor(0.0, 0.5)
                        .size(0.35)
                        .color(text_color)
                        .draw();

                    if has_forms {
                        ui.text(if is_expanded { "v" } else { ">" })
                            .pos(list_x + list_w - 0.05, y + ITEM_HEIGHT * 0.5)
                            .anchor(1.0, 0.5)
                            .size(0.28)
                            .color(text_color)
                            .draw();
                    }

                    y += ITEM_HEIGHT;

                    if is_expanded && has_forms {
                        let raw_indices = character.visible_forms_indices();
                        for (vi, &raw_idx) in raw_indices.iter().enumerate() {
                            let form = &character.forms[raw_idx];
                            let fr = Rect::new(list_x + 0.03, y, list_w - 0.03, FORM_ITEM_HEIGHT);
                            let form_is_active = is_active && raw_idx == character.selected_form;
                            let form_bg = if form_is_active {
                                semi_white(0.15 * c.a)
                            } else {
                                semi_black(0.0)
                            };
                            ui.fill_rect(fr, form_bg);
                            if form_vi < self.form_btns.len() {
                                self.form_btns[form_vi].set(ui, fr);
                            }
                            form_vi += 1;

                            let form_color = if form_is_active {
                                Color::new(1., 1., 1., c.a)
                            } else {
                                Color::new(1., 1., 1., 0.6 * c.a)
                            };

                            ui.text(form.name())
                                .pos(list_x + 0.06, y + FORM_ITEM_HEIGHT * 0.5)
                                .anchor(0.0, 0.5)
                                .size(0.28)
                                .color(form_color)
                                .draw();

                            y += FORM_ITEM_HEIGHT;
                        }
                    }
                }
                (list_w, total_h)
            });
        });
        Ok(())
    }
}
