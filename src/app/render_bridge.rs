use super::*;
impl App {
    pub fn resize(&mut self, cols: u16, rows: u16) -> anyhow::Result<()> {
        self.terminal
            .resize(Rect::new(0, 0, cols.max(1), rows.max(1)))?;
        Ok(())
    }

    pub fn force_full_repaint(&mut self) {
        let _ = self.terminal.clear();
    }

    pub fn render(&mut self) -> anyhow::Result<Vec<u8>> {
        let account = &self.account;
        let snapshot = &self.snapshot;
        let ui = &mut self.ui;
        let commands = self.commands.specs();
        self.terminal.draw(|frame| {
            render::draw(frame, account, snapshot, ui, commands);
            render::apply_selection(frame, ui);
        })?;
        let mut output = match self.shared.take() {
            Ok(output) => output,
            Err(err) => {
                tracing::warn!(error = ?err, "terminal shared buffer read failed");
                Vec::new()
            }
        };
        for link in &self.ui.link_overlays {
            output.extend(terminal::osc8_hyperlink_at(
                link.rect,
                &link.url,
                &link.text,
                link.style,
                self.terminal_capabilities.color_mode,
            ));
        }
        if self.pending_link_open.take().is_some() {
            self.ui.banner = Some(Banner::ok(
                "Link is available through terminal hyperlink support",
            ));
        }
        if self.desired_pointer_shape != self.emitted_pointer_shape {
            output.extend(terminal::pointer_shape(self.desired_pointer_shape.as_str()));
            self.emitted_pointer_shape = self.desired_pointer_shape;
        }
        if self.ui.selection.copy_requested {
            self.ui.selection.copy_requested = false;
            if !self.ui.selection.text.is_empty() {
                output.extend(terminal::osc52_copy(&self.ui.selection.text));
                self.ui.banner = Some(Banner::ok("Selection copied"));
            }
            self.ui.selection.clear();
        }
        if let Some(text) = self.pending_clipboard_copy.take()
            && !text.is_empty()
        {
            output.extend(terminal::osc52_copy(&text));
        }
        if let Some(title) = self.terminal_title_update() {
            output.extend(title);
        }
        while let Some(notification) = self.pending_terminal_notifications.pop_front() {
            output.extend(terminal::desktop_notification(
                &notification.title,
                &notification.body,
                &notification.id,
            ));
        }
        Ok(output)
    }

    pub(crate) fn active_modal_token(&self) -> Option<(&'static str, &str)> {
        self.ui
            .banner
            .as_ref()
            .filter(|banner| banner.modal_active())
            .and_then(|banner| {
                banner
                    .text
                    .strip_prefix("Invite code:")
                    .map(|code| ("Invite code", code.trim()))
                    .or_else(|| {
                        banner
                            .text
                            .strip_prefix("Device link token:")
                            .map(|code| ("Device link token", code.trim()))
                    })
            })
            .filter(|(_, code)| !code.is_empty())
    }
}
