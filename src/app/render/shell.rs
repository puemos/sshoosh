pub fn draw(
    frame: &mut Frame,
    account: &Account,
    snapshot: &Snapshot,
    ui: &mut UiState,
    commands: &[CommandSpec],
) {
    let area = frame.area();
    ui.hit_map.clear();
    ui.link_overlays.clear();
    frame.render_widget(Clear, area);
    if !account.activated {
        draw_onboarding(frame, area, account, ui);
        draw_banner(frame, area, ui);
        return;
    }

    let shell = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(6),
            Constraint::Length(bottombar_height(ui)),
        ])
        .split(area);
    draw_topbar(frame, shell[0], account, snapshot, ui);
    draw_horizontal_divider(frame, shell[1], theme::BORDER);
    draw_body(frame, shell[2], snapshot, ui);
    draw_bottombar(frame, shell[3], snapshot, ui);
    draw_pane_divider_intersections(frame, area, shell[1], shell[3], bottom_separator_color(ui));
    draw_banner(frame, area, ui);

    match ui.mode {
        UiMode::Palette => draw_palette(frame, area, centered(area, 72, 18), ui),
        UiMode::Prompt => draw_prompt(frame, area, centered(area, 58, 7), ui),
        UiMode::Help => draw_help(frame, area, centered(area, 76, 20), commands, ui),
        UiMode::ConfirmQuit => draw_confirm_quit(frame, area, centered(area, 42, 5), ui),
        UiMode::Compose if ui.composer.autocomplete.open => draw_autocomplete(frame, shell[3], ui),
        _ => {}
    }
}

