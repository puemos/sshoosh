use std::{
    io::{self, Write},
    sync::{Arc, Mutex},
};

use crossterm::{
    cursor,
    terminal::{self, ClearType},
};
use ratatui::{Terminal, TerminalOptions, Viewport, backend::CrosstermBackend, layout::Rect};

#[derive(Clone, Default)]
pub struct SharedBuffer {
    inner: Arc<Mutex<Vec<u8>>>,
}

impl SharedBuffer {
    pub fn take(&self) -> Vec<u8> {
        let mut guard = self.inner.lock().expect("shared terminal buffer poisoned");
        std::mem::take(&mut *guard)
    }
}

impl Write for SharedBuffer {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut guard = self.inner.lock().expect("shared terminal buffer poisoned");
        guard.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub type SshooshTerminal = Terminal<CrosstermBackend<SharedBuffer>>;

pub fn terminal(cols: u16, rows: u16) -> anyhow::Result<(SshooshTerminal, SharedBuffer)> {
    let shared = SharedBuffer::default();
    let backend = CrosstermBackend::new(shared.clone());
    let terminal = Terminal::with_options(
        backend,
        TerminalOptions {
            viewport: Viewport::Fixed(Rect::new(0, 0, cols.max(1), rows.max(1))),
        },
    )?;
    Ok((terminal, shared))
}

pub fn enter_alt_screen() -> Vec<u8> {
    let mut buf = Vec::new();
    crossterm::execute!(
        buf,
        terminal::EnterAlternateScreen,
        cursor::Hide,
        terminal::Clear(ClearType::All)
    )
    .expect("write terminal enter sequence");
    buf.extend_from_slice(b"\x1b[?2004h");
    buf
}

pub fn leave_alt_screen() -> Vec<u8> {
    let mut buf = Vec::new();
    buf.extend_from_slice(b"\x1b[?2004l\x1b]111\x1b\\");
    crossterm::execute!(buf, cursor::Show, terminal::LeaveAlternateScreen)
        .expect("write terminal leave sequence");
    buf
}
