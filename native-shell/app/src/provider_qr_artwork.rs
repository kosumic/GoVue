use std::fmt::Write;

/// GoVue's provider card keeps a four-module quiet zone around the matrix.
pub fn artwork(modules: &[String]) -> Option<String> {
    let size = modules.len();
    if !(21..=177).contains(&size)
        || modules
            .iter()
            .any(|row| row.len() != size || row.bytes().any(|cell| !matches!(cell, b'0' | b'1')))
    {
        return None;
    }
    let ink = |row: usize, col: usize| {
        modules
            .get(row)
            .and_then(|row| row.as_bytes().get(col))
            .is_some_and(|cell| *cell == b'1')
    };
    let mut path = String::with_capacity(size * size * 24);
    for row in 0..size {
        for col in 0..size {
            let finder = (row < 7 && (col < 7 || col >= size - 7)) || (col < 7 && row >= size - 7);
            if finder || !ink(row, col) {
                continue;
            }
            module_path(
                &mut path,
                (col + 4) as f32,
                (row + 4) as f32,
                [
                    row > 0 && ink(row - 1, col),
                    col + 1 < size && ink(row, col + 1),
                    row + 1 < size && ink(row + 1, col),
                    col > 0 && ink(row, col - 1),
                ],
            );
        }
    }
    let extent = (size + 8) as f32;
    let height = extent * 1.1;
    let mut svg = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="1200" height="1320" viewBox="0 0 {extent} {height}">
        <defs><linearGradient id="ink" x1="0" y1="0" x2="1" y2="1">
        <stop offset="0" stop-color="#142C42"/><stop offset="1" stop-color="#176568"/>
        </linearGradient></defs>
        <rect width="{extent}" height="{height}" rx="3" fill="white"/>
        <g fill="url(#ink)"><path d="{path}"/>"##
    );
    for (x, y) in [(4, 4), (size - 3, 4), (4, size - 3)] {
        let _ = write!(
            svg,
            r#"<rect x="{x}" y="{y}" width="7" height="7" rx="1.8"/>
            <rect x="{}" y="{}" width="5" height="5" rx="1.1" fill="white"/>
            <rect x="{}" y="{}" width="3" height="3" rx=".7"/>"#,
            x + 1,
            y + 1,
            x + 2,
            y + 2,
        );
    }
    let _ = write!(
        svg,
        r##"</g><text x="{}" y="{}" text-anchor="middle" fill="#142C42" font-family="sans-serif"
        font-size="{}" font-weight="700" letter-spacing="{}">GoVue</text></svg>"##,
        extent / 2.0,
        extent * 1.065,
        extent * 0.048,
        extent * 0.003,
    );
    Some(svg)
}

fn module_path(out: &mut String, x: f32, y: f32, [up, right, down, left]: [bool; 4]) {
    let corner = |a, b| if a || b { 0.0 } else { 0.4 };
    let top_left = corner(up, left);
    let top_right = corner(up, right);
    let bottom_right = corner(down, right);
    let bottom_left = corner(down, left);
    let x1 = x + 1.0;
    let y1 = y + 1.0;
    let _ = write!(
        out,
        "M{} {y}H{}Q{x1} {y} {x1} {}V{}Q{x1} {y1} {} {y1}H{}Q{x} {y1} {x} {}V{}Q{x} {y} {} {y}z",
        x + top_left,
        x1 - top_right,
        y + top_right,
        y1 - bottom_right,
        x1 - bottom_right,
        x + bottom_left,
        y1 - bottom_left,
        y + top_left,
        x + top_left,
    );
}
