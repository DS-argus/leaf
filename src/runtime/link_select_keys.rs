use crate::app::App;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

pub(crate) fn handle_link_select_key_with(
    app: &mut App,
    key: &KeyEvent,
    copy: impl FnOnce(&str) -> bool,
    open: impl FnOnce(&str),
) -> bool {
    let Some(id) = app.selected_link() else {
        return false;
    };
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    match (key.code, ctrl) {
        (KeyCode::Enter, _) | (KeyCode::Char('x'), true) => app.copy_link_with(id, copy),
        (KeyCode::Char('o'), true) => app.open_link_with(id, open),
        (KeyCode::Esc, _) | (KeyCode::Char('c'), true) => {
            app.exit_link_select_mode();
        }
        (KeyCode::Char('x'), false) => app.link_select_next(),
        (KeyCode::Char('X'), _) => app.link_select_prev(),
        _ => return false,
    }
    true
}

pub(super) fn handle_link_select_key(app: &mut App, key: &KeyEvent) -> bool {
    handle_link_select_key_with(
        app,
        key,
        crate::clipboard::copy_to_clipboard,
        super::open_url_in_background,
    )
}

pub(super) fn try_link_select_entry(app: &mut App, key: &KeyEvent) -> bool {
    match key.code {
        KeyCode::Char('x' | 'X') if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.enter_link_select_mode();
            true
        }
        _ => false,
    }
}
