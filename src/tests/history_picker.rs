use crate::app::{history::HistoryEntry, App, AppConfig};

fn new_history_picker_app() -> App {
    App::new_with_source(
        Vec::new(),
        Vec::new(),
        AppConfig {
            filename: "picker".to_string(),
            source: String::new(),
            debug_input: false,
            watch: false,
            filepath: None,
            last_file_state: None,
        },
    )
}

fn history_entries(count: usize) -> Vec<HistoryEntry> {
    (0..count)
        .map(|idx| HistoryEntry {
            path: std::env::temp_dir().join(format!("leaf-hist-{idx:03}.md")),
        })
        .collect()
}

#[test]
fn history_picker_page_moves_jump_and_clamp_at_edges() {
    let mut app = new_history_picker_app();

    app.set_file_history_length(Some(25));
    app.install_loaded_history_picker(history_entries(25));
    assert_eq!(app.history_picker_filtered_indices().len(), 25);

    for expected in [5, 10, 15, 20, 24, 24] {
        app.move_history_picker_page_down();
        assert_eq!(app.history_picker_index(), expected);
    }

    for expected in [19, 14, 9, 4, 0, 0] {
        app.move_history_picker_page_up();
        assert_eq!(app.history_picker_index(), expected);
    }
}

#[test]
fn history_picker_uses_five_row_pages_for_short_lists() {
    let mut app = new_history_picker_app();

    app.set_file_history_length(Some(19));
    app.install_loaded_history_picker(history_entries(19));
    assert_eq!(app.history_picker_filtered_indices().len(), 19);

    for expected in [5, 10, 15, 18, 18] {
        app.move_history_picker_page_down();
        assert_eq!(app.history_picker_index(), expected);
    }

    for expected in [13, 8, 3, 0, 0] {
        app.move_history_picker_page_up();
        assert_eq!(app.history_picker_index(), expected);
    }
}
