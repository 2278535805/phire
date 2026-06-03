use std::borrow::Cow;

use super::{NextPage, Page, SharedState};
use crate::character::Character;
use anyhow::Result;
use macroquad::prelude::*;
use phire::{
    ext::{SafeTexture, ScaleType, semi_black, semi_white},
    ui::Ui,
};

pub struct CharacterPage {
    character: Character,
}

impl CharacterPage {
    pub fn new(character: Character) -> Self {
        Self { character }
    }
}

impl Page for CharacterPage {
    fn label(&self) -> Cow<'static, str> {
        "CHARACTER".into()
    }

    fn touch(&mut self, touch: &Touch, _s: &mut SharedState) -> Result<bool> {
        Ok(true)
    }

    fn on_back_pressed(&mut self, _s: &mut SharedState) -> bool {
        false
    }

    fn update(&mut self, _s: &mut SharedState) -> Result<()> {
        Ok(())
    }

    fn render(&mut self, ui: &mut Ui, s: &mut SharedState) -> Result<()> {
        s.render_fader(ui, |ui, c| {
            ui.fill_rect(ui.screen_rect(), semi_black(0.4 * c.a));
            if let Some(illu) = &self.character.illu {
                let r = Rect::new(
                    -self.character.illu_adjust.2 * 0.5 + self.character.illu_adjust.0 - 0.2,
                    -self.character.illu_adjust.3 * 0.5 + self.character.illu_adjust.1,
                    self.character.illu_adjust.2,
                    self.character.illu_adjust.3
                );
                ui.fill_rect(r, (Texture2D::clone(illu), r, ScaleType::CropCenter, c));
            }

            let name = self.character.name();
            let intro = self.character.intro();
            let skill = self.character.skill();
            let illustrator = &self.character.illustrator;

            let info_x = -0.6;
            let top = -ui.top;

            ui.fill_rect(Rect::new(-0.7, -0.8 * top, 0.4, 0.2), semi_black(0.4 * c.a));

            // ui.text(name)
            //     .pos(info_x, 0.8 * top)
            //     .size(0.7)
            //     .anchor(1.0, 0.0)
            //     .color(Color::new(1., 1., 1., 0.8 * c.a))
            //     .draw();

            ui.text(format!("Illustrator: {}", illustrator))
                .pos(info_x, -0.83 * top)
                .size(0.35)
                .anchor(0.5, 0.0)
                .color(Color::new(1., 1., 1., 0.7 * c.a))
                .draw();

            ui.text(skill)
                .pos(info_x, -0.72 * top)
                .size(0.4)
                .anchor(0.5, 0.0)
                .color(Color::new(1., 1., 1., 0.8 * c.a))
                .multiline()
                .max_width(0.88)
                .draw();
            
            // ui.text(intro)
            //     .pos(info_x, 0.64 * top)
            //     .size(0.4)
            //     .anchor(1.0, 0.0)
            //     .color(Color::new(1., 1., 1., 0.8 * c.a))
            //     .multiline()
            //     .max_width(0.88)
            //     .draw();
        });
        Ok(())
    }
}