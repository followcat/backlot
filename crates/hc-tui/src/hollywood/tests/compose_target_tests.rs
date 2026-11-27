//! The compose line's fixed target chip: `hollywood` in gilt by
//! default, `↩ <studio> · <role>` in teal for a reply, shrinking
//! through `↩ <role>` to `↩` while CJK text keeps whole cells.
use ratatui::layout::Rect;

use super::draw_tests::{draw, layer, lines};
use super::*;
use crate::chrome::palette::Palette;
use crate::chrome::text_editor::TextEditor;
use crate::hollywood::{LayerMode, Target};
use crate::studio::StudioKey;

fn compose_row(
    width: u16,
    reply: bool,
) -> (
    String,
    crate::hollywood::HollywoodRender,
    ratatui::buffer::Buffer,
) {
    let summary = summary();
    let palette = Palette::backlot();
    let area = Rect::new(0, 0, width, 34);
    let mut hw = layer(LayerMode::Expanded);
    hw.selected = Some(Target::Input);
    hw.editing = true;
    hw.input = TextEditor::new("状態を見て", false);
    if reply {
        hw.reply_to = Some((StudioKey::local("acme-shop"), "producer".into()));
    }
    let (buf, hits) = draw(area, Some(&summary), true, &hw, false, &palette);
    let row = lines(&buf, area)[hits.input.y as usize].clone();
    (row, hits, buf)
}

#[test]
fn compose_target_chip_renders_at_100_60_and_40_columns_with_cjk() {
    for width in [100, 60, 40] {
        let (plain_row, plain_hits, _) = compose_row(width, false);
        assert!(plain_row.contains("› hollywood │"), "{width}: {plain_row}");
        assert!(
            plain_row.replace(' ', "").contains("状態を見て"),
            "{width}: {plain_row}"
        );
        let cursor = plain_hits.cursor.expect("editing cursor");
        assert_eq!(cursor.y, plain_hits.input.y);

        let (reply_row, reply_hits, reply_buf) = compose_row(width, true);
        if width == 40 {
            assert!(reply_row.contains("↩ producer │"), "{width}: {reply_row}");
            assert!(!reply_row.contains("acme-shop"), "{width}: {reply_row}");
        } else {
            assert!(
                reply_row.contains("↩ acme-shop · producer │"),
                "{width}: {reply_row}"
            );
        }
        assert!(
            reply_row.replace(' ', "").contains("状態を見て"),
            "{width}: {reply_row}"
        );
        let x = (0..width)
            .find(|x| reply_buf[(*x, reply_hits.input.y)].symbol() == "↩")
            .unwrap();
        assert_eq!(
            reply_buf[(x, reply_hits.input.y)].fg,
            Palette::backlot().teal
        );
    }
}

#[test]
fn compose_target_chip_never_hides_the_reply_glyph() {
    let summary = summary();
    let palette = Palette::backlot();
    let area = Rect::new(0, 0, 40, 34);
    let mut hw = layer(LayerMode::Expanded);
    hw.selected = Some(Target::Input);
    hw.editing = true;
    hw.reply_to = Some((
        StudioKey::local("averyverylongstudioname"),
        "an-extremely-long-role-name".into(),
    ));
    let (buf, hits) = draw(area, Some(&summary), true, &hw, false, &palette);
    let row = lines(&buf, area)[hits.input.y as usize].clone();
    assert!(row.contains('↩'), "{row}");
}

#[test]
fn compose_target_rows_match_their_width_ladder() {
    let expected = [
        (
            100,
            "╠═╡ › hollywood │ 状 態 を 見 て                                                                      ╞═╣",
            "╠═╡ › ↩ acme-shop · producer │ 状 態 を 見 て                                                         ╞═╣",
        ),
        (
            60,
            "╠═╡ › hollywood │ 状 態 を 見 て                              ╞═╣",
            "╠═╡ › ↩ acme-shop · producer │ 状 態 を 見 て                 ╞═╣",
        ),
        (
            40,
            "╠═╡ › hollywood │ 状 態 を 見 て          ╞═╣",
            "╠═╡ › ↩ producer │ 状 態 を 見 て         ╞═╣",
        ),
    ];
    for (width, plain, reply) in expected {
        assert_eq!(compose_row(width, false).0, plain);
        assert_eq!(compose_row(width, true).0, reply);
    }
}
