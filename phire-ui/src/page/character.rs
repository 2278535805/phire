use std::borrow::Cow;

use super::{NextPage, Page, SharedState};
use crate::{character::Character, get_data_mut};
use anyhow::Result;
use macroquad::prelude::*;
use phire::{
    ext::{SafeTexture, ScaleType, draw_text_aligned_opt_width, semi_black, semi_white},
    ui::{RectButton, Scroll, Ui},
};

const ITEM_HEIGHT: f32 = 0.12;

pub struct CharacterPage {
    characters: Vec<Character>,
    selected: usize,
    btns: Vec<RectButton>,
    scroll: Scroll,
}

impl CharacterPage {
    pub fn new(id: String, characters: Vec<Character>) -> Result<Self> {
        let selected = characters.iter().position(|c| c.id == id).unwrap_or(0);
        let btns = (0..characters.len()).map(|_| RectButton::new()).collect();
        Ok(Self {
            characters,
            selected,
            btns,
            scroll: Scroll::new(),
        })
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
                self.selected = i;
                s.character = self.characters[i].clone();
                get_data_mut().character_id = self.characters[i].id.clone();
                return Ok(true);
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

        s.render_fader(ui, |ui, c| {
            // ui.fill_rect(ui.screen_rect(), semi_black(0.4 * c.a));
            let top = -ui.top;
            draw_rectangle(-1., -top, 2., top * 2., Color::new(0., 0., 0., 0.4 * c.a));

            if let Some(character) = self.characters.get(self.selected) {
                if let Some(illu) = &character.illu {
                    let r = Rect::new(
                        character.illu_adjust.0 - character.illu_adjust.2 * 0.5 - 0.2,
                        character.illu_adjust.1 - character.illu_adjust.3 * 0.5,
                        character.illu_adjust.2,
                        character.illu_adjust.3,
                    );
                    ui.fill_rect(r, (Texture2D::clone(illu), r, ScaleType::CropCenter, c));
                }

                let name = character.name();
                let skill = character.skill();
                let illustrator = &character.illustrator;

                let info_x = -0.5;
                let info_y = -0.6 * top;
                let info_w = 0.5;
                let info_h = 0.2;

                ui.fill_rect(Rect::new(info_x - info_w * 0.5, info_y - info_h * 0.5, info_w, info_h), semi_black(0.5 * c.a));

                draw_text_aligned_opt_width(ui,
                    name,
                    info_x, info_y - 0.04,
                    (0.5, 0.5),
                    0.5,
                    Color::new(1., 1., 1., 0.9 * c.a),
                    info_w
                );

                draw_text_aligned_opt_width(ui,
                    skill, info_x, info_y + 0.04,
                    (0.5, 0.5),
                    0.35,
                    Color::new(1., 1., 1., 0.8 * c.a),
                    info_w
                );

                draw_text_aligned_opt_width(ui,
                    &format!("Illustrator: {}", illustrator),
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

            let content_h = self.characters.len() as f32 * ITEM_HEIGHT;
            self.scroll.size((list_w, list_h));
            self.scroll.render(ui, |ui| {
                for (i, character) in self.characters.iter().enumerate() {
                    let name = character.name();
                    let r = Rect::new(list_x, top + i as f32 * ITEM_HEIGHT, list_w, ITEM_HEIGHT);

                    let is_selected = i == self.selected;
                    let bg_color = if is_selected {
                        semi_white(0.3 * c.a)
                    } else {
                        semi_black(0.0)
                    };
                    ui.fill_rect(r, bg_color);

                    self.btns[i].set(ui, r);

                    let text_color = if is_selected {
                        Color::new(1., 1., 1., c.a)
                    } else {
                        Color::new(1., 1., 1., 0.7 * c.a)
                    };

                    ui.text(name)
                        .pos(list_x + 0.02, top + i as f32 * ITEM_HEIGHT + ITEM_HEIGHT * 0.5)
                        .anchor(0.0, 0.5)
                        .size(0.4)
                        .color(text_color)
                        .draw();
                }
                (list_w, content_h)
            });
        });
        Ok(())
    }
}