#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Key {
    Char(char),
    Enter,
    ShiftEnter,
    Esc,
    Backspace,
    Delete,
    Tab,
    BackTab,
    Up,
    Down,
    AltUp,
    AltDown,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    Ctrl(char),
    Alt(char),
    CtrlSeq(char, char),
    Paste(String),
    Mouse(MouseEvent),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MouseEvent {
    pub kind: MouseEventKind,
    pub column: u16,
    pub row: u16,
    pub modifiers: MouseModifiers,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseEventKind {
    Down(MouseButton),
    Up(MouseButton),
    Drag(MouseButton),
    Moved,
    ScrollUp,
    ScrollDown,
    ScrollLeft,
    ScrollRight,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Middle,
    Right,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MouseModifiers {
    pub shift: bool,
    pub alt: bool,
    pub control: bool,
}

#[derive(Default)]
pub struct InputDecoder {
    pending: Vec<u8>,
    leader_ctrl_x: bool,
}

impl InputDecoder {
    pub fn push(&mut self, bytes: &[u8]) -> Vec<Key> {
        self.pending.extend_from_slice(bytes);
        let mut out = Vec::new();
        while !self.pending.is_empty() {
            let Some((used, key)) = decode_one(&self.pending) else {
                break;
            };
            self.pending.drain(..used);
            if let Some(key) = self.apply_leader(key) {
                out.push(key);
            }
        }
        out
    }

    fn apply_leader(&mut self, key: Key) -> Option<Key> {
        if self.leader_ctrl_x {
            self.leader_ctrl_x = false;
            return match key {
                Key::Char(ch) | Key::Ctrl(ch) => Some(Key::CtrlSeq('x', ch)),
                other => Some(other),
            };
        }
        if key == Key::Ctrl('x') {
            self.leader_ctrl_x = true;
            None
        } else {
            Some(key)
        }
    }
}

pub(crate) fn decode_one(bytes: &[u8]) -> Option<(usize, Key)> {
    match bytes[0] {
        b'\r' => Some((1, Key::Enter)),
        b'\n' => Some((1, Key::ShiftEnter)),
        b'\t' => Some((1, Key::Tab)),
        0x7f | 0x08 => Some((1, Key::Backspace)),
        0x01..=0x1a => Some((1, Key::Ctrl((bytes[0] - 0x01 + b'a') as char))),
        0x1b => decode_escape(bytes),
        byte if byte.is_ascii() => Some((1, Key::Char(byte as char))),
        _ => {
            let text = std::str::from_utf8(bytes).ok()?;
            let ch = text.chars().next()?;
            Some((ch.len_utf8(), Key::Char(ch)))
        }
    }
}

pub(crate) fn decode_escape(bytes: &[u8]) -> Option<(usize, Key)> {
    if bytes.len() == 1 {
        return Some((1, Key::Esc));
    }
    if matches!(bytes[1], b'\r' | b'\n') {
        return Some((2, Key::ShiftEnter));
    }
    if bytes.len() >= 4 {
        match &bytes[..4] {
            b"\x1b\x1b[A" | b"\x1b\x1bOA" => return Some((4, Key::AltUp)),
            b"\x1b\x1b[B" | b"\x1b\x1bOB" => return Some((4, Key::AltDown)),
            _ => {}
        }
    }
    if bytes[1].is_ascii_alphabetic() {
        return Some((2, Key::Alt(bytes[1] as char)));
    }
    if bytes.len() < 3 {
        return None;
    }
    match &bytes[..3] {
        b"\x1b[A" | b"\x1bOA" => Some((3, Key::Up)),
        b"\x1b[B" | b"\x1bOB" => Some((3, Key::Down)),
        b"\x1b[C" | b"\x1bOC" => Some((3, Key::Right)),
        b"\x1b[D" | b"\x1bOD" => Some((3, Key::Left)),
        b"\x1b[H" | b"\x1bOH" => Some((3, Key::Home)),
        b"\x1b[F" | b"\x1bOF" => Some((3, Key::End)),
        b"\x1b[Z" => Some((3, Key::BackTab)),
        _ => decode_csi(bytes),
    }
}

pub(crate) fn decode_csi(bytes: &[u8]) -> Option<(usize, Key)> {
    if bytes.len() >= 6 && bytes.starts_with(b"\x1b[200~") {
        let end = find_subsequence(&bytes[6..], b"\x1b[201~")?;
        let paste = String::from_utf8_lossy(&bytes[6..6 + end]).into_owned();
        return Some((6 + end + 6, Key::Paste(paste)));
    }

    if bytes.starts_with(b"\x1b[<") {
        return decode_sgr_mouse(bytes);
    }

    let end = bytes
        .iter()
        .position(|byte| matches!(byte, b'~' | b'A'..=b'Z' | b'u'))?;
    if end < 2 {
        return None;
    }
    let seq = &bytes[..=end];
    match seq {
        b"\x1b[13;2u" | b"\x1b[13;2~" | b"\x1b[27;2;13~" | b"\x1b[13;3u" | b"\x1b[13;3~"
        | b"\x1b[27;3;13~" => Some((seq.len(), Key::ShiftEnter)),
        b"\x1b[1;3A" => Some((seq.len(), Key::AltUp)),
        b"\x1b[1;3B" => Some((seq.len(), Key::AltDown)),
        b"\x1b[1~" | b"\x1b[7~" => Some((seq.len(), Key::Home)),
        b"\x1b[4~" | b"\x1b[8~" => Some((seq.len(), Key::End)),
        b"\x1b[3~" => Some((seq.len(), Key::Delete)),
        b"\x1b[5~" => Some((seq.len(), Key::PageUp)),
        b"\x1b[6~" => Some((seq.len(), Key::PageDown)),
        _ => Some((1, Key::Esc)),
    }
}

pub(crate) fn decode_sgr_mouse(bytes: &[u8]) -> Option<(usize, Key)> {
    let end = bytes.iter().position(|byte| matches!(byte, b'M' | b'm'))?;
    if end < 3 {
        return Some((end + 1, Key::Esc));
    }
    let pressed = bytes[end] == b'M';
    let seq = &bytes[3..end];
    let Ok(text) = std::str::from_utf8(seq) else {
        return Some((end + 1, Key::Esc));
    };
    let mut parts = text.split(';');
    let Some(cb) = parts.next().and_then(|part| part.parse::<u8>().ok()) else {
        return Some((end + 1, Key::Esc));
    };
    let Some(column) = parts.next().and_then(|part| part.parse::<u16>().ok()) else {
        return Some((end + 1, Key::Esc));
    };
    let Some(row) = parts.next().and_then(|part| part.parse::<u16>().ok()) else {
        return Some((end + 1, Key::Esc));
    };
    if parts.next().is_some() {
        return Some((end + 1, Key::Esc));
    }
    let Some((kind, modifiers)) = decode_mouse_cb(cb, pressed) else {
        return Some((end + 1, Key::Esc));
    };
    let column = column.saturating_sub(1);
    let row = row.saturating_sub(1);
    Some((
        end + 1,
        Key::Mouse(MouseEvent {
            kind,
            column,
            row,
            modifiers,
        }),
    ))
}

pub(crate) fn decode_mouse_cb(cb: u8, pressed: bool) -> Option<(MouseEventKind, MouseModifiers)> {
    let button_number = (cb & 0b0000_0011) | ((cb & 0b1100_0000) >> 4);
    let dragging = cb & 0b0010_0000 == 0b0010_0000;
    let kind = match (button_number, dragging, pressed) {
        (0, false, true) => MouseEventKind::Down(MouseButton::Left),
        (1, false, true) => MouseEventKind::Down(MouseButton::Middle),
        (2, false, true) => MouseEventKind::Down(MouseButton::Right),
        (0, false, false) => MouseEventKind::Up(MouseButton::Left),
        (1, false, false) => MouseEventKind::Up(MouseButton::Middle),
        (2, false, false) => MouseEventKind::Up(MouseButton::Right),
        (0, true, _) => MouseEventKind::Drag(MouseButton::Left),
        (1, true, _) => MouseEventKind::Drag(MouseButton::Middle),
        (2, true, _) => MouseEventKind::Drag(MouseButton::Right),
        (3, false, _) => MouseEventKind::Up(MouseButton::Left),
        (3, true, _) | (4, true, _) | (5, true, _) => MouseEventKind::Moved,
        (4, false, _) => MouseEventKind::ScrollUp,
        (5, false, _) => MouseEventKind::ScrollDown,
        (6, false, _) => MouseEventKind::ScrollLeft,
        (7, false, _) => MouseEventKind::ScrollRight,
        _ => return None,
    };
    Some((
        kind,
        MouseModifiers {
            shift: cb & 0b0000_0100 == 0b0000_0100,
            alt: cb & 0b0000_1000 == 0b0000_1000,
            control: cb & 0b0001_0000 == 0b0001_0000,
        },
    ))
}

pub(crate) fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_arrows_tabs_ctrl_and_leader() {
        let mut decoder = InputDecoder::default();
        assert_eq!(
            decoder.push(b"a\x1b[A\x1b[Z\x10\x18p"),
            vec![
                Key::Char('a'),
                Key::Up,
                Key::BackTab,
                Key::Ctrl('p'),
                Key::CtrlSeq('x', 'p')
            ]
        );
    }

    #[test]
    fn decodes_alt_up_down() {
        let mut decoder = InputDecoder::default();
        assert_eq!(
            decoder.push(b"\x1b[1;3A\x1b[1;3B"),
            vec![Key::AltUp, Key::AltDown]
        );

        let mut decoder = InputDecoder::default();
        assert_eq!(
            decoder.push(b"\x1b\x1b[A\x1b\x1b[B"),
            vec![Key::AltUp, Key::AltDown]
        );
    }

    #[test]
    fn decodes_bracketed_paste() {
        let mut decoder = InputDecoder::default();
        assert_eq!(
            decoder.push(b"\x1b[200~hello\r\nworld\x1b[201~"),
            vec![Key::Paste("hello\r\nworld".to_string())]
        );
    }

    #[test]
    fn decodes_carriage_return_as_enter() {
        let mut decoder = InputDecoder::default();
        assert_eq!(decoder.push(b"\r"), vec![Key::Enter]);
    }

    #[test]
    fn decodes_shift_enter_variants() {
        for seq in [
            b"\n".as_slice(),
            b"\x1b\r".as_slice(),
            b"\x1b\n".as_slice(),
            b"\x1b[13;2u".as_slice(),
            b"\x1b[13;2~".as_slice(),
            b"\x1b[27;2;13~".as_slice(),
            b"\x1b[13;3u".as_slice(),
            b"\x1b[13;3~".as_slice(),
            b"\x1b[27;3;13~".as_slice(),
        ] {
            let mut decoder = InputDecoder::default();
            assert_eq!(decoder.push(seq), vec![Key::ShiftEnter]);
        }
    }

    #[test]
    fn decodes_sgr_mouse_click_release_scroll_and_modifiers() {
        let mut decoder = InputDecoder::default();
        assert_eq!(
            decoder.push(b"\x1b[<0;11;6M\x1b[<0;11;6m\x1b[<3;8;4m\x1b[<69;3;2M\x1b[<60;4;3M"),
            vec![
                Key::Mouse(MouseEvent {
                    kind: MouseEventKind::Down(MouseButton::Left),
                    column: 10,
                    row: 5,
                    modifiers: MouseModifiers::default(),
                }),
                Key::Mouse(MouseEvent {
                    kind: MouseEventKind::Up(MouseButton::Left),
                    column: 10,
                    row: 5,
                    modifiers: MouseModifiers::default(),
                }),
                Key::Mouse(MouseEvent {
                    kind: MouseEventKind::Up(MouseButton::Left),
                    column: 7,
                    row: 3,
                    modifiers: MouseModifiers::default(),
                }),
                Key::Mouse(MouseEvent {
                    kind: MouseEventKind::ScrollDown,
                    column: 2,
                    row: 1,
                    modifiers: MouseModifiers {
                        shift: true,
                        alt: false,
                        control: false,
                    },
                }),
                Key::Mouse(MouseEvent {
                    kind: MouseEventKind::Drag(MouseButton::Left),
                    column: 3,
                    row: 2,
                    modifiers: MouseModifiers {
                        shift: true,
                        alt: true,
                        control: true,
                    },
                }),
            ]
        );
    }

    #[test]
    fn buffers_incomplete_sgr_mouse_sequence() {
        let mut decoder = InputDecoder::default();
        assert_eq!(decoder.push(b"\x1b[<0;11"), Vec::<Key>::new());
        assert_eq!(
            decoder.push(b";6M"),
            vec![Key::Mouse(MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 10,
                row: 5,
                modifiers: MouseModifiers::default(),
            })]
        );
    }
}
