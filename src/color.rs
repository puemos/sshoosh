pub(crate) fn rgb_to_xterm256(rgb: (u8, u8, u8)) -> u8 {
    let mut best = 0;
    let mut best_distance = u32::MAX;
    for index in 0..=255 {
        let candidate = xterm256_to_rgb(index);
        let distance = color_distance(rgb, candidate);
        if distance < best_distance {
            best = index;
            best_distance = distance;
        }
    }
    best
}

pub(crate) fn xterm256_to_rgb(index: u8) -> (u8, u8, u8) {
    const ANSI_16: [(u8, u8, u8); 16] = [
        (0, 0, 0),
        (128, 0, 0),
        (0, 128, 0),
        (128, 128, 0),
        (0, 0, 128),
        (128, 0, 128),
        (0, 128, 128),
        (192, 192, 192),
        (128, 128, 128),
        (255, 0, 0),
        (0, 255, 0),
        (255, 255, 0),
        (0, 0, 255),
        (255, 0, 255),
        (0, 255, 255),
        (255, 255, 255),
    ];
    match index {
        0..=15 => ANSI_16[index as usize],
        16..=231 => {
            let value = index - 16;
            let r = value / 36;
            let g = (value % 36) / 6;
            let b = value % 6;
            (
                xterm_color_cube(r),
                xterm_color_cube(g),
                xterm_color_cube(b),
            )
        }
        232..=255 => {
            let level = 8 + (index - 232) * 10;
            (level, level, level)
        }
    }
}

fn xterm_color_cube(component: u8) -> u8 {
    if component == 0 {
        0
    } else {
        55 + component * 40
    }
}

fn color_distance(left: (u8, u8, u8), right: (u8, u8, u8)) -> u32 {
    let dr = left.0 as i32 - right.0 as i32;
    let dg = left.1 as i32 - right.1 as i32;
    let db = left.2 as i32 - right.2 as i32;
    (dr * dr + dg * dg + db * db) as u32
}
