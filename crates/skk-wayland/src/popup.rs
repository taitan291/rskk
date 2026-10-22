use cosmic_text::{Attrs, Buffer, Color, FontSystem, Metrics, Shaping, SwashCache};
use skk_core::CandidateWindow;

const MAX_VISIBLE: usize = 10;
const FONT_SIZE: f32 = 17.0;
const LINE_HEIGHT: u32 = 24;
const PADDING: u32 = 8;

pub struct RenderedPopup {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

pub struct PopupRenderer {
    font_system: FontSystem,
    swash_cache: SwashCache,
}

impl PopupRenderer {
    pub fn new() -> Self {
        Self {
            font_system: FontSystem::new(),
            swash_cache: SwashCache::new(),
        }
    }

    pub fn render(&mut self, candidates: &CandidateWindow) -> RenderedPopup {
        let visible = candidates.items.len().min(MAX_VISIBLE);
        let start = candidates
            .selected
            .saturating_sub(visible / 2)
            .min(candidates.items.len().saturating_sub(visible));
        let end = start + visible;
        let rows = candidates
            .items
            .iter()
            .enumerate()
            .skip(start)
            .take(end - start)
            .map(|(index, item)| {
                let key = candidates.select_keys.chars().nth(index).unwrap_or(' ');
                let selected = index == candidates.selected;
                (
                    format!("{} {key}  {item}", if selected { '›' } else { ' ' }),
                    selected,
                )
            })
            .collect::<Vec<_>>();
        let text = rows
            .iter()
            .map(|(text, _)| text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        let max_chars = rows
            .iter()
            .map(|(text, _)| text.chars().count())
            .max()
            .unwrap_or(1)
            .clamp(8, 64) as u32;
        let width = (max_chars * 18 + PADDING * 2).clamp(180, 760);
        let height = (visible as u32 * LINE_HEIGHT + PADDING * 2).max(LINE_HEIGHT + PADDING * 2);
        let pixels = vec![0; (width * height * 4) as usize];
        let mut canvas = Canvas {
            pixels,
            width,
            height,
        };
        canvas.fill_rect(0, 0, width, height, [27, 31, 43, 255]);
        for (row, (_, selected)) in rows.iter().enumerate() {
            if *selected {
                canvas.fill_rect(
                    0,
                    PADDING as i32 + row as i32 * LINE_HEIGHT as i32,
                    width,
                    LINE_HEIGHT,
                    [70, 88, 121, 255],
                );
            }
        }

        let mut buffer = Buffer::new(
            &mut self.font_system,
            Metrics::new(FONT_SIZE, LINE_HEIGHT as f32),
        );
        buffer.set_size(
            Some(width.saturating_sub(PADDING * 2) as f32),
            Some(height.saturating_sub(PADDING * 2) as f32),
        );
        buffer.set_text(&text, &Attrs::new(), Shaping::Advanced, None);
        buffer.draw(
            &mut self.font_system,
            &mut self.swash_cache,
            Color::rgb(245, 247, 252),
            |x, y, glyph_width, glyph_height, color| {
                canvas.draw_glyph(
                    x + PADDING as i32,
                    y + PADDING as i32,
                    glyph_width,
                    glyph_height,
                    color.as_rgba(),
                );
            },
        );
        RenderedPopup {
            width,
            height,
            pixels: canvas.pixels,
        }
    }
}

struct Canvas {
    pixels: Vec<u8>,
    width: u32,
    height: u32,
}

impl Canvas {
    fn fill_rect(&mut self, x: i32, y: i32, rect_width: u32, rect_height: u32, color: [u8; 4]) {
        for py in y.max(0)..(y + rect_height as i32).min(self.height as i32) {
            for px in x.max(0)..(x + rect_width as i32).min(self.width as i32) {
                let offset = ((py as u32 * self.width + px as u32) * 4) as usize;
                if let Some(pixel) = self.pixels.get_mut(offset..offset + 4) {
                    pixel.copy_from_slice(&[color[2], color[1], color[0], color[3]]);
                }
            }
        }
    }

    fn draw_glyph(&mut self, x: i32, y: i32, glyph_width: u32, glyph_height: u32, color: [u8; 4]) {
        for py in y.max(0)..(y + glyph_height as i32).min(self.height as i32) {
            for px in x.max(0)..(x + glyph_width as i32).min(self.width as i32) {
                let offset = ((py as u32 * self.width + px as u32) * 4) as usize;
                let Some(pixel) = self.pixels.get_mut(offset..offset + 4) else {
                    continue;
                };
                let alpha = u16::from(color[3]);
                let inverse = 255 - alpha;
                for channel in 0..3 {
                    let source = u16::from(color[2 - channel]);
                    let destination = u16::from(pixel[channel]);
                    pixel[channel] = ((source * alpha + destination * inverse) / 255) as u8;
                }
                pixel[3] = 255;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_candidate_list_with_selected_row() {
        let candidates = CandidateWindow {
            items: vec!["仮名".into(), "かな;読み".into()],
            selected: 1,
            select_keys: "asdfjkl".into(),
        };
        let mut renderer = PopupRenderer::new();
        let rendered = renderer.render(&candidates);
        assert_eq!(rendered.height, LINE_HEIGHT * 2 + PADDING * 2);
        assert!(rendered.width >= 180);
        assert_eq!(
            rendered.pixels.len(),
            (rendered.width * rendered.height * 4) as usize
        );
        assert!(
            rendered
                .pixels
                .as_chunks::<4>()
                .0
                .iter()
                .any(|pixel| pixel[0] == 121)
        );
    }
}
