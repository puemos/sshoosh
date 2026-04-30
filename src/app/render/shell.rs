use super::*;
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
        .constraints([Constraint::Min(6), Constraint::Length(bottombar_height(ui))])
        .split(area);
    draw_body(frame, shell[0], snapshot, ui);
    draw_bottombar(frame, shell[1], account, snapshot, ui);
    draw_banner(frame, area, ui);
    if ui.startup_splash_active() {
        draw_startup_splash(frame, area, ui);
    }

    match ui.mode {
        UiMode::Palette => draw_palette(frame, area, centered(area, 72, 18), ui),
        UiMode::Help => draw_help(frame, area, help_modal_area(area), commands, ui),
        UiMode::ConfirmQuit => draw_confirm_quit(frame, area, centered(area, 42, 5), ui),
        UiMode::Compose if ui.composer.autocomplete.open => draw_autocomplete(frame, shell[1], ui),
        _ => {}
    }
    draw_comment_menu(frame, area, ui);
    draw_comment_delete_confirm(frame, area, ui);
}
