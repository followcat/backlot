//! Native cell decoder for owned parity captures. Never connects to any session.
use hc_vt::{CellColor, Rgb, Vt, Wide};
use serde_json::{Value, json};

fn color(color: CellColor) -> Value {
    match color {
        CellColor::Default => json!({"kind":"default"}),
        CellColor::Palette(index) => json!({"kind":"palette","index":index}),
        CellColor::Rgb(rgb) => json!({"kind":"rgb","rgb":[rgb.r,rgb.g,rgb.b]}),
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if !(3..=4).contains(&args.len()) {
        return Err("usage: parity_cells COLS ROWS CAPTURE [RESIZE_TIMELINE_JSON]".into());
    }
    let cols: u16 = args[0].parse()?;
    let rows: u16 = args[1].parse()?;
    if cols == 0 || rows == 0 {
        return Err("positive grid required".into());
    }
    let mut vt = Vt::new(cols, rows, 0);
    // Both PTYs receive these same measured host defaults, without palette
    // answers. There is no stripping of alternate-screen or teardown bytes.
    vt.set_default_colors(
        Some(Rgb {
            r: 255,
            g: 255,
            b: 255,
        }),
        Some(Rgb {
            r: 40,
            g: 44,
            b: 52,
        }),
        None,
    )?;
    vt.set_cell_px(10, 20);
    let bytes = std::fs::read(&args[2])?;
    let mut offset = 0;
    if let Some(path) = args.get(3) {
        let events: Vec<(usize, u16, u16)> = serde_json::from_slice(&std::fs::read(path)?)?;
        for (end, columns, rows) in events {
            if end < offset || end > bytes.len() || columns == 0 || rows == 0 {
                return Err("invalid resize timeline".into());
            }
            vt.feed(&bytes[offset..end]);
            vt.resize(columns, rows)?;
            offset = end;
        }
    }
    vt.feed(&bytes[offset..]);
    let frame = vt.frame(0)?;
    let lines: Vec<_>=frame.rows.iter().map(|row| {
        let cells: Vec<_>=row.cells.iter().map(|cell| {
            let fg=frame.colors.resolve(cell.fg,frame.colors.foreground);
            let bg=frame.colors.resolve(cell.bg,frame.colors.background);
            let underline = frame.colors.resolve(cell.underline_color,fg);
            json!({
                // Native empty narrow cells and written ASCII spaces are the same
                // displayed blank. Preserve wide tails and every style field.
                "text":if cell.text.is_empty() && cell.wide != Wide::SpacerTail {" "} else {cell.text.as_str()},"wide":format!("{:?}",cell.wide),
                "fg":color(cell.fg),"bg":color(cell.bg),
                "resolved_fg":[fg.r,fg.g,fg.b],"resolved_bg":[bg.r,bg.g,bg.b],
                "bold":cell.bold,"dim":cell.faint,"italic":cell.italic,
                "underline":format!("{:?}",cell.underline),"underline_color":color(cell.underline_color),
                "resolved_underline_color":[underline.r,underline.g,underline.b],
                "blink":cell.blink,"inverse":cell.inverse,"hidden":cell.invisible,
                "strike":cell.strikethrough,"overline":cell.overline,
            })
        }).collect();
        json!({"y":row.y,"cells":cells})
    }).collect();
    let result = json!({"title":vt.title()?,"host_modes": {
        "alternate":vt.mode(1049)?,"paste":vt.mode(2004)?,"focus":vt.mode(1004)?,
        "all_mouse":vt.mode(1003)?,"sgr_mouse":vt.mode(1006)?,"pixel_mouse":vt.mode(1016)?,
        "scheme":vt.mode(2031)?,"line_wrap":vt.mode(7)?,"kitty":vt.kitty_flags()?,
    },"cols":cols,"rows":rows,"cells":lines,"cursor":{
        "position":frame.cursor.position,"visible":frame.cursor.visible,
        "shape":format!("{:?}",frame.cursor.shape),"blink":frame.cursor.blinking,
    }});
    serde_json::to_writer(std::io::stdout().lock(), &result)?;
    Ok(())
}
