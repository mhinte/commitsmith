//! The TUI's tests: what a frame contains for a given app state.

#![allow(clippy::too_many_lines)]

use super::form::*;
use super::*;

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[test]
fn window_keeps_the_cursor_visible() {
    let (display, col) = window("abcdefghij", 9, true, 5);
    assert_eq!(display, "fghij");
    assert_eq!(col, Some(4));
}

#[test]
fn an_empty_field_has_a_cursor_but_nothing_to_scroll() {
    // 0.3.1 panicked here on startup: every empty field indexed chars[0].
    assert_eq!(window("", 0, true, 40), (String::new(), Some(0)));
    assert_eq!(window("", 0, false, 40), (String::new(), None));
    // A field narrower than one cell must not divide by zero either.
    assert_eq!(window("", 0, true, 0), (String::new(), Some(0)));
}

#[test]
fn the_longest_field_still_shows_the_cursor() {
    let text = "x".repeat(200);
    let (display, col) = window(&text, 200, true, 20);
    // The cursor needs a cell of its own, so 19 characters plus the cursor
    // fill the 20 columns; nothing may overflow the field.
    let col = col.expect("cursor column");
    assert_eq!(col, text_cells(&display));
    assert!(text_cells(&display) < 20);
    assert!(display.ends_with("xxx"));
}

fn press(app: &mut App, code: KeyCode) {
    app.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
}

fn press_mod(app: &mut App, code: KeyCode, modifiers: KeyModifiers) {
    app.handle_key(KeyEvent::new(code, modifiers));
}

/// Position of the first cell of `needle` in the rendered buffer.
fn find(
    terminal: &ratatui::Terminal<ratatui::backend::TestBackend>,
    needle: &str,
) -> Option<(u16, u16)> {
    let buffer = terminal.backend().buffer();
    for y in 0..buffer.area.height {
        let row: String = (0..buffer.area.width)
            .map(|x| buffer[(x, y)].symbol())
            .collect();
        if let Some(byte) = row.find(needle) {
            let column = row[..byte].chars().count() as u16;
            return Some((column, y));
        }
    }
    None
}

#[test]
fn the_caret_sits_after_the_last_character() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let mut app = App::with_config(crate::config::Config::conventional(), Vec::new());
    press(&mut app, KeyCode::Tab);
    press(&mut app, KeyCode::Tab);
    app.handle_paste("new button");

    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    terminal.draw(|frame| draw(frame, &app)).unwrap();
    let (text_x, _) = find(&terminal, "new button").expect("subject row");
    let caret = terminal.backend().cursor_position();
    // The caret belongs one cell behind the last character, not on it.
    assert_eq!(
        caret.x,
        text_x + "new button".len() as u16,
        "caret {caret:?}"
    );
}

#[test]
fn the_caret_starts_at_the_first_value_column_when_the_field_is_empty() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let mut app = App::with_config(crate::config::Config::conventional(), Vec::new());
    press(&mut app, KeyCode::Tab);
    press(&mut app, KeyCode::Tab);

    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    terminal.draw(|frame| draw(frame, &app)).unwrap();
    let (marker_x, _) = find(&terminal, "▌").expect("active marker");
    let caret = terminal.backend().cursor_position();
    // Marker plus its trailing space, then the (empty) value.
    assert_eq!(
        caret.x,
        marker_x + 2,
        "caret {caret:?}, marker at {marker_x}"
    );
}

/// The whole screen as text, one string per row.
fn rows(terminal: &ratatui::Terminal<ratatui::backend::TestBackend>) -> Vec<String> {
    let buffer = terminal.backend().buffer();
    (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect()
        })
        .collect()
}

#[test]
fn the_warning_outside_a_repository_sits_in_the_footer() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let mut app = App::with_config(crate::config::Config::conventional(), Vec::new());
    app.set_in_repository(false);
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    terminal.draw(|frame| draw(frame, &app)).unwrap();
    let screen = rows(&terminal).join("\n");
    assert!(
        screen.contains("no git repository · conventional"),
        "{screen}"
    );
    assert!(
        screen.contains("not a git repository · `check` works here"),
        "{screen}"
    );

    app.set_in_repository(true);
    app.set_repo_line("main · 0 staged".to_string());
    terminal.draw(|frame| draw(frame, &app)).unwrap();
    let screen = rows(&terminal).join("\n");
    assert!(!screen.contains("no git repository"), "{screen}");
    assert!(
        screen.contains("main · 0 staged · conventional"),
        "{screen}"
    );
}

#[test]
fn the_title_is_plain_like_the_other_panels() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let mut app = App::with_config(crate::config::Config::conventional(), Vec::new());
    app.set_in_repository(true);
    app.set_repo_line("main · 1 staged".to_string());
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    terminal.draw(|frame| draw(frame, &app)).unwrap();
    let screen = rows(&terminal);
    let title = &screen
        .iter()
        .find(|row| row.contains("compose"))
        .cloned()
        .expect("compose panel");
    // Same shape as " preview " and " checks ": the name, nothing else.
    assert!(
        title.contains("╭ compose ─") || title.contains("┬ compose ─"),
        "{title}"
    );
    // The type appears once, as the description at the other end.
    assert!(title.contains("a new feature"), "{title}");
    for gone in [
        "error",
        "warning",
        "· conventional",
        "main · 1 staged",
        "0/72",
    ] {
        assert!(
            !title.contains(gone),
            "{gone:?} still in the title: {title}"
        );
    }
}

#[test]
fn the_subject_counter_sits_next_to_the_subject() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let mut app = App::with_config(crate::config::Config::conventional(), Vec::new());
    app.form.commit.subject = "add the thing".to_string();
    app.revalidate();
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    terminal.draw(|frame| draw(frame, &app)).unwrap();
    let screen = rows(&terminal);
    let subject_row = screen
        .iter()
        .position(|line| line.contains("subject"))
        .expect("subject row");
    let counter_row = screen
        .iter()
        .position(|line| line.contains("/72"))
        .expect("counter");
    assert_eq!(counter_row, subject_row, "{screen:#?}");
    let column = screen[counter_row].find("/72").unwrap();
    assert!(column > 60, "right end of the field: {column}");
    assert!(screen[subject_row].contains("add the thing"), "{screen:#?}");
}

#[test]
fn errors_are_counted_only_in_the_checks_panel() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let app = App::with_config(crate::config::Config::conventional(), Vec::new());
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    terminal.draw(|frame| draw(frame, &app)).unwrap();
    let screen = rows(&terminal);
    let mentioning: Vec<&String> = screen
        .iter()
        .filter(|line| line.contains("error") || line.contains("warning"))
        .collect();
    // One line counts them, and it is the checks corner of the single
    // preview pane — not the header, not the footer.
    assert_eq!(mentioning.len(), 1, "{screen:#?}");
    assert!(
        mentioning[0].contains("preview") || mentioning[0].contains("1 error"),
        "{}",
        mentioning[0]
    );
}

fn branch_app() -> App {
    use crate::application::test_support::RecordingGit;

    let mut app = App::with_config(crate::config::Config::conventional(), Vec::new());
    app.set_in_repository(true);
    let git = RecordingGit::new(&["main", "feature/x"], "main");
    git.branches.borrow_mut().push({
        let mut remote = commitsmith::domain::ports::BranchInfo::remote("origin/main");
        remote.upstream = Some("origin/main".to_string());
        remote
    });
    app.set_git(Box::new(git));
    app.handle_key(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE));
    app
}

#[test]
fn the_branch_tab_lists_branches_with_the_head_marker() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let app = branch_app();
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    terminal.draw(|frame| draw(frame, &app)).unwrap();
    let screen = rows(&terminal).join("\n");

    assert!(screen.contains(" branches · 3 "), "{screen}");
    assert!(screen.contains("● main"), "HEAD marker: {screen}");
    assert!(screen.contains("○ feature/x"), "{screen}");
    assert!(screen.contains("remotes"), "remote heading: {screen}");
    assert!(screen.contains("origin/main"), "{screen}");
    assert!(screen.contains("▸ "), "selection marker: {screen}");
}

#[test]
fn the_branch_tab_shows_a_confirmation_before_deleting() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let mut app = branch_app();
    app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    app.handle_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    terminal.draw(|frame| draw(frame, &app)).unwrap();
    let screen = rows(&terminal).join("\n");
    assert!(screen.contains("press d again"), "{screen}");
    assert!(screen.contains("feature/x"), "{screen}");
}

#[test]
fn the_branch_tab_says_so_outside_a_repository() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let mut app = App::with_config(crate::config::Config::conventional(), Vec::new());
    app.handle_key(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE));
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    terminal.draw(|frame| draw(frame, &app)).unwrap();
    let screen = rows(&terminal).join("\n");
    assert!(screen.contains("no repository"), "{screen}");
}

#[test]
fn the_help_shows_the_keys_of_the_active_tab() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    // Tab 1: the composer's keys, not the explorer's actions.
    let mut app = App::with_config(crate::config::Config::conventional(), Vec::new());
    app.handle_key(KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE));
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    terminal.draw(|frame| draw(frame, &app)).unwrap();
    let screen = rows(&terminal).join("\n");
    assert!(screen.contains("keys · Commit"), "{screen}");
    assert!(screen.contains("type picker"), "{screen}");
    assert!(!screen.contains("check out"), "{screen}");

    // Tab 2: the explorer's keys, and no commit shortcut.
    let mut app = branch_app();
    app.handle_key(KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE));
    terminal.draw(|frame| draw(frame, &app)).unwrap();
    let screen = rows(&terminal).join("\n");
    assert!(screen.contains("keys · Branches"), "{screen}");
    assert!(screen.contains("check out the selected branch"), "{screen}");
    assert!(screen.contains("merge the selected branch"), "{screen}");
    assert!(!screen.contains("type picker"), "{screen}");
    assert!(!screen.contains("Ctrl+S"), "{screen}");
}

#[test]
fn every_field_and_the_whole_footer_fit() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let mut app = App::with_config(crate::config::Config::conventional(), Vec::new());
    app.form.commit.footer = "Refs TICKET-42\nCo-authored-by: hima".to_string();
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    terminal.draw(|frame| draw(frame, &app)).unwrap();
    let screen = rows(&terminal).join("\n");

    for label in ["type", "scope", "subject", "body", "breaking", "footer"] {
        assert!(screen.contains(label), "{label} missing: {screen}");
    }
    assert!(
        screen.contains("Co-authored-by: hima"),
        "the second footer line must be visible: {screen}"
    );
}

#[test]
fn the_wizard_sits_beside_the_branch_list() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let mut app = branch_app();
    app.config.author = Some("hima".to_string());
    app.config.ticket_prefix = "TICKET".to_string();
    app.config.branch_template = "{type}/{author}/{ticket}/{slug}".to_string();
    app.handle_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE));

    let mut terminal = Terminal::new(TestBackend::new(110, 30)).unwrap();
    terminal.draw(|frame| draw(frame, &app)).unwrap();
    let screen = rows(&terminal).join("\n");

    assert!(screen.contains("new branch"), "{screen}");
    assert!(
        screen.contains("branches · "),
        "the list stays visible: {screen}"
    );
    assert!(screen.contains("feat/hima/<description>"), "{screen}");
    assert!(screen.contains("description"), "{screen}");
}

/// Style of the first cell of the row containing `needle`.
fn row_style(
    terminal: &ratatui::Terminal<ratatui::backend::TestBackend>,
    needle: &str,
) -> ratatui::style::Style {
    let buffer = terminal.backend().buffer();
    for y in 0..buffer.area.height {
        let row: String = (0..buffer.area.width)
            .map(|x| buffer[(x, y)].symbol())
            .collect();
        if row.contains(needle) {
            // Any cell of the row carries the fill; the panels start at
            // different columns now.
            return (0..buffer.area.width)
                .map(|x| buffer[(x, y)].style())
                .find(|style| style.bg.is_some() && style.bg != Some(ratatui::style::Color::Reset))
                .unwrap_or_else(|| buffer[(0, y)].style());
        }
    }
    panic!("no row with {needle:?}");
}

#[test]
fn the_compose_panel_has_the_bright_border() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let app = App::with_config(crate::config::Config::conventional(), Vec::new());
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    terminal.draw(|frame| draw(frame, &app)).unwrap();
    let buffer = terminal.backend().buffer();

    let screen = rows(&terminal);
    let cyan_in =
        |y: u16| (0..buffer.area.width).any(|x| buffer[(x, y)].fg == ratatui::style::Color::Cyan);
    let compose_row = screen
        .iter()
        .position(|row| row.contains("compose"))
        .expect("compose panel");
    assert!(cyan_in(compose_row as u16), "the compose panel is lit");
    let preview_row = screen
        .iter()
        .position(|row| row.contains("preview"))
        .expect("preview panel");
    assert!(!cyan_in(preview_row as u16), "preview stays quiet");
}

#[test]
fn tab_two_lists_the_changed_files_beside_the_branches() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let (mut app, _) = staging_app_in_ui();
    app.tab = crate::application::tabs::Tab::Branches;
    let mut terminal = Terminal::new(TestBackend::new(110, 30)).unwrap();
    terminal.draw(|frame| draw(frame, &app)).unwrap();
    let screen = rows(&terminal).join("\n");
    // The files live on tab 2 now, next to the branches.
    assert!(screen.contains("files · 1"), "{screen}");
    assert!(screen.contains("a.txt"), "{screen}");
    assert!(screen.contains("files"), "{screen}");
}

fn staging_app_in_ui() -> (App, ()) {
    use crate::application::test_support::RecordingGit;
    use commitsmith::domain::ports::{ChangedFile, FileState};

    let git = RecordingGit::new(&["main"], "main");
    git.files
        .borrow_mut()
        .push(ChangedFile::new("a.txt", None, Some(FileState::Modified)));
    let mut app = App::with_config(crate::config::Config::conventional(), Vec::new());
    app.set_in_repository(true);
    app.set_git(Box::new(git));
    (app, ())
}

#[test]
fn the_focused_field_is_filled_in_the_composer() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let mut app = App::with_config(crate::config::Config::conventional(), Vec::new());
    press(&mut app, KeyCode::Tab); // scope
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    terminal.draw(|frame| draw(frame, &app)).unwrap();

    let focused = row_style(&terminal, "scope");
    let idle = row_style(&terminal, "subject");
    assert_eq!(focused.bg, Some(theme::FOCUS_BG), "scope has the focus");
    assert_ne!(idle.bg, Some(theme::FOCUS_BG), "subject does not");

    press(&mut app, KeyCode::Down);
    terminal.draw(|frame| draw(frame, &app)).unwrap();
    assert_eq!(row_style(&terminal, "subject").bg, Some(theme::FOCUS_BG));
    assert_ne!(row_style(&terminal, "scope").bg, Some(theme::FOCUS_BG));
}

#[test]
fn the_focused_wizard_field_is_filled_and_its_panel_bright() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let mut app = branch_app();
    app.handle_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE));
    let mut terminal = Terminal::new(TestBackend::new(110, 30)).unwrap();
    terminal.draw(|frame| draw(frame, &app)).unwrap();

    assert_eq!(
        row_style(&terminal, "│ type").bg,
        Some(theme::FOCUS_BG),
        "the first wizard field is focused"
    );
    assert_ne!(row_style(&terminal, "ticket").bg, Some(theme::FOCUS_BG));

    // The panel border carries the same cyan as a focused row's label. The
    // pane can sit anywhere in the lower half, so look there instead of at a
    // fixed column.
    let buffer = terminal.backend().buffer();
    let lit = (buffer.area.height / 2..buffer.area.height).any(|y| {
        (0..buffer.area.width)
            .any(|x| buffer[(x, y)].style().fg == Some(ratatui::style::Color::Cyan))
    });
    assert!(lit, "the wizard panel has the bright border");
}

#[test]
fn the_selected_branch_is_filled_too() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let mut app = branch_app();
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    terminal.draw(|frame| draw(frame, &app)).unwrap();
    assert_eq!(row_style(&terminal, "● main").bg, Some(theme::FOCUS_BG));
    assert_ne!(
        row_style(&terminal, "○ feature/x").bg,
        Some(theme::FOCUS_BG)
    );

    app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    terminal.draw(|frame| draw(frame, &app)).unwrap();
    assert_eq!(
        row_style(&terminal, "○ feature/x").bg,
        Some(theme::FOCUS_BG)
    );
}

#[test]
fn the_wizard_calls_the_field_type_and_brackets_the_value() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let mut app = branch_app();
    app.config.author = Some("hima".to_string());
    app.handle_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE));
    let mut terminal = Terminal::new(TestBackend::new(110, 30)).unwrap();
    terminal.draw(|frame| draw(frame, &app)).unwrap();
    let screen = rows(&terminal).join("\n");

    assert!(screen.contains("type"), "same word as on tab 1: {screen}");
    assert!(
        screen.contains("feat ]") && (screen.contains("✨") || !screen.contains("✨")),
        "bracketed like tab 1: {screen}"
    );
    assert!(!screen.contains("kind"), "{screen}");
}

#[test]
fn the_wizard_previews_the_name_as_it_is_typed() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let mut app = branch_app();
    app.config.author = Some("hima".to_string());
    app.config.branch_template = "{type}/{author}/{slug}".to_string();
    app.handle_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE));
    app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    for c in "Add a thing".chars() {
        app.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
    }

    let mut terminal = Terminal::new(TestBackend::new(110, 30)).unwrap();
    terminal.draw(|frame| draw(frame, &app)).unwrap();
    let screen = rows(&terminal).join("\n");
    assert!(screen.contains("feat/hima/add-a-thing"), "{screen}");
}

#[test]
fn the_branch_tab_advertises_its_own_keys() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let app = branch_app();
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    terminal.draw(|frame| draw(frame, &app)).unwrap();
    let screen = rows(&terminal).join("\n");
    for hint in ["checkout", "delete", "merge"] {
        assert!(screen.contains(hint), "{hint} missing: {screen}");
    }
    assert!(
        !screen.contains("C-s"),
        "the composer keys do not belong here"
    );
}

#[test]
fn a_long_status_does_not_run_under_the_context() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let mut app = App::with_config(crate::config::Config::conventional(), Vec::new());
    app.set_in_repository(true);
    app.set_repo_line("feat/hima/TICKET-42/add-a-thing · 0 staged".to_string());
    app.status = "created and checked out feat/hima/TICKET-42/add-a-thing · now on the branch, working tree clean, nothing staged".to_string();

    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    terminal.draw(|frame| draw(frame, &app)).unwrap();
    let screen = rows(&terminal);
    // The context moved to the keys row; it must survive there in full.
    assert!(
        screen[28].contains("feat/hima/TICKET-42/add-a-thing · 0 staged · conventional"),
        "the context stays whole: {:?}",
        screen[28]
    );
    // The status has the second row to itself and is cut, not dropped.
    let status = &screen[29];
    assert!(status.contains("created"), "{status:?}");
    assert!(status.contains('…'), "cut, not overwritten: {status:?}");
}

#[test]
fn the_tab_bar_uses_bracketed_numbers() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let app = App::with_config(crate::config::Config::conventional(), Vec::new());
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    terminal.draw(|frame| draw(frame, &app)).unwrap();
    let top = rows(&terminal).remove(0);
    assert!(top.contains("[1] Commit"), "{top}");
    assert!(top.contains("[2] Branches"), "{top}");
    assert!(top.contains("[3] Historie"), "{top}");
}

#[test]
fn a_scope_alone_is_already_shown_in_the_preview() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let mut app = App::with_config(crate::config::Config::conventional(), Vec::new());
    press(&mut app, KeyCode::Tab); // scope
    for c in "ui".chars() {
        press(&mut app, KeyCode::Char(c));
    }

    let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
    terminal.draw(|frame| draw(frame, &app)).unwrap();
    let screen = rows(&terminal).join("\n");
    assert!(screen.contains("feat(ui)"), "{screen}");
    assert!(
        !screen.contains("start with a subject"),
        "the placeholder gives way: {screen}"
    );
}

#[test]
fn typing_a_scope_updates_the_preview_on_screen() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let mut app = App::with_config(crate::config::Config::conventional(), Vec::new());
    press(&mut app, KeyCode::Tab); // scope
    for c in "ui".chars() {
        press(&mut app, KeyCode::Char(c));
    }
    app.form.commit.subject = "add the button".to_string();

    let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
    terminal.draw(|frame| draw(frame, &app)).unwrap();
    let screen = rows(&terminal).join("\n");
    assert!(screen.contains("feat(ui): add the button"), "{screen}");
}

#[test]
fn the_version_sits_in_the_header() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let app = App::with_config(crate::config::Config::conventional(), Vec::new());
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    terminal.draw(|frame| draw(frame, &app)).unwrap();
    let screen = rows(&terminal);
    let version = format!("commitsmith {}", env!("CARGO_PKG_VERSION"));
    let row = screen
        .iter()
        .position(|line| line.contains(&version))
        .expect("version in the footer");
    assert_eq!(row, 0, "the header row: {screen:#?}");
    let column = screen[row].find(&version).unwrap() as u16;
    assert!(column > 60, "top right, not left: {column}");
}

#[test]
fn the_footer_fits_eighty_columns() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    for width in [80u16, 100, 120, 160] {
        let app = App::with_config(crate::config::Config::conventional(), Vec::new());
        let mut terminal = Terminal::new(TestBackend::new(width, 30)).unwrap();
        terminal.draw(|frame| draw(frame, &app)).unwrap();

        let buffer = terminal.backend().buffer();
        let rows: Vec<String> = (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect();
        let footer = rows
            .iter()
            .find(|row| row.contains("C-k"))
            .unwrap_or_else(|| panic!("no footer at width {width}: {rows:?}"));
        // The essentials have to be readable at every width, including the
        // last entry, which used to be cut off at 80 columns.
        for needed in ["Tab", "next", "C-k", "clear", "C-s", "?", "help"] {
            assert!(
                footer.contains(needed),
                "{needed:?} missing at {width}: {footer:?}"
            );
        }
        // The version moved to the header; the footer must not repeat it.
        assert!(
            !footer.contains("commitsmith"),
            "no version in the footer at {width}: {footer:?}"
        );
        // The context takes the right end of this row, so the optional hints
        // start later than they used to.
        if width >= 120 {
            assert!(
                footer.contains("type"),
                "a 120 column footer should reach the type hint: {footer:?}"
            );
        }
    }
}

#[test]
fn the_composer_draws_at_any_terminal_size() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let app = App::with_config(crate::config::Config::conventional(), Vec::new());
    // Empty fields, then a filled one with the cursor at the end.
    for size in [(100, 30), (80, 24), (62, 24), (61, 23), (40, 12), (20, 6)] {
        let mut terminal = Terminal::new(TestBackend::new(size.0, size.1)).unwrap();
        terminal
            .draw(|frame| draw(frame, &app))
            .expect("empty composer");
    }

    let mut filled = App::with_config(crate::config::Config::conventional(), Vec::new());
    filled.handle_paste("scope with a rather long value that needs scrolling");
    filled.handle_key(ratatui::crossterm::event::KeyEvent::new(
        ratatui::crossterm::event::KeyCode::Tab,
        ratatui::crossterm::event::KeyModifiers::NONE,
    ));
    for size in [(100, 30), (80, 24), (40, 12)] {
        let mut terminal = Terminal::new(TestBackend::new(size.0, size.1)).unwrap();
        terminal
            .draw(|frame| draw(frame, &filled))
            .expect("filled composer");
    }
}

#[test]
fn wide_characters_occupy_two_columns() {
    assert_eq!(cells('a'), 1);
    assert_eq!(cells('我'), 2);
    assert_eq!(text_cells("我我a"), 5);
    let (kept, used) = take_cells("我我我", 4);
    assert_eq!(kept, "我我");
    assert_eq!(used, 4);
}

#[test]
fn cursor_column_counts_cells_not_characters() {
    // Cursor after two CJK characters sits at column 4, not 2.
    let (display, col) = window("我我ab", 2, true, 20);
    assert_eq!(display, "我我ab");
    assert_eq!(col, Some(4));
}

#[test]
fn scrolling_uses_cells() {
    // Ten wide characters in an eight column field: the visible window
    // starts at a character boundary and the cursor stays inside.
    let (display, col) = window("我我我我我我我我我我", 10, true, 8);
    assert!(text_cells(&display) <= 8, "{display:?}");
    assert!(col.unwrap() < 8);
}

#[test]
fn wrap_field_splits_on_newlines_and_width() {
    assert_eq!(wrap_field("abc\ndef", 10), vec!["abc", "def"]);
    assert_eq!(wrap_field("abcdef", 3), vec!["abc", "def"]);
}

/// Style of the cell at the first character of `needle`.
fn style_of(terminal: &ratatui::Terminal<ratatui::backend::TestBackend>, needle: &str) -> Style {
    let (x, y) = find(terminal, needle).expect("text on screen");
    // Only the colour matters; the buffer fills in explicit defaults that a
    // hand-built style leaves unset.
    Style::new().fg(terminal.backend().buffer()[(x, y)]
        .style()
        .fg
        .unwrap_or(Color::Reset))
}

fn staging_item(path: &str) -> commitsmith::domain::ports::ChangedFile {
    commitsmith::domain::ports::ChangedFile::new(
        path,
        None,
        Some(commitsmith::domain::ports::FileState::Modified),
    )
}

#[test]
fn the_bright_border_follows_the_keyboard_to_the_file_list() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let mut app = App::with_config(crate::config::Config::conventional(), Vec::new());
    app.tab = crate::application::tabs::Tab::Branches;
    app.branches.files.items = vec![staging_item("src/main.rs")];

    let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
    terminal.draw(|frame| draw(frame, &app)).unwrap();
    let branches_before = style_of(&terminal, "branches");
    let staging_before = style_of(&terminal, "files");

    // Tab walks the panes; the bright border walks with it.
    press(&mut app, KeyCode::Tab);
    assert_eq!(app.branches.panel, crate::application::Panel::Files);
    terminal.draw(|frame| draw(frame, &app)).unwrap();
    let branches_after = style_of(&terminal, "branches");
    let staging_after = style_of(&terminal, "files");

    assert_eq!(branches_before, theme::focused_panel());
    assert_eq!(staging_before, theme::idle_title());
    assert_eq!(staging_after, theme::focused_panel());
    assert_eq!(branches_after, theme::idle_title());
}

#[test]
fn the_file_list_scrolls_to_keep_the_cursor_visible() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let mut app = App::with_config(crate::config::Config::conventional(), Vec::new());
    app.tab = crate::application::tabs::Tab::Branches;
    app.branches.files.items = (0..30)
        .map(|index| staging_item(&format!("src/file{index:02}.rs")))
        .collect();
    press_mod(&mut app, KeyCode::Char('l'), KeyModifiers::CONTROL);
    // There is no repository here, so the list cannot load a diff; the cursor
    // position is what the renderer has to follow.
    app.branches.files.selected = 25;

    // The diff halves the list; the cursor must stay inside that half.
    app.branches.files.show_diff = true;
    let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
    terminal.draw(|frame| draw(frame, &app)).unwrap();
    let screen = rows(&terminal).join("\n");
    assert!(screen.contains("file25.rs"), "the selected file is visible");
    assert!(
        !screen.contains("file00.rs"),
        "the list scrolled instead of clipping the cursor"
    );
    assert!(screen.contains("▌"), "the cursor marker is on screen");
}

#[test]
fn a_long_path_is_shortened_but_named_in_full_in_the_diff() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let path = "src/infrastructure/very/long/directory/chain/file_name.rs";
    let mut app = App::with_config(crate::config::Config::conventional(), Vec::new());
    app.tab = crate::application::tabs::Tab::Branches;
    app.branches.files.items = vec![staging_item(path)];
    app.branches.files.show_diff = true;
    app.branches.files.set_diff(String::new());

    // Narrow enough that the full path cannot fit next to the status code.
    let mut terminal = Terminal::new(TestBackend::new(80, 30)).unwrap();
    terminal.draw(|frame| draw(frame, &app)).unwrap();
    let screen = rows(&terminal).join("\n");

    // However narrow the pane, the file name itself survives; the whole path
    // lives in the footer and in the diff title.
    assert!(screen.contains("file_name.rs"), "{screen}");
}

fn past(kind: &str, scope: &str, summary: &str) -> commitsmith::domain::history::PastCommit {
    commitsmith::domain::history::PastCommit {
        short_id: "abc1234".to_string(),
        id: "abc1234".to_string(),
        author: "Marc".to_string(),
        date: "2026-11-25".to_string(),
        summary: summary.to_string(),
        body: String::new(),
        kind: Some(kind.to_string()),
        scope: Some(scope.to_string()),
        files: Vec::new(),
        merge: false,
    }
}

#[test]
fn the_log_shows_what_the_filter_keeps() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let mut app = App::with_config(crate::config::Config::conventional(), Vec::new());
    app.tab = crate::application::tabs::Tab::History;
    app.set_history(vec![
        past("feat", "ui", "add the button"),
        past("fix", "api", "repair the route"),
    ]);
    app.release.log.filter.kind = Some("feat".to_string());

    let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
    terminal.draw(|frame| draw(frame, &app)).unwrap();
    let screen = rows(&terminal).join("\n");
    assert!(screen.contains("add the button"), "{screen}");
    assert!(!screen.contains("repair the route"), "{screen}");
    assert!(screen.contains("1 of 2"), "{screen}");
}

#[test]
fn help_opens_on_the_third_tab() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let mut app = App::with_config(crate::config::Config::conventional(), Vec::new());
    app.tab = crate::application::tabs::Tab::History;
    app.set_history(vec![past("feat", "ui", "add the button")]);

    press(&mut app, KeyCode::Char('?'));
    assert!(app.show_help, "the help must open here too");
    let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
    terminal.draw(|frame| draw(frame, &app)).unwrap();
    let screen = rows(&terminal).join("\n");
    assert!(screen.contains("filter"), "{screen}");
    assert!(screen.contains("clear filter"), "{screen}");

    // Esc closes it again.
    press(&mut app, KeyCode::Esc);
    assert!(!app.show_help);
}

#[test]
fn the_version_sits_in_the_header_and_the_branch_in_the_footer() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let mut app = App::with_config(crate::config::Config::conventional(), Vec::new());
    app.set_in_repository(true);
    app.set_repo_line("main".to_string());

    let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
    terminal.draw(|frame| draw(frame, &app)).unwrap();
    let lines = rows(&terminal);
    assert!(lines[0].contains("commitsmith"), "header: {}", lines[0]);
    let footer = lines
        .iter()
        .rev()
        .take(2)
        .cloned()
        .collect::<Vec<_>>()
        .join("\n");
    assert!(footer.contains("main"), "context moved down: {footer}");
    assert!(
        !footer.contains("commitsmith"),
        "no double version: {footer}"
    );
}

#[test]
fn the_staging_title_counts_what_is_staged() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let mut app = App::with_config(crate::config::Config::conventional(), Vec::new());
    app.tab = crate::application::tabs::Tab::Branches;
    app.branches.files.items = vec![
        commitsmith::domain::ports::ChangedFile::new(
            "src/a.rs",
            Some(commitsmith::domain::ports::FileState::Modified),
            None,
        ),
        commitsmith::domain::ports::ChangedFile::new(
            "src/b.rs",
            None,
            Some(commitsmith::domain::ports::FileState::Modified),
        ),
    ];

    let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
    terminal.draw(|frame| draw(frame, &app)).unwrap();
    let screen = rows(&terminal).join("\n");
    assert!(screen.contains("files · 2 · 1 staged"), "{screen}");
}

#[test]
fn a_clean_commit_is_green_and_optional_hints_are_not_repeated() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let mut app = App::with_config(crate::config::Config::conventional(), Vec::new());
    app.tab = crate::application::tabs::Tab::History;
    app.set_history(vec![past("feat", "ui", "add the button")]);

    let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
    terminal.draw(|frame| draw(frame, &app)).unwrap();
    let screen = rows(&terminal).join("\n");
    assert!(screen.contains("details · lint clean"), "{screen}");
    assert!(!screen.contains("optional:"), "no hints here: {screen}");
}

#[test]
fn the_detail_pane_names_what_the_linter_objects_to() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let mut app = App::with_config(crate::config::Config::conventional(), Vec::new());
    app.tab = crate::application::tabs::Tab::History;
    // A subject that starts with a capital and ends with a period: two
    // findings the composer would also show.
    app.set_history(vec![past("feat", "ui", "Add the button.")]);

    let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
    terminal.draw(|frame| draw(frame, &app)).unwrap();
    let screen = rows(&terminal).join("\n");
    assert!(screen.contains("subject:"), "{screen}");
    assert!(
        screen.contains("period") || screen.contains("imperative"),
        "{screen}"
    );
}

#[test]
fn an_empty_filter_result_says_so() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let mut app = App::with_config(crate::config::Config::conventional(), Vec::new());
    app.tab = crate::application::tabs::Tab::History;
    app.set_history(vec![past("fix", "api", "repair the route")]);
    app.release.log.filter.kind = Some("feat".to_string());

    let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
    terminal.draw(|frame| draw(frame, &app)).unwrap();
    let screen = rows(&terminal).join("\n");
    assert!(screen.contains("no matches"), "{screen}");
    assert!(screen.contains("type: feat"), "{screen}");
}

#[test]
fn folding_takes_body_breaking_and_footer_off_the_screen() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let mut app = App::with_config(crate::config::Config::conventional(), Vec::new());
    let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
    terminal.draw(|frame| draw(frame, &app)).unwrap();
    let full = rows(&terminal).join("\n");
    assert!(full.contains("breaking"), "{full}");

    press_mod(&mut app, KeyCode::Char('o'), KeyModifiers::CONTROL);
    terminal.draw(|frame| draw(frame, &app)).unwrap();
    let folded = rows(&terminal).join("\n");
    // The labels leave the form; the help text and the field list may still
    // name them, so look for the row, not the word.
    assert!(!folded.contains("breaking   "), "{folded}");
    assert!(folded.contains("subject"), "{folded}");
    // The preview still shows the message that would be committed.
    assert!(folded.contains("preview"), "{folded}");
}

#[test]
fn the_footer_names_the_highlighted_file_in_full() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let path = "src/infrastructure/very/long/directory/chain/file_name.rs";
    let mut app = App::with_config(crate::config::Config::conventional(), Vec::new());
    app.tab = crate::application::tabs::Tab::Branches;
    app.branches.files.items = vec![staging_item(path)];

    let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
    terminal.draw(|frame| draw(frame, &app)).unwrap();
    let footer = rows(&terminal)
        .into_iter()
        .rev()
        .take(2)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(footer.contains(path), "{footer}");
}

#[test]
fn the_footer_keeps_the_status_beside_the_path() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let short = "src/main.rs";
    let mut app = App::with_config(crate::config::Config::conventional(), Vec::new());
    app.branches.files.items = vec![staging_item(short)];
    app.status = "staged src/main.rs".to_string();

    let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
    terminal.draw(|frame| draw(frame, &app)).unwrap();
    let footer = rows(&terminal)
        .into_iter()
        .rev()
        .take(2)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(footer.contains(short), "path: {footer}");
    assert!(footer.contains("staged src/main.rs"), "status: {footer}");
}

#[test]
fn the_branch_wizard_explains_the_selected_type() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let mut app = App::with_config(crate::config::Config::conventional(), Vec::new());
    app.tab = crate::application::tabs::Tab::Branches;
    press(&mut app, KeyCode::Char('n'));

    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal.draw(|frame| draw(frame, &app)).unwrap();
    assert!(
        rows(&terminal).join("\n").contains("new functionality"),
        "the feat description sits in the panel title"
    );

    // The description follows the kind.
    press(&mut app, KeyCode::Right);
    terminal.draw(|frame| draw(frame, &app)).unwrap();
    let screen = rows(&terminal).join("\n");
    assert!(screen.contains("a bug fix on develop"), "{screen}");
    assert!(!screen.contains("new functionality"), "{screen}");
}

#[test]
fn cursor_maps_into_wrapped_rows() {
    assert_eq!(
        cursor_in_wrapped(&["abc".into(), "def".into()], 4),
        Some((1, 1))
    );
}

#[test]
fn a_tick_without_input_does_not_draw_a_frame() {
    use crate::ui::drive;

    let mut draws = 0;
    let mut ticks = 0;
    // Five ticks with no input at all, then the loop ends.
    drive(
        &mut 0u8,
        || Ok(None),
        |_, event| {
            assert!(event.is_none());
            ticks += 1;
            ticks >= 5
        },
        |_| {
            draws += 1;
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(ticks, 5, "the loop kept ticking");
    assert_eq!(draws, 1, "one frame for the state, none per tick");
}

#[test]
fn every_event_draws_one_frame() {
    use crate::ui::drive;
    use ratatui::crossterm::event::Event;

    let mut draws = 0;
    let mut pending = vec![
        Event::Resize(40, 10),
        Event::Resize(40, 10),
        Event::Resize(40, 10),
    ];
    // Then a tick with nothing, then quit on the next tick.
    let mut quiet = 0;
    drive(
        &mut 0u8,
        || Ok(pending.pop()),
        |_, event| {
            if event.is_none() {
                quiet += 1;
                return quiet >= 2;
            }
            false
        },
        |_| {
            draws += 1;
            Ok(())
        },
    )
    .unwrap();
    // First frame, then one per event; the two quiet ticks add nothing.
    assert_eq!(draws, 4, "one frame per event and no more");
}

#[test]
fn the_wizard_shows_the_type_emoji_and_the_source_branch() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let mut app = branch_app();
    app.config.author = Some("hima".to_string());
    app.config.emoji = true;
    app.repo.branch_name = Some("main".to_string());
    app.handle_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE));

    let mut terminal = Terminal::new(TestBackend::new(110, 30)).unwrap();
    terminal.draw(|frame| draw(frame, &app)).unwrap();
    let screen = rows(&terminal).join("\n");
    assert!(
        screen.contains("from   main"),
        "the source branch is information, not a field: {screen}"
    );
    assert!(
        screen.contains("[ ✨") && screen.contains("feat ]"),
        "the kind row carries the emoji the type picker uses: {screen}"
    );
}

#[test]
fn the_wizard_hides_the_emoji_when_the_config_says_so() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let mut app = branch_app();
    app.config.author = Some("hima".to_string());
    app.config.emoji = false;
    app.handle_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE));

    let mut terminal = Terminal::new(TestBackend::new(110, 30)).unwrap();
    terminal.draw(|frame| draw(frame, &app)).unwrap();
    let screen = rows(&terminal).join("\n");
    assert!(screen.contains("[ feat ]"), "{screen}");
    assert!(!screen.contains("✨"), "{screen}");
}

#[test]
fn the_log_title_says_how_many_commits_are_local() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let mut app = App::with_config(crate::config::Config::conventional(), Vec::new());
    app.tab = crate::application::tabs::Tab::History;
    app.set_history(vec![commitsmith::domain::history::PastCommit {
        id: "abc1234".into(),
        short_id: "abc1234".into(),
        summary: "add a button".into(),
        kind: Some("feat".into()),
        ..Default::default()
    }]);
    app.repo.sync = Some((2, 0));

    let mut terminal = Terminal::new(TestBackend::new(140, 30)).unwrap();
    terminal.draw(|frame| draw(frame, &app)).unwrap();
    let screen = rows(&terminal).join("\n");
    assert!(screen.contains("↑2 local"), "{screen}");

    // Nothing local, nothing said.
    app.repo.sync = Some((0, 3));
    terminal.draw(|frame| draw(frame, &app)).unwrap();
    let screen = rows(&terminal).join("\n");
    assert!(!screen.contains("local"), "{screen}");
}

#[test]
fn the_type_summary_sits_at_the_bottom_of_the_log() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let mut app = App::with_config(crate::config::Config::conventional(), Vec::new());
    app.tab = crate::application::tabs::Tab::History;
    app.set_history(vec![
        past("feat", "ui", "add a button"),
        past("fix", "api", "repair the route"),
    ]);
    let mut terminal = Terminal::new(TestBackend::new(140, 30)).unwrap();
    terminal.draw(|frame| draw(frame, &app)).unwrap();
    let screen = rows(&terminal).join("\n");
    // The title no longer carries the counts ...
    let title_line = screen
        .lines()
        .find(|line| line.contains(" log · "))
        .unwrap_or("");
    assert!(
        !title_line.contains("feat"),
        "the title stays short: {title_line}"
    );
    // ... the last line of the list does.
    assert!(screen.contains("feat 1"), "{screen}");
    assert!(screen.contains("fix 1"), "{screen}");
}

#[test]
fn the_details_title_counts_the_issues() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let mut app = App::with_config(crate::config::Config::conventional(), Vec::new());
    app.tab = crate::application::tabs::Tab::History;
    // Past tense in the subject: exactly one warning.
    app.set_history(vec![past("feat", "ui", "added the button")]);

    let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
    terminal.draw(|frame| draw(frame, &app)).unwrap();
    let screen = rows(&terminal).join("\n");
    assert!(screen.contains("details · 1 issue"), "{screen}");
    // The finding itself uses the short wording from 0.23.6.
    assert!(
        screen.contains("imperative: `add`, not `added`"),
        "{screen}"
    );
}

#[test]
fn a_detached_head_says_where_it_is() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let mut app = App::with_config(crate::config::Config::conventional(), Vec::new());
    app.tab = crate::application::tabs::Tab::Branches;
    app.set_in_repository(true);
    app.set_git(Box::new(
        crate::application::test_support::RecordingGit::new(&["main"], "main"),
    ));
    app.repo.branch_name = None;
    app.set_history(vec![commitsmith::domain::history::PastCommit {
        id: "deadbee".into(),
        short_id: "deadbee".into(),
        summary: "some work".into(),
        kind: Some("feat".into()),
        ..Default::default()
    }]);
    app.branches.list.items = vec![commitsmith::domain::ports::BranchInfo::local("main", false)];

    let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
    terminal.draw(|frame| draw(frame, &app)).unwrap();
    let screen = rows(&terminal).join("\n");
    assert!(screen.contains("detached HEAD at deadbee"), "{screen}");
    assert_eq!(app.branch_label(), "detached at deadbee");
}

#[test]
fn the_compact_view_counts_the_fields_it_can_reach_and_shows_a_status() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let mut config = crate::config::Config::conventional();
    config.compact_form = true;
    let mut app = App::with_config(config, Vec::new());
    app.problem("the working tree is in the way");

    // Below COMPACT_HEIGHT/COMPACT_WIDTH the frame switches over.
    let mut terminal = Terminal::new(TestBackend::new(
        crate::ui::COMPACT_WIDTH - 1,
        crate::ui::COMPACT_HEIGHT - 1,
    ))
    .unwrap();
    terminal.draw(|frame| draw(frame, &app)).unwrap();
    let screen = rows(&terminal).join("\n");
    assert!(
        screen.contains("1/3"),
        "only the three reachable fields are counted: {screen}"
    );
    assert!(
        screen.contains("the working tree is in the way"),
        "the status is the only place a problem can appear here: {screen}"
    );

    // An amend must be visible, because Ctrl+S then replaces a commit.
    app.form.amend = true;
    terminal.draw(|frame| draw(frame, &app)).unwrap();
    let screen = rows(&terminal).join("\n");
    assert!(screen.contains("amend"), "{screen}");
}

#[test]
fn missing_hooks_are_named_in_the_footer_and_go_away_when_they_are_there() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let mut app = App::with_config(crate::config::Config::conventional(), Vec::new());
    app.set_repo_line("main · 0 staged".to_string());
    app.set_hooks_missing(&["commit-msg"]);
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    terminal.draw(|frame| draw(frame, &app)).unwrap();
    let screen = rows(&terminal).join("\n");
    assert!(
        screen.contains("hooks not installed · `commitsmith hooks install`"),
        "the hint is readable in full: {screen}"
    );
    // It is not a passing note: a keypress must not clear it.
    assert!(!app.status_is_note);
    app.handle_key(ratatui::crossterm::event::KeyEvent::from(
        ratatui::crossterm::event::KeyCode::Char('x'),
    ));
    assert!(
        app.status.contains("hooks not installed"),
        "a keypress must not clear it: {}",
        app.status
    );

    // With the hooks installed there is nothing to say.
    let mut clean = App::with_config(crate::config::Config::conventional(), Vec::new());
    clean.set_repo_line("main · 0 staged".to_string());
    clean.set_hooks_missing(&[]);
    terminal.draw(|frame| draw(frame, &clean)).unwrap();
    let screen = rows(&terminal).join("\n");
    assert!(!screen.contains("hooks not installed"), "{screen}");
}

#[test]
fn the_miniature_view_shows_the_whole_header_while_a_field_is_typed() {
    // Variant C (E10-1): the header sits above the active field, so the scope you
    // typed a moment ago stays visible while you type the subject.
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let mut app = App::with_config(crate::config::Config::conventional(), Vec::new());
    app.form.commit.kind = "feat".to_string();
    app.form.commit.scope = "TICKET-12".to_string();
    app.form.commit.subject = "add the thing".to_string();
    app.form.field = 2; // subject
    app.revalidate();

    let mut terminal = Terminal::new(TestBackend::new(62, 23)).unwrap();
    terminal.draw(|frame| draw(frame, &app)).unwrap();
    let screen = rows(&terminal).join("\n");

    assert!(
        screen.contains("feat(TICKET-12): add the thing"),
        "the complete header is visible: {screen}"
    );
    // The active field repeats only its own value, and carries the caret.
    assert!(screen.contains("▌"), "the caret is in the field: {screen}");
    assert!(
        screen.contains("13/72"),
        "the counter sits with the field: {screen}"
    );
}

#[test]
fn the_miniature_view_puts_an_error_next_to_its_field() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let mut app = App::with_config(crate::config::Config::conventional(), Vec::new());
    app.form.field = 2;
    app.revalidate();
    let mut terminal = Terminal::new(TestBackend::new(62, 23)).unwrap();
    terminal.draw(|frame| draw(frame, &app)).unwrap();
    let screen = rows(&terminal).join("\n");
    // "empty: say what changes, in one line" is the subject error; it belongs in the
    // field box, not only in the count in the corner.
    assert!(
        screen.contains("say what changes"),
        "the field's own error is shown beside it: {screen}"
    );
}
