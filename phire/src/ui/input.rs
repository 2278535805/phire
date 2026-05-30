use super::Ui;
use crate::{
    ext::RectExt,
};
use macroquad::{
    input::Touch,
    prelude::*,
    miniquad::window::{clipboard_get, clipboard_set},
};

pub struct InlineInputBox {
    buffer: String,
    rect: Rect,
    multiline: bool,

    state: State,
}

#[derive(Default)]
struct State {
    active: bool,
    cursor: usize,
    selection_anchor: Option<usize>,
    backspace_time: Option<f64>,
    last_pop_time: Option<f64>,

    left_arrow_time: Option<f64>,
    right_arrow_time: Option<f64>,
    last_cursor_time: Option<f64>,
}

impl InlineInputBox {
    pub fn new() -> Self {
        Self {
            buffer: String::new(),
            rect: Rect::new(0., 0., 0., 0.),
            multiline: false,
            state: State::default()
        }
    }

    pub fn activate(&mut self, initial: &str, multiline: bool) {
        self.state.active = true;
        self.buffer = initial.to_string();
        self.multiline = multiline;
        self.state.cursor = initial.chars().count();
        self.state.backspace_time = None;
        miniquad::window::set_ime_enabled(true);
        miniquad::window::show_keyboard(true);
    }

    pub fn is_active(&self) -> bool {
        self.state.active
    }

    pub fn cancel(&mut self) {
        self.state.active = false;
        self.buffer.clear();
        self.state.cursor = 0;
        self.state.backspace_time = None;
        miniquad::window::set_ime_enabled(false);
        miniquad::window::show_keyboard(false);
    }

    pub fn confirm(&mut self) -> String {
        self.state.active = false;
        self.state.cursor = 0;
        self.state.backspace_time = None;
        std::mem::take(&mut self.buffer)
    }

    fn byte_at(&self, char_idx: usize) -> usize {
        self.buffer.char_indices().nth(char_idx).map(|(i, _)| i).unwrap_or(self.buffer.len())
    }

    fn remove_char_at(&mut self, idx: usize) {
        let start = self.byte_at(idx);
        let end = self.byte_at(idx + 1);
        self.buffer.replace_range(start..end, "");
    }

    fn text_before(&self) -> &str {
        let end = self.byte_at(self.state.cursor);
        &self.buffer[..end]
    }

    fn selection_range(&self) -> Option<(usize, usize)> {
        self.state.selection_anchor.map(|anchor| {
            let start = anchor.min(self.state.cursor);
            let end = anchor.max(self.state.cursor);
            (start, end)
        })
    }

    fn selected_text(&self) -> Option<String> {
        self.selection_range().map(|(start, end)| {
            let start_byte = self.byte_at(start);
            let end_byte = self.byte_at(end);
            self.buffer[start_byte..end_byte].to_string()
        })
    }

    fn delete_selection(&mut self) -> bool {
        if let Some((start, end)) = self.selection_range() {
            let start_byte = self.byte_at(start);
            let end_byte = self.byte_at(end);
            self.buffer.replace_range(start_byte..end_byte, "");
            self.state.cursor = start;
            self.state.selection_anchor = None;
            true
        } else {
            false
        }
    }

    // TODO: set IME position.
    fn update_ime(&self, _cursor_screen: (f32, f32)) {
        // let dpi = miniquad::window::dpi_scale();
        // let x = cursor_screen.0 * dpi;
        // let y = cursor_screen.1 * dpi;
        // miniquad::window::set_ime_position(x as i32, y as i32);
    }

    pub fn touch(&mut self, touch: &Touch) -> bool {
        let p = touch.position;
        let in_rect = self.rect.contains(p);
        let ratio = (p.x - self.rect.x - 0.02) / (self.rect.w - 0.04) * self.buffer.chars().count() as f32;
        let cursor = (ratio.round() as usize).clamp(0, self.buffer.chars().count());
        match touch.phase {
            TouchPhase::Moved => {
                if in_rect {
                    self.state.cursor = cursor;
                }
                false
            }
            TouchPhase::Stationary | TouchPhase::Ended | TouchPhase::Cancelled => {
                false
            }
            TouchPhase::Started => {
                if in_rect {
                    self.state.cursor = cursor;
                    if !self.multiline { // TODO: multiline text selection
                        self.state.selection_anchor = Some(cursor);
                    }
                }
                !in_rect
            }
        }
    }

    pub fn update(&mut self) {
        let now = get_time();
        let ctrl = is_key_down(KeyCode::LeftControl) || is_key_down(KeyCode::RightControl);
        let shift = is_key_down(KeyCode::LeftShift) || is_key_down(KeyCode::RightShift);

        // Arrow keys
        if is_key_pressed(KeyCode::Right) {
            self.state.right_arrow_time = Some(now);
            if shift {
                if self.state.selection_anchor.is_none() {
                    self.state.selection_anchor = Some(self.state.cursor);
                }
            } else {
                self.state.selection_anchor = None;
            }
            if self.state.cursor < self.buffer.chars().count() {
                self.state.cursor += 1;
            }
        } else if let Some(arrow_time) = self.state.right_arrow_time {
            if is_key_down(KeyCode::Right) {
                if now - arrow_time > 0.5 {
                    if self.state.last_cursor_time.map_or(true, |t| now - t > 0.02) {
                        self.state.last_cursor_time = Some(now);
                        if shift {
                            if self.state.selection_anchor.is_none() {
                                self.state.selection_anchor = Some(self.state.cursor);
                            }
                        } else {
                            self.state.selection_anchor = None;
                        }
                        if self.state.cursor < self.buffer.chars().count() {
                            self.state.cursor += 1;
                        }
                    }
                }
            } else {
                self.state.right_arrow_time = None;
            }
        }
        if is_key_pressed(KeyCode::Left) {
            self.state.left_arrow_time = Some(now);
            if shift {
                if self.state.selection_anchor.is_none() {
                    self.state.selection_anchor = Some(self.state.cursor);
                }
            } else {
                self.state.selection_anchor = None;
            }
            if self.state.cursor > 0 {
                self.state.cursor -= 1;
            }
        } else if let Some(arrow_time) = self.state.left_arrow_time {
            if is_key_down(KeyCode::Left) {
                if now - arrow_time > 0.5 {
                    if self.state.last_cursor_time.map_or(true, |t| now - t > 0.02) {
                        self.state.last_cursor_time = Some(now);
                        if shift {
                            if self.state.selection_anchor.is_none() {
                                self.state.selection_anchor = Some(self.state.cursor);
                            }
                        } else {
                            self.state.selection_anchor = None;
                        }
                        if self.state.cursor > 0 {
                            self.state.cursor -= 1;
                        }
                    }
                }
            } else {
                self.state.left_arrow_time = None;
            }
        }
        if self.multiline {
            if is_key_pressed(KeyCode::Up) {
                if shift {
                    if self.state.selection_anchor.is_none() {
                        self.state.selection_anchor = Some(self.state.cursor);
                    }
                } else {
                    self.state.selection_anchor = None;
                }
                let before = self.text_before();
                if let Some(line_start) = before.rfind('\n') {
                    let col = before.len() - line_start - 1;
                    let prev_line = &before[..line_start];
                    let prev_start = prev_line.rfind('\n').map(|i| i + 1).unwrap_or(0);
                    let prev_col = col.min(line_start - prev_start);
                    let target_byte = prev_start + prev_col;
                    self.state.cursor = self.buffer.char_indices().take_while(|(i, _)| *i < target_byte).count();
                }
            }
            if is_key_pressed(KeyCode::Down) {
                if shift {
                    if self.state.selection_anchor.is_none() {
                        self.state.selection_anchor = Some(self.state.cursor);
                    }
                } else {
                    self.state.selection_anchor = None;
                }
                let before = self.text_before();
                let before_byte = self.byte_at(self.state.cursor);
                let line_start_byte = before.rfind('\n').map(|i| i + 1).unwrap_or(0);
                let col = before_byte - line_start_byte;
                let after = &self.buffer[before_byte..];
                if let Some(rel_nl) = after.find('\n') {
                    let next_line_start = before_byte + rel_nl + 1;
                    let next_line_end = self.buffer[next_line_start..].find('\n').map(|i| next_line_start + i).unwrap_or(self.buffer.chars().count());
                    let next_line_len = next_line_end - next_line_start;
                    let target_col = col.min(next_line_len);
                    self.state.cursor = self.buffer.char_indices().take_while(|(i, _)| *i < next_line_start + target_col).count();
                }
            }
        }
        if is_key_pressed(KeyCode::Home) {
            if shift {
                if self.state.selection_anchor.is_none() {
                    self.state.selection_anchor = Some(self.state.cursor);
                }
            } else {
                self.state.selection_anchor = None;
            }
            let before = self.text_before();
            self.state.cursor = before.rfind('\n').map(|i| self.buffer[..i].chars().count() + 1).unwrap_or(0);
        }
        if is_key_pressed(KeyCode::End) {
            if shift {
                if self.state.selection_anchor.is_none() {
                    self.state.selection_anchor = Some(self.state.cursor);
                }
            } else {
                self.state.selection_anchor = None;
            }
            let after_byte = self.byte_at(self.state.cursor);
            self.state.cursor = self.buffer[after_byte..].find('\n').map(|i| {
                self.buffer[..after_byte + i].chars().count()
            }).unwrap_or(self.buffer.chars().count());
        }

        // Copy/Paste/Cut
        if ctrl {
            if is_key_pressed(KeyCode::C) {
                if let Some(text) = self.selected_text() {
                    clipboard_set(&text);
                }
            }
            if is_key_pressed(KeyCode::X) {
                if let Some(text) = self.selected_text() {
                    clipboard_set(&text);
                    self.delete_selection();
                }
            }
            if is_key_pressed(KeyCode::V) {
                if let Some(text) = clipboard_get().map(|s| s.to_string()) {
                    // Delete selection first
                    self.delete_selection();
                    let byte_pos = self.byte_at(self.state.cursor);
                    self.buffer.insert_str(byte_pos, &text);
                    self.state.cursor += text.chars().count();
                }
            }
            if is_key_pressed(KeyCode::A) {
                self.state.selection_anchor = Some(0);
                self.state.cursor = self.buffer.chars().count();
            }
        }

        if is_key_pressed(KeyCode::Backspace) {
            self.state.backspace_time = Some(now);
            if !self.delete_selection() {
                if self.state.cursor > 0 {
                    self.state.cursor -= 1;
                    self.remove_char_at(self.state.cursor);
                }
            }
        } else if let Some(backspace_time) = self.state.backspace_time {
            if is_key_down(KeyCode::Backspace) {
                if now - backspace_time > 0.5 {
                    if self.state.last_pop_time.map_or(true, |t| now - t > 0.02) {
                        self.state.last_pop_time = Some(now);
                        if !self.delete_selection() {
                            if self.state.cursor > 0 {
                                self.state.cursor -= 1;
                                self.remove_char_at(self.state.cursor);
                            }
                        }
                    }
                }
            } else {
                self.state.backspace_time = None;
            }
        }

        // Delete key
        if is_key_pressed(KeyCode::Delete) {
            if !self.delete_selection() {
                if self.state.cursor < self.buffer.chars().count() {
                    self.remove_char_at(self.state.cursor);
                }
            }
        }

        // Enter key
        if is_key_pressed(KeyCode::Enter) {
            if self.multiline {
                self.delete_selection();
                let byte_pos = self.byte_at(self.state.cursor);
                self.buffer.insert(byte_pos, '\n');
                self.state.cursor += 1;
            } else {
                self.confirm();
            }
        }

        // Character input
        while let Some(ch) = get_char_pressed() {
            if !ch.is_control() {
                // Delete selection first if any
                self.delete_selection();
                let byte_pos = self.byte_at(self.state.cursor);
                self.buffer.insert(byte_pos, ch);
                self.state.cursor += 1;
            }
        }

        if is_key_pressed(KeyCode::Escape) {
            self.cancel();
        }
    }

    pub fn render(&mut self, ui: &mut Ui, rect: Rect, c: Color, placeholder: &str) {
        self.rect = ui.rect_to_global(rect);
        let bx = rect.x;
        let by = rect.y;
        let bw = rect.w;
        let bh = rect.h;

        ui.fill_path(
            &Rect::new(bx, by, bw, bh).rounded(0.008),
            Color::new(0.35, 0.5, 1.0, c.a * 0.8),
        );
        ui.fill_path(
            &Rect::new(bx + 0.002, by + 0.002, bw - 0.004, bh - 0.004).rounded(0.006),
            Color::new(0.15, 0.15, 0.18, c.a),
        );

        let text_x = bx + 0.02;
        let max_w = bw - 0.04;
        let max_h = bh - 0.04;
        let clip = Rect::new(bx + 0.002, by + 0.002, bw - 0.004, bh - 0.004);
        let saved = ui.scissor_state();
        ui.scissor(Some(clip));
        if self.buffer.is_empty() {
            let text_y = by + bh / 2.0;
            ui.text(placeholder)
                .pos(text_x, text_y)
                .anchor(0.0, 0.5)
                .no_baseline()
                .size(0.42)
                .color(Color::new(1.0, 1.0, 1.0, c.a * 0.3))
                .draw();
            let cursor_x = text_x;
            let cursor_y = by + 0.01;
            ui.fill_rect(Rect::new(cursor_x, cursor_y, 0.003, bh - 0.02), Color::new(1.0, 1.0, 1.0, c.a * 0.9));
            let (sx, sy) = ui.to_global((cursor_x, cursor_y));
            self.update_ime((sx, sy + 0.5));
        } else if self.multiline {
            let text_y = by + 0.02;
            let line_h_with_space = ui.text("0\n0").size(0.42).multiline().measure().h - ui.text("0").size(0.42).measure().h;
            let line_h = ui.text("0").size(0.42).measure().h;
            let before = self.text_before();
            let line_start = before.rfind('\n').map(|i| i + 1).unwrap_or(0);
            let cursor_line_text = &before[line_start..];
            let line_num = before.chars().filter(|c| *c == '\n').count() as f32;
            let cursor_w = ui.text(cursor_line_text).size(0.42).multiline().measure().w;
            let cursor_y = line_num * line_h_with_space;
            let full_text = ui.text(&self.buffer).size(0.42).multiline().measure();
            let text_x_adj = if full_text.w > max_w {
                let overflow = full_text.w - max_w;
                let shift = (cursor_w - max_w * 0.8).max(0.0).min(overflow);
                text_x - shift
            } else {
                text_x
            };
            let text_y_adj = if full_text.h > max_h {
                let overflow = full_text.h - max_h;
                let shift = (cursor_y - max_h * 0.8).max(0.0).min(overflow);
                text_y - shift
            } else {
                text_y
            };
            let cursor_y_adj = text_y_adj + line_num * line_h_with_space;
            ui.text(&self.buffer)
                .pos(text_x_adj, text_y_adj)
                .size(0.42)
                .color(Color::new(1.0, 1.0, 1.0, c.a))
                .multiline()
                .draw();
            let cx = text_x_adj + cursor_w;
            ui.fill_rect(Rect::new(cx, cursor_y_adj, 0.003, line_h + 0.01), Color::new(1.0, 1.0, 1.0, c.a * 0.9));
            let (sx, sy) = ui.to_global((cx, cursor_y_adj + 0.01));
            self.update_ime((sx, sy + 0.5));
        } else {
            let text_y = by + bh / 2.0;
            let before = self.text_before();
            let cursor_w = ui.text(before).size(0.42).measure().w;
            let full_w = ui.text(&self.buffer).size(0.42).measure().w;
            let text_x_adj = if full_w > max_w {
                let overflow = full_w - max_w;
                let shift = (cursor_w - max_w * 0.8).max(0.0).min(overflow);
                text_x - shift
            } else {
                text_x
            };
            // Draw selection highlight
            if let Some((sel_start, sel_end)) = self.selection_range() {
                let start_before = &self.buffer[..self.byte_at(sel_start)];
                let end_before = &self.buffer[..self.byte_at(sel_end)];
                let sel_start_w = ui.text(start_before).size(0.42).measure().w;
                let sel_end_w = ui.text(end_before).size(0.42).measure().w;
                let sel_x = text_x_adj + sel_start_w;
                let sel_w = sel_end_w - sel_start_w;
                ui.fill_rect(Rect::new(sel_x, by + 0.01, sel_w, bh - 0.02), Color::new(0.3, 0.5, 1.0, c.a * 0.3));
            }
            ui.text(&self.buffer)
                .pos(text_x_adj, text_y)
                .anchor(0.0, 0.5)
                .no_baseline()
                .size(0.42)
                .color(Color::new(1.0, 1.0, 1.0, c.a))
                .draw();
            let cx = text_x_adj + cursor_w;
            ui.fill_rect(Rect::new(cx, by + 0.01, 0.003, bh - 0.02), Color::new(1.0, 1.0, 1.0, c.a * 0.9));
            let (sx, sy) = ui.to_global((cx, by + 0.01));
            self.update_ime((sx, sy + 0.5));
        }
        ui.restore_scissor(saved);
    }
}
