//! The application's tests.
//!
//! Out of `mod.rs` so the module itself reads as the state and its rules, not as
//! 1.400 lines of assertions.

#![allow(clippy::too_many_lines)]

use super::*;
use crate::application::test_support::RecordingGit;

fn app_on_the_branch_tab(git: RecordingGit) -> App {
    let mut app = App::default();
    app.set_in_repository(true);
    app.set_git(Box::new(git));
    press(&mut app, KeyCode::Char('2'));
    app
}

fn staging_app() -> (App, std::rc::Rc<std::cell::RefCell<Vec<String>>>) {
    let git = RecordingGit::new(&["main"], "main");
    git.files
        .borrow_mut()
        .push(commitsmith::domain::ports::ChangedFile::new(
            "a.txt",
            None,
            Some(commitsmith::domain::ports::FileState::Modified),
        ));
    let log = git.calls();
    let mut app = App::with_config(crate::config::Config::conventional(), Vec::new());
    app.set_in_repository(true);
    app.set_git(Box::new(git));
    (app, log)
}

#[test]
fn space_stages_and_v_shows_the_diff() {
    let (mut app, log) = staging_app();
    // The files live on tab 2 now; Tab puts the keyboard on them.
    app.tab = tabs::Tab::Branches;
    press(&mut app, KeyCode::Tab);
    press(&mut app, KeyCode::Char(' '));
    assert!(log.borrow().iter().any(|call| call == "stage a.txt"));
    // The footer names the file; the status only says what happened.
    assert_eq!(app.status, "staged");

    press(&mut app, KeyCode::Char('v'));
    assert!(app.branches.files.show_diff);
    assert!(app.branches.files.diff.contains("diff --git"));
}

#[test]
fn the_arrows_move_through_the_branch_list() {
    let mut app = app_on_the_branch_tab(RecordingGit::new(&["main", "feature"], "main"));
    assert_eq!(app.branches.list.selected, 0, "HEAD stays on top");
    press(&mut app, KeyCode::Down);
    assert_eq!(app.branches.list.selected, 1);
    press(&mut app, KeyCode::Down);
    assert_eq!(app.branches.list.selected, 1, "the list ends here");
    press(&mut app, KeyCode::Up);
    assert_eq!(app.branches.list.selected, 0);
    press(&mut app, KeyCode::End);
    assert_eq!(app.branches.list.selected, 1);
    press(&mut app, KeyCode::Home);
    assert_eq!(app.branches.list.selected, 0);
}

#[test]
fn j_and_k_no_longer_move_the_branch_list() {
    let mut app = app_on_the_branch_tab(RecordingGit::new(&["main", "feature"], "main"));
    type_text(&mut app, "jk");
    assert_eq!(
        app.branches.list.selected, 0,
        "letters are not navigation here"
    );
}

#[test]
fn enter_checks_out_the_selected_branch() {
    let git = RecordingGit::new(&["main", "feature"], "main");
    let mut app = app_on_the_branch_tab(git);
    press(&mut app, KeyCode::Down);
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.status, "checked out feature");
    assert_eq!(app.repo.line, "feature · 0 staged");
    assert!(
        app.branches.list.current().unwrap().is_head,
        "list reloaded"
    );
}

#[test]
fn deleting_a_branch_takes_a_second_press() {
    let mut app = app_on_the_branch_tab(RecordingGit::new(&["main", "feature"], "main"));
    press(&mut app, KeyCode::Down);
    press(&mut app, KeyCode::Char('d'));
    assert_eq!(
        app.branches.list.confirming.as_deref(),
        Some("feature"),
        "the first press only asks"
    );
    assert!(app.status.contains("press d again"), "{}", app.status);
    assert_eq!(app.branches.list.len(), 2, "nothing deleted yet");

    press(&mut app, KeyCode::Char('d'));
    assert_eq!(app.branches.list.len(), 1, "the second press deletes");
    assert_eq!(app.status, "deleted feature");
    assert!(app.branches.list.confirming.is_none());
}

#[test]
fn esc_cancels_the_deletion_instead_of_quitting() {
    let mut app = app_on_the_branch_tab(RecordingGit::new(&["main", "feature"], "main"));
    press(&mut app, KeyCode::Down);
    press(&mut app, KeyCode::Char('d'));
    press(&mut app, KeyCode::Esc);
    assert!(!app.should_quit, "Esc only cancels the pending deletion");
    assert!(app.branches.list.confirming.is_none());
    assert_eq!(app.branches.list.len(), 2);

    press(&mut app, KeyCode::Esc);
    assert!(app.should_quit, "a second Esc leaves");
}

#[test]
fn the_checked_out_branch_cannot_be_deleted() {
    let git = RecordingGit::new(&["main", "feature"], "main");
    let mut app = app_on_the_branch_tab(git);
    press(&mut app, KeyCode::Char('d'));
    assert!(app.status.contains("is checked out"), "{}", app.status);
    assert_eq!(app.branches.list.len(), 2);
}

#[test]
fn a_merge_conflict_is_reported_and_the_app_stays_usable() {
    let mut git = RecordingGit::new(&["main", "feature"], "main");
    git.merge_fails = true;
    let mut app = app_on_the_branch_tab(git);
    press(&mut app, KeyCode::Down);
    press(&mut app, KeyCode::Char('m'));
    assert!(!app.status.is_empty());
    assert!(!app.should_quit);
    press(&mut app, KeyCode::Up);
    assert_eq!(app.branches.list.selected, 0, "still navigable");
}

#[test]
fn the_actions_go_through_the_repository_port() {
    let git = RecordingGit::new(&["main", "feature"], "main");
    let log = git.calls();
    let mut app = App::default();
    app.set_in_repository(true);
    app.set_git(Box::new(git));
    press(&mut app, KeyCode::Char('2'));

    press(&mut app, KeyCode::Down);
    press(&mut app, KeyCode::Enter);
    // `feature` is checked out now, so the next row is `main`; merging it
    // into the current branch is the normal direction.
    press(&mut app, KeyCode::Down);
    press(&mut app, KeyCode::Char('m'));
    let actions: Vec<String> = log
        .borrow()
        .iter()
        .filter(|call| call.as_str() != "branch_overview")
        .cloned()
        .collect();
    assert_eq!(
        actions,
        ["checkout feature", "merge main"],
        "git must be told about both, in that order"
    );
}

fn wizard_app(git: RecordingGit) -> App {
    let mut app = App::with_config(crate::config::Config::conventional(), Vec::new());
    app.config.author = Some("hima".to_string());
    app.config.ticket_prefix = "TICKET".to_string();
    app.config.branch_template = "{type}/{author}/{ticket}/{slug}".to_string();
    app.set_in_repository(true);
    app.set_git(Box::new(git));
    press(&mut app, KeyCode::Char('2'));
    app
}

#[test]
fn n_opens_the_wizard_and_esc_closes_it_again() {
    let mut app = wizard_app(RecordingGit::new(&["main"], "main"));
    press(&mut app, KeyCode::Char('n'));
    assert!(app.branches.wizard.open);
    press(&mut app, KeyCode::Esc);
    assert!(!app.branches.wizard.open, "Esc closes the wizard");
    assert!(!app.should_quit, "and does not leave the program");
}

#[test]
fn enter_creates_the_branch_and_checks_it_out() {
    let git = RecordingGit::new(&["main"], "main");
    let log = git.calls();
    let mut app = wizard_app(git);
    press(&mut app, KeyCode::Char('n'));
    press(&mut app, KeyCode::Tab); // ticket
    press(&mut app, KeyCode::Tab); // description
    type_text(&mut app, "Add a thing");
    press(&mut app, KeyCode::Enter);

    assert_eq!(app.status, "created and checked out feat/hima/add-a-thing");
    assert!(!app.branches.wizard.open, "the wizard closes on success");
    let actions: Vec<String> = log
        .borrow()
        .iter()
        .filter(|call| call.as_str() != "branch_overview")
        .cloned()
        .collect();
    assert_eq!(
        actions,
        [
            "create feat/hima/add-a-thing",
            "checkout feat/hima/add-a-thing"
        ]
    );
}

#[test]
fn a_ticket_lands_in_the_name() {
    let git = RecordingGit::new(&["main"], "main");
    let log = git.calls();
    let mut app = wizard_app(git);
    press(&mut app, KeyCode::Char('n'));
    press(&mut app, KeyCode::Tab);
    type_text(&mut app, "1234");
    press(&mut app, KeyCode::Tab);
    type_text(&mut app, "add a thing");
    press(&mut app, KeyCode::Enter);
    assert_eq!(
        app.status,
        "created and checked out feat/hima/TICKET-1234/add-a-thing"
    );
    assert!(
        log.borrow()
            .iter()
            .any(|call| call == "create feat/hima/TICKET-1234/add-a-thing"),
        "{:?}",
        log.borrow()
    );
}

#[test]
fn a_duplicate_branch_keeps_the_wizard_open() {
    let mut git = RecordingGit::new(&["main"], "main");
    git.create_fails = true;
    let mut app = wizard_app(git);
    press(&mut app, KeyCode::Char('n'));
    press(&mut app, KeyCode::Tab);
    press(&mut app, KeyCode::Tab);
    type_text(&mut app, "thing");
    press(&mut app, KeyCode::Enter);

    assert!(
        app.branches.wizard.open,
        "the user can correct and try again"
    );
    assert!(app.branches.wizard.error.is_some());
    assert!(app.status.contains("already exists"), "{}", app.status);
}

#[test]
fn the_wizard_takes_the_keys_away_from_the_list() {
    let mut app = wizard_app(RecordingGit::new(&["main", "feature"], "main"));
    press(&mut app, KeyCode::Char('n'));
    press(&mut app, KeyCode::Char('j'));
    assert_eq!(app.branches.list.selected, 0, "j does not move the list");
    press(&mut app, KeyCode::Tab);
    press(&mut app, KeyCode::Tab);
    press(&mut app, KeyCode::Char('j'));
    assert_eq!(app.branches.wizard.description, "j");
    // `j` is text, not navigation — in the wizard and outside it.
}

fn wizard_on_description(git: RecordingGit) -> App {
    let mut app = wizard_app(git);
    press(&mut app, KeyCode::Char('n'));
    press(&mut app, KeyCode::Tab);
    press(&mut app, KeyCode::Tab);
    app
}

#[test]
fn arrows_in_the_wizard_move_through_the_text() {
    let mut app = wizard_on_description(RecordingGit::new(&["main"], "main"));
    type_text(&mut app, "abc");
    let field = app.branches.wizard.field;
    press(&mut app, KeyCode::Left);
    assert_eq!(app.branches.wizard.field, field, "left stays in the field");
    assert_eq!(app.branches.wizard.cursor(), 2);
    press(&mut app, KeyCode::Char('X'));
    assert_eq!(
        app.branches.wizard.description, "abXc",
        "typing happens at the caret"
    );
    press(&mut app, KeyCode::Home);
    assert_eq!(app.branches.wizard.cursor(), 0);
    press(&mut app, KeyCode::Delete);
    assert_eq!(app.branches.wizard.description, "bXc");
    press(&mut app, KeyCode::End);
    assert_eq!(app.branches.wizard.cursor(), 3);
}

#[test]
fn arrows_in_an_empty_wizard_field_move_between_fields() {
    let mut app = wizard_on_description(RecordingGit::new(&["main"], "main"));
    // Down/Up are the field keys, also inside an empty field.
    press(&mut app, KeyCode::Up);
    assert_eq!(
        app.branches.wizard.field,
        branch_wizard::WizardField::Ticket
    );
    press(&mut app, KeyCode::Down);
    assert_eq!(
        app.branches.wizard.field,
        branch_wizard::WizardField::Description
    );
    // Left/Right stay on the (empty) text.
    press(&mut app, KeyCode::Left);
    assert_eq!(
        app.branches.wizard.field,
        branch_wizard::WizardField::Description
    );
    assert_eq!(app.branches.wizard.cursor(), 0);
}

#[test]
fn arrows_on_the_kind_field_cycle_it() {
    let mut app = wizard_app(RecordingGit::new(&["main"], "main"));
    press(&mut app, KeyCode::Char('n'));
    assert_eq!(
        app.branches.wizard.kind,
        commitsmith::domain::branch::BranchKind::Feat
    );
    press(&mut app, KeyCode::Right);
    assert_eq!(
        app.branches.wizard.kind,
        commitsmith::domain::branch::BranchKind::Fix
    );
    press(&mut app, KeyCode::Left);
    assert_eq!(
        app.branches.wizard.kind,
        commitsmith::domain::branch::BranchKind::Feat
    );
    assert_eq!(
        app.branches.wizard.field,
        branch_wizard::WizardField::Kind,
        "and stay"
    );
}

#[test]
fn the_composer_follows_the_same_rules() {
    let mut app = App::default();
    press(&mut app, KeyCode::Tab);
    assert_eq!(app.active_field(), Field::Scope);

    // Left/Right are the text — also when it is empty.
    press(&mut app, KeyCode::Right);
    assert_eq!(app.active_field(), Field::Scope, "still in the field");

    type_text(&mut app, "abc");
    press(&mut app, KeyCode::Left);
    assert_eq!(app.active_field(), Field::Scope);
    assert_eq!(app.cursor(), 2);
    press(&mut app, KeyCode::Char('X'));
    assert_eq!(app.form.commit.scope, "abXc");

    // Up/Down are the fields.
    press(&mut app, KeyCode::Down);
    assert_eq!(app.active_field(), Field::Subject);
    press(&mut app, KeyCode::Up);
    assert_eq!(app.active_field(), Field::Scope);
}

#[test]
fn tab_and_shift_tab_step_through_every_field() {
    let mut app = App::default();
    for expected in [
        Field::Scope,
        Field::Subject,
        Field::Body,
        Field::Breaking,
        Field::Footer,
    ] {
        press(&mut app, KeyCode::Tab);
        assert_eq!(app.active_field(), expected);
    }
    press(&mut app, KeyCode::Tab);
    assert_eq!(app.active_field(), Field::Type, "wraps to the start");
    press(&mut app, KeyCode::BackTab);
    assert_eq!(app.active_field(), Field::Footer, "and back");
}

#[test]
fn the_composer_never_sees_keys_from_the_branch_tab() {
    let mut app = app_on_the_branch_tab(RecordingGit::new(&["main"], "main"));
    type_text(&mut app, "wxyz");
    assert_eq!(
        app.form.commit.subject, "",
        "typing must not reach the subject"
    );
}

fn press(app: &mut App, code: KeyCode) {
    app.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
}

fn ctrl(app: &mut App, c: char) {
    app.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL));
}

fn press_with(app: &mut App, code: KeyCode, modifiers: KeyModifiers) {
    app.handle_key(KeyEvent::new(code, modifiers));
}

#[test]
fn ctrl_o_folds_the_optional_fields_and_keeps_their_text() {
    let mut app = App::with_config(Config::conventional(), Vec::new());
    // Body is field 3.
    while app.active_field() != Field::Body {
        app.next_field();
    }
    app.handle_paste("body text");

    ctrl(&mut app, 'o');
    assert!(app.form.compact, "Ctrl+O folds");
    assert_eq!(app.value(Field::Body), "body text");
    assert_ne!(
        app.active_field(),
        Field::Body,
        "the focus leaves a field that is no longer drawn"
    );

    // The folded text is still part of the message and still validated.
    app.form.commit.subject = "add a thing".to_string();
    let rendered = app.form.commit.render();
    assert!(
        rendered.contains("body text"),
        "a folded body still goes into the commit: {rendered}"
    );

    ctrl(&mut app, 'o');
    assert!(!app.form.compact, "Ctrl+O unfolds again");
    assert_eq!(app.value(Field::Body), "body text");
}

#[test]
fn tab_skips_folded_fields_in_both_directions() {
    let mut app = App::with_config(Config::conventional(), Vec::new());
    ctrl(&mut app, 'o');
    assert!(app.form.compact);

    let order: Vec<Field> = (0..4)
        .map(|_| {
            let field = app.active_field();
            app.next_field();
            field
        })
        .collect();
    assert_eq!(
        order,
        vec![Field::Type, Field::Scope, Field::Subject, Field::Type],
        "forward: only the three visible fields"
    );

    let back: Vec<Field> = (0..3)
        .map(|_| {
            app.previous_field();
            app.active_field()
        })
        .collect();
    assert_eq!(
        back,
        vec![Field::Type, Field::Subject, Field::Scope],
        "backward: the same three, in reverse"
    );
}

#[test]
fn the_compact_choice_is_written_to_the_config() {
    let mut app = App::with_config(Config::conventional(), Vec::new());
    let store = crate::application::test_support::RecordingConfig::default();
    let written = store.written.clone();
    app.set_config_store(Box::new(store));

    ctrl(&mut app, 'o');
    let text = written.borrow().clone();
    assert!(text.contains("compact_form = true"), "{text}");
    assert!(app.status.contains("compact"), "{}", app.status);
}

fn app_with_remote(
    sync: Option<(usize, usize)>,
) -> (App, std::rc::Rc<std::cell::RefCell<Vec<String>>>) {
    use crate::application::test_support::RecordingGit;
    let mut git = RecordingGit::new(&["main"], "main");
    git.sync = sync;
    let calls = git.calls();
    let mut app = App::with_config(Config::conventional(), Vec::new());
    app.repo.branch_name = Some("main".to_string());
    app.set_git(Box::new(git));
    (app, calls)
}

fn alt(app: &mut App, c: char) {
    app.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::ALT));
}

#[test]
fn a_pull_without_upstream_names_the_command_that_creates_one() {
    let (mut app, _) = app_with_remote(None);
    alt(&mut app, 'p');
    assert_eq!(app.status, "no upstream for main · git push -u origin main");
}

#[test]
fn a_pull_reports_how_many_commits_arrived() {
    let (mut app, calls) = app_with_remote(Some((0, 3)));
    alt(&mut app, 'p');
    assert!(app.status.contains("pulled 3 commits"), "{}", app.status);
    assert!(app.status.contains("into main"), "{}", app.status);
    assert!(calls.borrow().iter().any(|call| call == "pull"));
}

#[test]
fn a_push_reports_how_many_commits_went_out() {
    let (mut app, calls) = app_with_remote(Some((2, 0)));
    alt(&mut app, 'u');
    assert!(app.status.contains("pushed 2 commits"), "{}", app.status);
    assert!(calls.borrow().iter().any(|call| call == "push"));
}

#[test]
fn a_rejected_push_says_to_pull_first() {
    use crate::application::test_support::RecordingGit;
    let mut git = RecordingGit::new(&["main"], "main");
    git.sync = Some((1, 2));
    git.sync_error =
        Some("! [rejected] main -> main (non-fast-forward)\nerror: failed to push".to_string());
    let mut app = App::with_config(Config::conventional(), Vec::new());
    app.repo.branch_name = Some("main".to_string());
    app.set_git(Box::new(git));

    alt(&mut app, 'u');
    assert!(app.status.contains("push rejected"), "{}", app.status);
    assert!(app.status.contains("pull first"), "{}", app.status);
}

#[test]
fn a_diverged_pull_explains_the_conflict_without_merging() {
    use crate::application::test_support::RecordingGit;
    let mut git = RecordingGit::new(&["main"], "main");
    git.sync = Some((1, 1));
    git.sync_error = Some("fatal: Not possible to fast-forward, aborting.".to_string());
    let mut app = App::with_config(Config::conventional(), Vec::new());
    app.repo.branch_name = Some("main".to_string());
    app.set_git(Box::new(git));

    alt(&mut app, 'p');
    assert!(app.status.contains("pull stopped"), "{}", app.status);
    assert!(app.status.contains("pull --rebase"), "{}", app.status);
}

#[test]
fn the_sync_keys_belong_to_the_first_tab() {
    let (mut app, calls) = app_with_remote(Some((1, 0)));
    app.tab = tabs::Tab::Branches;
    alt(&mut app, 'u');
    assert!(
        !calls.borrow().iter().any(|call| call == "push"),
        "Alt+U on tab 2 must not push"
    );
}

#[test]
fn the_footer_label_says_how_far_the_branch_is() {
    let (app, _) = app_with_remote(Some((2, 1)));
    assert_eq!(app.sync_label().as_deref(), Some("↑2↓1"));
    let (app, _) = app_with_remote(Some((0, 0)));
    assert_eq!(app.sync_label().as_deref(), Some("✓"));
    let (app, _) = app_with_remote(None);
    assert_eq!(app.sync_label().as_deref(), Some("–"));
}

#[test]
fn a_cancelled_release_writes_nothing() {
    let dir = std::env::temp_dir().join(format!("commitsmith-cancel-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    use crate::application::test_support::RecordingGit;
    let git = RecordingGit::new(&["main"], "main");
    let calls = git.calls();
    let mut app = App::with_config(Config::conventional(), Vec::new());
    app.tab = tabs::Tab::History;
    app.set_changelog_root(dir.clone());
    app.set_git(Box::new(git));
    app.refresh_plan();

    press(&mut app, KeyCode::Char('y'));
    assert!(app.release.confirm);
    press(&mut app, KeyCode::Esc);
    assert!(!app.release.confirm);
    assert!(
        calls.borrow().iter().all(|call| !call.starts_with("tag ")),
        "no tag after cancelling: {:?}",
        calls.borrow()
    );
    assert!(
        !dir.join("CHANGELOG.md").exists(),
        "no file after cancelling"
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_confirmed_release_writes_the_file_and_no_tag() {
    let dir = std::env::temp_dir().join(format!("commitsmith-write-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    use crate::application::test_support::RecordingGit;
    let git = RecordingGit::new(&["main"], "main");
    git.commits
        .borrow_mut()
        .push(commitsmith::domain::history::PastCommit {
            short_id: "abc1234".to_string(),
            id: "abc1234".to_string(),
            author: "Marc".to_string(),
            date: "today".to_string(),
            summary: "add a button".to_string(),
            body: String::new(),
            kind: Some("feat".to_string()),
            scope: Some("ui".to_string()),
            files: Vec::new(),
        });
    let calls = git.calls();
    let mut app = App::with_config(Config::conventional(), Vec::new());
    app.tab = tabs::Tab::History;
    app.set_changelog_root(dir.clone());
    app.set_git(Box::new(git));
    app.refresh_plan();

    press(&mut app, KeyCode::Char('y'));
    press(&mut app, KeyCode::Enter);

    let text = std::fs::read_to_string(dir.join("CHANGELOG.md")).unwrap();
    assert!(text.contains("## 0.1.0"), "{text}");
    assert!(text.contains("**ui**: add a button"), "{text}");
    // No tags until the project decides to use them.
    assert!(
        calls.borrow().iter().all(|call| !call.starts_with("tag ")),
        "no tag created: {:?}",
        calls.borrow()
    );
    assert!(app.status.contains("no tag set"), "{}", app.status);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_plan_names_the_next_version() {
    use crate::application::test_support::RecordingGit;
    let git = RecordingGit::new(&["main"], "main");
    git.commits
        .borrow_mut()
        .push(commitsmith::domain::history::PastCommit {
            short_id: "abc1234".to_string(),
            id: "abc1234".to_string(),
            author: "Marc".to_string(),
            date: "today".to_string(),
            summary: "add a button".to_string(),
            body: String::new(),
            kind: Some("feat".to_string()),
            scope: Some("ui".to_string()),
            files: Vec::new(),
        });
    let mut app = App::with_config(Config::conventional(), Vec::new());
    app.set_git(Box::new(git));
    app.refresh_plan();

    let plan = app.release.plan.as_ref().expect("a plan without tags");
    assert_eq!(plan.next.to_string(), "0.1.0", "no tag: the first release");
    assert_eq!(plan.commits.len(), 1);
}

#[test]
fn fetch_is_a_tab_two_key_that_reloads_the_marks() {
    use crate::application::test_support::RecordingGit;
    let mut git = RecordingGit::new(&["main", "feature"], "main");
    git.sync_map.insert("main".to_string(), (0, 2));
    let calls = git.calls();
    let mut app = App::with_config(Config::conventional(), Vec::new());
    app.tab = tabs::Tab::Branches;
    app.set_git(Box::new(git));

    press(&mut app, KeyCode::Char('f'));
    assert!(
        calls.borrow().iter().any(|call| call == "fetch"),
        "{:?}",
        calls.borrow()
    );
    assert!(app.status.starts_with("fetched"), "{}", app.status);
    assert!(app.status.contains("1 branch"), "{}", app.status);
}

#[test]
fn the_scope_reaches_the_message_while_typing() {
    let mut app = App::with_config(Config::conventional(), Vec::new());
    press(&mut app, KeyCode::Tab); // scope
    type_text(&mut app, "ui");
    let rendered = app.form.commit.render();
    assert!(rendered.contains("feat(ui):"), "{rendered}");
}

#[test]
fn ctrl_n_offers_what_the_field_can_use() {
    let mut app = App::with_config(Config::conventional(), Vec::new());
    // Type field: Ctrl+N does not open the list, Enter does.
    ctrl(&mut app, 'n');
    assert!(app.picker.is_none(), "Ctrl+N stays away from the type list");
    assert!(app.status.contains("Enter"), "{}", app.status);
    press(&mut app, KeyCode::Enter);
    assert!(app.picker.is_some(), "Enter opens the type list");
    press(&mut app, KeyCode::Esc);

    // Subject field: earlier commits, or a hint when there are none.
    press(&mut app, KeyCode::Tab);
    press(&mut app, KeyCode::Tab);
    ctrl(&mut app, 'n');
    assert!(
        app.picker.is_some() || app.status.contains("no commits"),
        "{}",
        app.status
    );
    press(&mut app, KeyCode::Esc);

    // Body: no list, but an explanation.
    press(&mut app, KeyCode::Tab);
    ctrl(&mut app, 'n');
    assert!(app.picker.is_none(), "no list in the body");
    assert!(
        app.status.contains("no suggestions for body"),
        "{}",
        app.status
    );
}

#[test]
fn the_footer_takes_its_ticket_from_the_branch() {
    use crate::application::test_support::RecordingGit;
    let git = RecordingGit::new(
        &["feat/hima/TICKET-42/add-a-thing", "main"],
        "feat/hima/TICKET-42/add-a-thing",
    );
    let mut app = App::with_config(Config::conventional(), Vec::new());
    app.set_git(Box::new(git));
    assert_eq!(
        app.repo.branch_name.as_deref(),
        Some("feat/hima/TICKET-42/add-a-thing")
    );
    assert!(
        app.form.commit.footer.contains("Refs TICKET-42"),
        "footer: {:?}",
        app.form.commit.footer
    );
}

fn type_text(app: &mut App, text: &str) {
    for c in text.chars() {
        press(app, KeyCode::Char(c));
    }
}

#[test]
fn starts_on_the_type_field() {
    let app = App::default();
    assert_eq!(app.active_field(), Field::Type);
    assert!(!app.is_editing());
    assert_eq!(app.form.commit.kind, "feat");
}

#[test]
fn cycles_types_with_arrows() {
    let mut app = App::default();
    press(&mut app, KeyCode::Right);
    assert_eq!(app.form.commit.kind, "fix");
    press(&mut app, KeyCode::Left);
    assert_eq!(app.form.commit.kind, "feat");
    press(&mut app, KeyCode::Left);
    assert_eq!(
        app.form.commit.kind, "security",
        "wraps around the full type list"
    );
}

#[test]
fn h_and_l_do_not_cycle_the_type() {
    // Removed on request: the arrow keys own the field, and no footer or help
    // line ever advertised `h`/`l`.
    let mut app = App::default();
    press(&mut app, KeyCode::Char('l'));
    assert_eq!(app.form.commit.kind, "feat");
    press(&mut app, KeyCode::Char('h'));
    assert_eq!(app.form.commit.kind, "feat");
    assert_eq!(app.active_field(), Field::Type, "no typing either");
}

#[test]
fn space_opens_the_type_picker() {
    let mut app = App::default();
    press(&mut app, KeyCode::Char(' '));
    assert!(app.picker.is_some());
    type_text(&mut app, "refak");
    press(&mut app, KeyCode::Backspace);
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.form.commit.kind, "refactor");
    assert_eq!(app.active_field(), Field::Scope);
}

#[test]
fn digit_selects_the_nth_entry() {
    let mut app = App::default();
    press(&mut app, KeyCode::Char(' '));
    press(&mut app, KeyCode::Char('2'));
    assert_eq!(app.form.commit.kind, "fix");
    assert!(app.picker.is_none());
    assert_eq!(app.active_field(), Field::Scope);

    press(&mut app, KeyCode::BackTab);
    press(&mut app, KeyCode::Char(' '));
    press(&mut app, KeyCode::Char('0'));
    assert_eq!(app.form.commit.kind, "security", "0 selects the last entry");
}

#[test]
fn digits_filter_once_a_filter_is_typed() {
    let mut app = App::default();
    press(&mut app, KeyCode::Char(' '));
    type_text(&mut app, "b");
    press(&mut app, KeyCode::Char('2'));
    let picker = app.picker.as_ref().expect("picker open");
    assert_eq!(picker.filter, "b2");
    assert!(picker.current().is_none(), "no entry matches b2");
}

#[test]
fn picker_filter_narrows_the_list() {
    let mut app = App::default();
    press(&mut app, KeyCode::Char(' '));
    type_text(&mut app, "er");
    let visible = app.picker.as_ref().map(|p| p.filtered().len()).unwrap();
    assert!(visible >= 1);
    press(&mut app, KeyCode::Esc);
    assert!(app.picker.is_none());
    assert_eq!(app.form.commit.kind, "feat");
}

#[test]
fn ctrl_n_offers_scopes_from_history() {
    let mut app = App::default();
    app.set_scopes(ScopeInfo {
        history: vec!["api".into(), "ui".into()],
        configured: Vec::new(),
    });
    // Ctrl+N follows the keyboard: scopes belong to the scope field.
    press(&mut app, KeyCode::Tab);
    ctrl(&mut app, 'n');
    press(&mut app, KeyCode::Down);
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.form.commit.scope, "ui");
}

fn past(id: &str, kind: &str, scope: Option<&str>, summary: &str, body: &str) -> PastCommit {
    PastCommit {
        short_id: id[..7].to_string(),
        id: id.to_string(),
        author: "Marc".into(),
        date: "2026-11-24".into(),
        summary: summary.into(),
        body: body.into(),
        kind: Some(kind.into()),
        scope: scope.map(str::to_string),
        files: vec!["M src/app.rs".into()],
    }
}

#[test]
fn the_log_reads_its_marks_from_the_cache() {
    // The event loop redraws five times a second; judging the visible rows
    // inside `draw` parsed the same messages again and again. The marks are
    // computed once and only looked up afterwards — proven by poisoning the
    // cache: the drawn mark follows the cache, not a fresh parse.
    let mut app = App::default();
    app.set_history(vec![
        past("aaaaaaa1111111", "feat", Some("ui"), "add a picker", ""),
        past("bbbbbbb2222222", "fix", None, "stop the crash", ""),
    ]);
    assert_eq!(app.history.marks.len(), 2, "one entry per commit");
    let clean = app.history.commits[0].clone();
    assert_eq!(app.log_mark(&clean), Severity::Info);

    app.history.marks.insert(
        clean.id.clone(),
        crate::application::history_view::LintMark {
            severity: Severity::Error,
            issues: Vec::new(),
        },
    );
    assert_eq!(
        app.log_mark(&clean),
        Severity::Error,
        "the drawn mark must come from the cache"
    );

    // Judging again (new rules, new list) clears it.
    app.history.rebuild_marks(&app.rules);
    assert_eq!(app.log_mark(&clean), Severity::Info);
}

#[test]
fn a_commit_that_arrived_late_is_judged_on_demand() {
    let mut app = App::default();
    app.set_history(vec![past("aaaaaaa1111111", "feat", None, "first", "")]);
    let late = past("ccccccc3333333", "feat", None, "second", "");
    assert!(app.history.mark(&late).is_none(), "not in the cache yet");
    // No panic, and the same answer the cache would hold.
    let issues = app.log_issues(&late);
    let fresh = crate::validate::lint_with(
        &crate::application::history_view::lint_text(&late),
        &app.rules,
    );
    assert_eq!(issues, fresh, "on demand equals what the cache computes");
    assert_eq!(
        app.log_mark(&late),
        crate::application::history_view::severity_of(&fresh)
    );
}

fn app_with_history() -> App {
    let mut app = App::default();
    app.set_history(vec![
        past(
            "aaaaaaa1111111",
            "feat",
            Some("composer"),
            "add picker preview",
            "The picker previews the highlighted commit.",
        ),
        past("bbbbbbb2222222", "fix", None, "stop the crash", ""),
    ]);
    app
}

#[test]
fn history_picker_carries_previews() {
    let mut app = app_with_history();
    ctrl(&mut app, 'r');
    let picker = app.picker.as_ref().expect("history picker open");
    assert_eq!(picker.kind, PickerKind::History);
    assert_eq!(picker.items.len(), 2);
    assert!(picker.items[0].contains("feat(composer)"));
    assert!(
        picker
            .preview()
            .unwrap()
            .contains("previews the highlighted")
    );
    press(&mut app, KeyCode::Down);
    let picker = app.picker.as_ref().unwrap();
    assert!(picker.preview().unwrap().starts_with("fix: stop the crash"));
}

#[test]
fn history_without_commits_reports_instead_of_opening() {
    let mut app = App::default();
    ctrl(&mut app, 'r');
    assert!(app.picker.is_none());
    assert!(app.status.contains("no commits yet"));
}

#[test]
fn confirming_history_reuses_type_scope_and_body() {
    let mut app = app_with_history();
    app.form.commit.subject = "already typed".into();
    ctrl(&mut app, 'r');
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.form.commit.kind, "feat");
    assert_eq!(app.form.commit.scope, "composer");
    assert!(app.form.commit.body.contains("previews the highlighted"));
    assert_eq!(app.form.commit.subject, "already typed", "subject stays");
    assert_eq!(app.active_field(), Field::Subject);
    assert!(app.status.contains("reused: type, scope, body"));
}

#[test]
fn history_filter_finds_a_commit_by_text() {
    let mut app = app_with_history();
    ctrl(&mut app, 'r');
    type_text(&mut app, "crash");
    assert_eq!(app.picker.as_ref().unwrap().filtered().len(), 1);
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.form.commit.kind, "fix");
    assert_eq!(
        app.form.commit.scope, "",
        "plain commit keeps the scope empty"
    );
}

#[test]
fn unknown_types_from_history_are_not_applied() {
    let mut app = App::default();
    app.set_history(vec![past("ccccccc3333333", "wip", None, "scratch", "")]);
    ctrl(&mut app, 'r');
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.form.commit.kind, "feat", "wip is not in the profile");
    // The type is not applied, but the subject still is: it is a commit
    // message, and the user may want its wording.
    assert_eq!(app.form.commit.subject, "scratch");
    assert!(app.status.contains("reused: subject"), "{}", app.status);
}

#[test]
fn ctrl_p_switches_profile_and_types() {
    let mut app = App::default();
    ctrl(&mut app, 'p');
    type_text(&mut app, "ang");
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.config.profile, "angular");
    assert_eq!(app.form.commit.kind, app.config.type_names()[0]);
    ctrl(&mut app, 'p');
    type_text(&mut app, "atomic");
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.config.profile, "atomic");
}

#[test]
fn typing_lands_in_the_active_field() {
    let mut app = App::default();
    press(&mut app, KeyCode::Tab);
    type_text(&mut app, "api");
    assert_eq!(app.form.commit.scope, "api");
    assert_eq!(app.cursor(), 3);
}

#[test]
fn backspace_and_delete_edit_around_the_cursor() {
    let mut app = App::default();
    press(&mut app, KeyCode::Tab);
    type_text(&mut app, "abc");
    press(&mut app, KeyCode::Left);
    press(&mut app, KeyCode::Backspace);
    assert_eq!(app.form.commit.scope, "ac");
    press(&mut app, KeyCode::Delete);
    assert_eq!(app.form.commit.scope, "a");
}

#[test]
fn enter_in_body_starts_a_new_line() {
    let mut app = App::default();
    press(&mut app, KeyCode::Tab);
    press(&mut app, KeyCode::Tab);
    press(&mut app, KeyCode::Tab);
    type_text(&mut app, "first");
    press(&mut app, KeyCode::Enter);
    type_text(&mut app, "second");
    assert_eq!(app.form.commit.body, "first\nsecond");
    press(&mut app, KeyCode::Up);
    assert_eq!(app.cursor(), 5);
}

#[test]
fn ctrl_s_renders_the_message() {
    let mut app = App::default();
    press(&mut app, KeyCode::Tab);
    press(&mut app, KeyCode::Tab);
    type_text(&mut app, "add composer");
    ctrl(&mut app, 's');
    assert_eq!(app.output.as_deref(), Some("feat: add composer"));
    assert!(app.should_quit);
}

#[test]
fn ctrl_k_clears_only_the_field_under_the_cursor() {
    let mut app = App::default();
    press(&mut app, KeyCode::Tab);
    type_text(&mut app, "api");
    press(&mut app, KeyCode::Tab);
    type_text(&mut app, "a subject");
    // Back to the scope, then clear it.
    press(&mut app, KeyCode::BackTab);
    ctrl(&mut app, 'k');
    assert_eq!(app.value(Field::Scope), "");
    assert_eq!(app.cursor(), 0);
    assert_eq!(app.value(Field::Subject), "a subject", "other fields stay");
    assert!(app.status.contains("cleared"));
}

#[test]
fn ctrl_k_leaves_the_type_alone() {
    let mut app = App::default();
    let before = app.value(Field::Type).to_string();
    ctrl(&mut app, 'k');
    assert_eq!(app.value(Field::Type), before);
    assert!(app.status.contains("type stays"));
}

#[test]
fn ctrl_k_on_an_empty_field_says_so() {
    let mut app = App::default();
    press(&mut app, KeyCode::Tab);
    ctrl(&mut app, 'k');
    assert!(app.status.contains("already empty"), "{}", app.status);
}

#[test]
fn ctrl_s_refuses_to_finish_with_errors() {
    let mut app = App::default();
    // 0.3.2 rendered "feat:" for an empty subject and committed it.
    ctrl(&mut app, 's');
    assert!(
        app.output.is_none(),
        "an invalid message must not leave the app"
    );
    assert!(!app.should_quit);
    assert!(app.status.starts_with("not yet:"), "{}", app.status);
    assert!(app.status.contains("subject"), "{}", app.status);
}

#[test]
fn ctrl_s_finishes_after_clearing_the_error() {
    let mut app = App::default();
    ctrl(&mut app, 's');
    assert!(app.output.is_none());
    press(&mut app, KeyCode::Tab);
    press(&mut app, KeyCode::Tab);
    type_text(&mut app, "add composer");
    ctrl(&mut app, 's');
    assert_eq!(app.output.as_deref(), Some("feat: add composer"));
    assert!(app.should_quit);
}

#[test]
fn digits_switch_tabs_when_no_field_is_typing() {
    let mut app = App::default();
    press(&mut app, KeyCode::Char('2'));
    assert_eq!(app.tab, tabs::Tab::Branches);
    press(&mut app, KeyCode::Char('3'));
    assert_eq!(app.tab, tabs::Tab::History);
    press(&mut app, KeyCode::Char('1'));
    assert_eq!(app.tab, tabs::Tab::Commit);
}

#[test]
fn a_digit_inside_a_field_stays_text() {
    let mut app = App::default();
    press(&mut app, KeyCode::Tab);
    press(&mut app, KeyCode::Tab);
    type_text(&mut app, "1st try");
    assert_eq!(app.form.commit.subject, "1st try");
    assert_eq!(app.tab, tabs::Tab::Commit, "typing must not change the tab");
}

#[test]
fn the_tab_keeps_what_was_typed_in_it() {
    let mut app = App::default();
    press(&mut app, KeyCode::Tab);
    press(&mut app, KeyCode::Tab);
    type_text(&mut app, "add composer");
    // While a field is focused the digits are text, so the page keys are the
    // way out; the round trip must not lose the subject.
    app.handle_key(KeyEvent::new(KeyCode::PageDown, KeyModifiers::CONTROL));
    assert_eq!(app.tab, tabs::Tab::Branches);
    app.handle_key(KeyEvent::new(KeyCode::PageUp, KeyModifiers::CONTROL));
    assert_eq!(app.tab, tabs::Tab::Commit);
    assert_eq!(app.form.commit.subject, "add composer");
}

#[test]
fn a_digit_typed_in_a_field_does_not_leak_into_the_tab_bar() {
    let mut app = App::default();
    press(&mut app, KeyCode::Tab);
    press(&mut app, KeyCode::Tab);
    type_text(&mut app, "fix 2 bugs");
    assert_eq!(app.form.commit.subject, "fix 2 bugs");
    assert_eq!(app.tab, tabs::Tab::Commit);
}

#[test]
fn ctrl_page_keys_cycle_tabs() {
    let mut app = App::default();
    app.handle_key(KeyEvent::new(KeyCode::PageDown, KeyModifiers::CONTROL));
    assert_eq!(app.tab, tabs::Tab::Branches);
    app.handle_key(KeyEvent::new(KeyCode::PageUp, KeyModifiers::CONTROL));
    assert_eq!(app.tab, tabs::Tab::Commit);
    app.handle_key(KeyEvent::new(KeyCode::PageUp, KeyModifiers::CONTROL));
    assert_eq!(app.tab, tabs::Tab::History, "wraps around");
}

#[test]
fn digits_do_not_switch_tabs_while_a_picker_is_open() {
    let mut app = App::default();
    press(&mut app, KeyCode::Char(' ')); // type picker
    press(&mut app, KeyCode::Char('2'));
    assert_eq!(app.tab, tabs::Tab::Commit);
}

#[test]
fn the_history_preview_loads_files_lazily_and_once() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
    let mut app = App::default();
    app.set_files_loader(Box::new(move |id: &str| {
        counter.fetch_add(1, Ordering::SeqCst);
        vec![format!("M {id}.rs")]
    }));
    app.set_history(vec![
        PastCommit {
            id: "aaaa111".into(),
            short_id: "aaaa111".into(),
            summary: "first".into(),
            kind: Some("feat".into()),
            ..Default::default()
        },
        PastCommit {
            id: "bbbb222".into(),
            short_id: "bbbb222".into(),
            summary: "second".into(),
            kind: Some("fix".into()),
            ..Default::default()
        },
    ]);

    app.handle_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL));
    assert_eq!(calls.load(Ordering::SeqCst), 1, "one commit read on open");
    let preview = app.picker.as_ref().unwrap().preview().unwrap().to_string();
    assert!(preview.contains("M aaaa111.rs"), "{preview}");

    // Moving to the next commit reads exactly one more.
    app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    let preview = app.picker.as_ref().unwrap().preview().unwrap().to_string();
    assert!(preview.contains("M bbbb222.rs"), "{preview}");

    // Coming back is a cache hit.
    app.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
    assert_eq!(
        calls.load(Ordering::SeqCst),
        2,
        "no second read for a cached commit"
    );
}

#[test]
fn a_commit_without_files_still_previews() {
    let mut app = App::default();
    app.set_files_loader(Box::new(|_: &str| Vec::new()));
    app.set_history(vec![PastCommit {
        id: "cccc333".into(),
        short_id: "cccc333".into(),
        summary: "no files".into(),
        kind: Some("docs".into()),
        ..Default::default()
    }]);
    app.handle_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL));
    let preview = app.picker.as_ref().unwrap().preview().unwrap();
    assert!(preview.contains("docs: no files"), "{preview}");
}

#[test]
fn q_quits_only_outside_a_text_field() {
    let mut app = App::default();
    press(&mut app, KeyCode::Tab);
    press(&mut app, KeyCode::Char('q'));
    assert_eq!(app.form.commit.scope, "q");
    assert!(!app.should_quit);
    press(&mut app, KeyCode::Esc);
    assert!(app.should_quit);
}

#[test]
fn help_overlay_swallows_keys_until_closed() {
    let mut app = App::default();
    press(&mut app, KeyCode::Char('?'));
    assert!(app.show_help);
    press(&mut app, KeyCode::Tab);
    assert_eq!(app.active_field(), Field::Type);
    press(&mut app, KeyCode::Esc);
    assert!(!app.show_help);
    assert!(!app.should_quit, "Esc closes the overlay first");
}

#[test]
fn validation_follows_the_input() {
    let mut app = App::default();
    press(&mut app, KeyCode::Tab);
    press(&mut app, KeyCode::Tab);
    type_text(&mut app, "Added stuff.");
    assert!(
        app.issues
            .iter()
            .any(|issue| issue.message.contains("imperative"))
    );
}

#[test]
fn footer_is_seeded_with_the_author_handle() {
    let mut config = Config::conventional();
    config.user = Some("Marc Hinte".into());
    config.prefix = Some("hima".into());
    let app = App::with_config(config, Vec::new());
    assert_eq!(app.form.commit.footer, "Co-authored-by: hima");
}

fn app_on_branch(config: Config, branch: &str) -> App {
    let git = RecordingGit::new(&[branch], branch);
    let mut app = App::with_config(config, Vec::new());
    app.set_in_repository(true);
    app.set_git(Box::new(git));
    app
}

#[test]
fn the_branch_ticket_lands_in_the_footer() {
    let mut config = Config::conventional();
    config.prefix = Some("hima".into());
    let app = app_on_branch(config, "feat/hima/TICKET-42/add-a-thing");
    assert_eq!(
        app.form.commit.footer,
        "Refs TICKET-42\nCo-authored-by: hima"
    );
}

#[test]
fn the_co_author_can_be_switched_off() {
    let mut config = Config::conventional();
    config.prefix = Some("hima".into());
    config.co_author = false;
    let app = app_on_branch(config, "feat/hima/TICKET-42/add-a-thing");
    assert_eq!(app.form.commit.footer, "Refs TICKET-42");
}

#[test]
fn a_custom_co_author_name_is_used() {
    let mut config = Config::conventional();
    config.prefix = Some("hima".into());
    config.co_author_name = Some("Marc Hinte <marc@example.com>".into());
    let app = app_on_branch(config, "main");
    assert_eq!(
        app.form.commit.footer,
        "Co-authored-by: Marc Hinte <marc@example.com>"
    );
}

#[test]
fn a_branch_without_a_ticket_carries_no_reference() {
    let mut config = Config::conventional();
    config.prefix = Some("hima".into());
    let app = app_on_branch(config, "fix/old");
    assert_eq!(app.form.commit.footer, "Co-authored-by: hima");
}

#[test]
fn typing_in_the_footer_survives_a_branch_change() {
    let mut config = Config::conventional();
    config.prefix = Some("hima".into());
    let git = RecordingGit::new(&["main", "feat/hima/TICKET-7/next"], "main");
    let mut app = App::with_config(config, Vec::new());
    app.set_in_repository(true);
    app.set_git(Box::new(git));
    app.form.commit.footer = "Closes OTHER-9".to_string();

    press(&mut app, KeyCode::Char('2'));
    press(&mut app, KeyCode::Down);
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.form.commit.footer, "Closes OTHER-9", "typing wins");
}

#[test]
fn the_footer_follows_a_branch_the_wizard_created() {
    let mut config = Config::conventional();
    config.prefix = Some("hima".into());
    config.author = Some("hima".into());
    config.ticket_prefix = "TICKET".into();
    let mut app = app_on_branch(config, "main");
    assert_eq!(app.form.commit.footer, "Co-authored-by: hima");

    press(&mut app, KeyCode::Char('2'));
    press(&mut app, KeyCode::Char('n'));
    press(&mut app, KeyCode::Tab);
    type_text(&mut app, "42");
    press(&mut app, KeyCode::Tab);
    type_text(&mut app, "add a thing");
    press(&mut app, KeyCode::Enter);
    assert_eq!(
        app.form.commit.footer, "Refs TICKET-42\nCo-authored-by: hima",
        "the branch's ticket and the co-author follow each other"
    );
}

#[test]
fn the_branch_kind_sets_the_commit_type() {
    let config = Config::conventional();
    let app = app_on_branch(config, "hotfix/hima/TICKET-1/urgent");
    assert_eq!(
        app.form.commit.kind, "fix",
        "a hotfix is a fix for the message"
    );
}

#[test]
fn a_docs_branch_sets_docs() {
    let app = app_on_branch(Config::conventional(), "docs/hima/TICKET-2/readme");
    assert_eq!(app.form.commit.kind, "docs");
}

#[test]
fn a_typed_type_is_never_overwritten_by_a_branch() {
    let git = RecordingGit::new(&["main", "chore/hima/TICKET-3/tidy"], "main");
    let mut app = App::with_config(Config::conventional(), Vec::new());
    app.set_in_repository(true);
    app.set_git(Box::new(git));
    // The user picks a type themselves.
    press(&mut app, KeyCode::Right);
    let chosen = app.form.commit.kind.clone();
    assert_ne!(chosen, "chore");

    press(&mut app, KeyCode::Char('2'));
    press(&mut app, KeyCode::Down);
    press(&mut app, KeyCode::Enter);
    assert_eq!(
        app.form.commit.kind, chosen,
        "the branch must not win over the user"
    );
}

#[test]
fn a_branch_kind_missing_from_the_profile_is_ignored() {
    let mut config = Config::conventional();
    // A house profile without `docs`: the branch kind has no counterpart.
    let mut profile = config.active().clone();
    profile.types.retain(|kind| kind.name != "docs");
    config.profiles.insert("house".into(), profile);
    config.profile = "house".into();
    assert!(!config.type_names().contains(&"docs".to_string()));

    let app = app_on_branch(config, "docs/hima/TICKET-4/readme");
    assert_ne!(app.form.commit.kind, "docs", "the profile has no such type");
    assert_eq!(app.form.commit.kind, "feat", "the default stays");
}

#[test]
fn a_non_conventional_branch_leaves_the_type_alone() {
    let app = app_on_branch(Config::conventional(), "main");
    assert_eq!(app.form.commit.kind, "feat", "the default stays");
}

#[test]
fn a_branch_from_the_wizard_sets_the_type_too() {
    let mut config = Config::conventional();
    config.author = Some("hima".into());
    config.prefix = Some("hima".into());
    config.ticket_prefix = "TICKET".into();
    config.branch_template = "{type}/{author}/{ticket}/{slug}".into();
    let mut app = app_on_branch(config, "main");
    assert_eq!(app.form.commit.kind, "feat");

    press(&mut app, KeyCode::Char('2'));
    press(&mut app, KeyCode::Char('n'));
    press(&mut app, KeyCode::Right); // feat -> fix
    press(&mut app, KeyCode::Tab);
    type_text(&mut app, "12");
    press(&mut app, KeyCode::Tab);
    type_text(&mut app, "broken thing");
    press(&mut app, KeyCode::Enter);

    assert_eq!(
        app.form.commit.kind, "fix",
        "the new branch is a fix branch"
    );
    assert_eq!(
        app.form.commit.footer,
        "Refs TICKET-12\nCo-authored-by: hima"
    );
}

#[test]
fn no_handle_means_no_footer_noise() {
    let app = App::default();
    assert_eq!(app.form.commit.footer, "");
    assert_eq!(app.suggested_footer(), None);
}

#[test]
fn custom_profile_types_are_accepted() {
    let mut config = Config::conventional();
    config.profile = "atomic".into();
    let mut app = App::with_config(config, Vec::new());
    app.form.commit.subject = "add thing".into();
    assert!(!app.issues.iter().any(|i| i.field == "type"));
}

#[test]
fn switching_to_the_branches_tab_costs_no_branch_read() {
    // The list is loaded when it changes, not when a tab is looked at, and the
    // ahead/behind arrows were two extra processes per switch. A switch reads
    // `git status` and nothing else.
    let (mut app, log) = staging_app();
    log.borrow_mut().clear();
    app.tab = tabs::Tab::Commit;
    app.branches.panel = Panel::Files;
    press(&mut app, KeyCode::Char('2'));
    assert_eq!(app.tab, tabs::Tab::Branches, "2 goes to the branches");
    let calls = log.borrow().clone();
    assert!(
        calls.is_empty(),
        "unexpected git calls on a switch: {calls:?}"
    );
}

#[test]
fn ctrl_enter_writes_the_message_like_ctrl_s() {
    let (mut app, _) = staging_app();
    press(&mut app, KeyCode::Tab); // type -> scope
    press(&mut app, KeyCode::Tab); // scope -> subject
    for c in "add the button".chars() {
        press(&mut app, KeyCode::Char(c));
    }
    press_with(&mut app, KeyCode::Enter, KeyModifiers::CONTROL);
    assert!(
        app.output
            .as_deref()
            .unwrap_or_default()
            .contains("add the button"),
        "Ctrl+Enter hands the message over: {:?}",
        app.status
    );
}

#[test]
fn ctrl_space_opens_the_suggestions_for_the_field() {
    let (mut app, _) = staging_app();
    app.set_scopes(ScopeInfo {
        history: vec!["ui".into(), "api".into()],
        configured: Vec::new(),
    });
    press(&mut app, KeyCode::Tab); // scope is the field with a list
    press_with(&mut app, KeyCode::Char(' '), KeyModifiers::CONTROL);
    assert!(
        app.picker.is_some(),
        "Ctrl+Space opens the list: {}",
        app.status
    );

    let (mut app, _) = staging_app();
    app.set_scopes(ScopeInfo {
        history: vec!["ui".into()],
        configured: Vec::new(),
    });
    press(&mut app, KeyCode::Tab);
    // A terminal that sends NUL: crossterm may report it either way.
    press_with(&mut app, KeyCode::Null, KeyModifiers::NONE);
    assert!(
        app.picker.is_some(),
        "a bare NUL means the same: {}",
        app.status
    );
}

fn file_app(
    index: Option<commitsmith::domain::ports::FileState>,
    worktree: Option<commitsmith::domain::ports::FileState>,
) -> (App, std::rc::Rc<std::cell::RefCell<Vec<String>>>) {
    let git = RecordingGit::new(&["main"], "main");
    git.files
        .borrow_mut()
        .push(commitsmith::domain::ports::ChangedFile::new(
            "a.txt", index, worktree,
        ));
    let log = git.calls();
    let mut app = App::with_config(crate::config::Config::conventional(), Vec::new());
    app.set_in_repository(true);
    app.set_git(Box::new(git));
    app.reload_staging();
    app.tab = tabs::Tab::Branches;
    app.branches.panel = Panel::Files;
    (app, log)
}

#[test]
fn r_takes_the_file_out_of_the_index_and_keeps_the_changes() {
    let (mut app, log) = file_app(
        Some(commitsmith::domain::ports::FileState::Modified),
        Some(commitsmith::domain::ports::FileState::Modified),
    );
    press(&mut app, KeyCode::Char('r'));
    assert!(
        log.borrow().iter().any(|call| call == "unstage a.txt"),
        "{:?}",
        log.borrow()
    );
    assert!(app.status.contains("left the index"), "{}", app.status);
}

#[test]
fn x_asks_before_it_throws_changes_away() {
    let (mut app, log) = file_app(None, Some(commitsmith::domain::ports::FileState::Modified));
    press(&mut app, KeyCode::Char('x'));
    assert!(app.branches.files.confirm.is_some(), "the question is open");
    assert!(
        !log.borrow().iter().any(|call| call.starts_with("discard")),
        "nothing happens on the first press"
    );

    press(&mut app, KeyCode::Char('n'));
    assert!(
        app.branches.files.confirm.is_none(),
        "any other key cancels"
    );
    assert!(!log.borrow().iter().any(|call| call.starts_with("discard")));
}

#[test]
fn y_after_the_question_discards_the_changes() {
    let (mut app, log) = file_app(None, Some(commitsmith::domain::ports::FileState::Modified));
    press(&mut app, KeyCode::Char('x'));
    press(&mut app, KeyCode::Char('y'));
    assert!(
        log.borrow().iter().any(|call| call == "discard a.txt"),
        "{:?}",
        log.borrow()
    );
    assert!(app.status.contains("discarded"));
}

#[test]
fn z_parks_the_changes_and_shift_z_asks_before_taking_them_back() {
    let (mut app, log) = file_app(None, Some(commitsmith::domain::ports::FileState::Modified));
    press(&mut app, KeyCode::Char('z'));
    assert!(
        log.borrow().iter().any(|call| call.starts_with("stash")),
        "{:?}",
        log.borrow()
    );

    press(&mut app, KeyCode::Char('Z'));
    assert!(
        app.branches.files.confirm.is_some(),
        "taking it back asks first"
    );
    press(&mut app, KeyCode::Char('y'));
    assert!(
        log.borrow().iter().any(|call| call == "stash pop"),
        "{:?}",
        log.borrow()
    );
}

#[test]
fn the_release_starts_with_this_branch_and_b_moves_the_window() {
    let (mut app, log) = staging_app();
    app.tab = tabs::Tab::History;
    app.refresh_plan();
    assert_eq!(
        app.release.basis,
        release_view::Basis::Branch,
        "the default"
    );
    let _ = &log;
    assert_eq!(
        app.release.plan.as_ref().unwrap().basis.as_deref(),
        Some("on this branch, not on main")
    );

    press(&mut app, KeyCode::Char('b'));
    assert_eq!(app.release.basis, release_view::Basis::Recent(20));
    assert_eq!(
        app.release.plan.as_ref().unwrap().basis.as_deref(),
        Some("last 20 commits")
    );
    press(&mut app, KeyCode::Char('b'));
    assert_eq!(app.release.basis, release_view::Basis::Recent(50));
    press(&mut app, KeyCode::Char('b'));
    assert_eq!(app.release.basis, release_view::Basis::Tag);
    assert!(
        app.release.plan.as_ref().unwrap().basis.is_none(),
        "since the tag"
    );
    press(&mut app, KeyCode::Char('b'));
    assert_eq!(
        app.release.basis,
        release_view::Basis::Branch,
        "and around again"
    );
}

#[test]
fn e_writes_the_release_notes_to_a_file() {
    let dir = std::env::temp_dir().join(format!("commitsmith-export-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let (mut app, _) = staging_app();
    app.tab = tabs::Tab::History;
    app.set_changelog_root(dir.clone());
    app.refresh_plan();
    press(&mut app, KeyCode::Char('e'));

    let written: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.to_string_lossy().contains("RELEASE-NOTES-"))
        .collect();
    assert_eq!(
        written.len(),
        1,
        "one file, not a directory full: {}",
        app.status
    );
    let text = std::fs::read_to_string(&written[0]).unwrap();
    assert!(text.contains("## "), "the section has a heading: {text}");
    std::fs::remove_dir_all(&dir).ok();
}
