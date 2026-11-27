//! `commitsmith hooks` without a subcommand: a small wizard that explains what is
//! about to happen, does it, and proves that it works.
//!
//! Shaped like the other dialogs in this program: arrow keys move, Enter takes
//! the choice, Esc leaves without writing. No tooltips, no toasts — the summary
//! at the end is the whole point, because a hook that is installed but not
//! understood gets switched off at the first annoying commit.

use std::io;

use ratatui::DefaultTerminal;
use ratatui::Frame;
use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, BorderType, Paragraph, Wrap};

use crate::hooks;
use crate::ui::theme;

/// The three hooks, in the order they run.
const HOOKS: [(&str, &str); 3] = [
    ("commit-msg", "checks the message before git records it"),
    (
        "prepare-commit-msg",
        "starts a new message from the branch name",
    ),
    ("pre-commit", "hints about the index, never blocks"),
];

/// Where the wizard is.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Step {
    Choose,
    Result,
}

pub(crate) struct Wizard {
    step: Step,
    /// Which hooks are ticked, in the order of [`HOOKS`].
    chosen: [bool; 3],
    /// Cursor for the choice list.
    cursor: usize,
    /// What happened, once it happened.
    outcome: Option<Result<String, String>>,
    /// Everything the notes column says about this repository.
    notes: Vec<String>,
}

/// What the wizard ended with.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The user left without installing anything.
    Skipped,
    /// Hooks were installed (or the attempt failed) — the text is for the screen.
    Done(Result<String, String>),
}

impl Wizard {
    fn new() -> Self {
        Self {
            step: Step::Choose,
            chosen: [true, true, true],
            cursor: 0,
            outcome: None,
            notes: notes(),
        }
    }

    fn toggle(&mut self) {
        self.chosen[self.cursor] = !self.chosen[self.cursor];
    }

    fn install(&mut self) {
        let wanted: Vec<&str> = HOOKS
            .iter()
            .enumerate()
            .filter(|(index, _)| self.chosen[*index])
            .map(|(_, (name, _))| *name)
            .collect();
        if wanted.is_empty() {
            self.outcome = Some(Err("nothing chosen · nothing installed".to_string()));
            self.step = Step::Result;
            return;
        }
        self.outcome = Some(match hooks::install_selected(&wanted, false) {
            Ok(done) => {
                let mut text = format!("installed {}", done.paths.join(", "));
                for previous in done.chained {
                    text.push_str(&format!("\nyour previous hook moved to {previous}"));
                }
                text.push_str("\ntry it: git commit -m \"feat:no space\" is rejected");
                Ok(text)
            }
            Err(error) => Err(error),
        });
        self.step = Step::Result;
    }
}

/// Facts about this repository that change what the wizard should say.
fn notes() -> Vec<String> {
    let mut notes = Vec::new();
    match hooks::hooks_dir_display() {
        Some(dir) => notes.push(format!("hooks go to {dir}")),
        None => notes.push("not inside a git repository · nothing to install".to_string()),
    }
    let missing = hooks::missing();
    if missing.is_empty() {
        notes.push("all three hooks are installed already".to_string());
    } else {
        notes.push(format!("missing: {}", missing.join(", ")));
    }
    notes.push("always skippable: git commit --no-verify".to_string());
    notes
}

impl Wizard {
    fn handle_key(&mut self, key: KeyEvent) {
        if key.kind != KeyEventKind::Press {
            return;
        }
        match self.step {
            Step::Choose => match key.code {
                KeyCode::Up | KeyCode::Char('k') => {
                    self.cursor = self.cursor.saturating_sub(1);
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    self.cursor = (self.cursor + 1).min(HOOKS.len() - 1);
                }
                KeyCode::Char(' ') => self.toggle(),
                KeyCode::Enter => self.install(),
                _ => {}
            },
            Step::Result => {}
        }
    }

    fn finished(&self, key: &KeyEvent) -> bool {
        match self.step {
            Step::Result => true,
            Step::Choose => matches!(key.code, KeyCode::Esc),
        }
    }
}

/// Run the wizard on the terminal the caller already owns.
pub fn run(terminal: &mut DefaultTerminal) -> io::Result<Outcome> {
    let mut wizard = Wizard::new();
    let mut outcome: Option<Outcome> = None;
    crate::ui::event_loop(
        terminal,
        &mut wizard,
        || match crate::ui::poll_tick()? {
            true => Ok(Some(crate::ui::read_event()?)),
            false => Ok(None),
        },
        |wizard, event| {
            if let Some(Event::Key(key)) = event {
                if key.kind == KeyEventKind::Press && wizard.finished(&key) {
                    outcome = Some(match wizard.outcome.take() {
                        Some(result) => Outcome::Done(result),
                        None => Outcome::Skipped,
                    });
                    return true;
                }
                wizard.handle_key(key);
            }
            false
        },
        draw,
    )?;
    Ok(outcome.unwrap_or(Outcome::Skipped))
}

/// Draw one frame. Public so the tests can render it without a terminal.
pub fn draw(frame: &mut Frame, wizard: &Wizard) {
    let area = frame.area();
    let [header, body, footer] = Layout::vertical([
        Constraint::Length(2),
        Constraint::Min(6),
        Constraint::Length(1),
    ])
    .areas(area);

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(" hooks ", theme::accent()),
            Span::styled(
                "what git will run before it records a commit",
                theme::muted(),
            ),
        ])),
        header,
    );

    match wizard.step {
        Step::Choose => draw_choice(frame, wizard, body),
        Step::Result => draw_result(frame, wizard, body),
    }

    let keys = match wizard.step {
        Step::Choose => " ↓ ↑  move   Space  tick   Enter  install   Esc  cancel",
        Step::Result => " Enter  done",
    };
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(keys, theme::muted()))),
        footer,
    );
}

fn draw_choice(frame: &mut Frame, wizard: &Wizard, area: Rect) {
    // The notes box needs its border on top of the lines it shows.
    let notes_height = wizard.notes.len() as u16 + 2;
    let [list_area, notes_area] =
        Layout::vertical([Constraint::Min(6), Constraint::Length(notes_height.max(3))]).areas(area);

    let mut lines: Vec<Line> = Vec::new();
    for (index, (name, what)) in HOOKS.iter().enumerate() {
        let ticked = wizard.chosen[index];
        let mark = if ticked { "[x]" } else { "[ ]" };
        let cursor = if index == wizard.cursor { "›" } else { " " };
        let style = if index == wizard.cursor {
            theme::focused_panel()
        } else {
            theme::muted()
        };
        lines.push(Line::from(vec![
            Span::styled(format!(" {cursor} {mark} "), style),
            Span::styled(
                format!("{name:<19}"),
                if ticked { style } else { theme::muted() },
            ),
            Span::styled(*what, theme::muted()),
        ]));
    }

    frame.render_widget(
        Paragraph::new(Text::from(lines))
            .block(centered_block(" which hooks "))
            .wrap(Wrap { trim: false }),
        list_area,
    );
    frame.render_widget(
        Paragraph::new(Text::from(
            wizard
                .notes
                .iter()
                .map(|note| Line::from(Span::styled(format!(" {note}"), theme::muted())))
                .collect::<Vec<_>>(),
        ))
        .block(centered_block(" this repository ")),
        notes_area,
    );
}

fn draw_result(frame: &mut Frame, wizard: &Wizard, area: Rect) {
    let (text, style) = match &wizard.outcome {
        Some(Ok(text)) => (text.clone(), theme::ok()),
        Some(Err(error)) => (format!("{error}\nnothing was changed"), theme::warn()),
        None => ("nothing happened".to_string(), theme::muted()),
    };
    let mut lines = vec![Line::from(Span::styled(format!(" {text}"), style))];
    lines.push(Line::from(Span::styled(
        " a hook can always be skipped with git commit --no-verify",
        theme::muted(),
    )));
    frame.render_widget(
        Paragraph::new(Text::from(lines))
            .block(centered_block(" done "))
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn centered_block(title: &'static str) -> Block<'static> {
    Block::bordered()
        .border_type(BorderType::Rounded)
        .title(title)
}

#[cfg(test)]
mod tests {
    use super::{HOOKS, Wizard, draw};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn rows(terminal: &Terminal<TestBackend>) -> Vec<String> {
        let buffer = terminal.backend().buffer();
        (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect()
    }

    #[test]
    fn the_choices_are_all_ticked_and_named_with_the_way_out() {
        let wizard = Wizard::new();
        let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
        terminal.draw(|frame| draw(frame, &wizard)).unwrap();
        let screen = rows(&terminal).join("\n");
        for (name, _) in HOOKS {
            assert!(screen.contains(name), "{name} missing: {screen}");
        }
        assert!(screen.contains("[x]"), "{screen}");
        assert!(
            screen.contains("--no-verify"),
            "the way out is named: {screen}"
        );
        assert!(screen.contains("hooks go to"), "{screen}");
    }

    #[test]
    fn a_space_ticks_a_hook_off_and_the_screen_says_so() {
        let mut wizard = Wizard::new();
        wizard.handle_key(key(KeyCode::Down));
        wizard.handle_key(key(KeyCode::Char(' ')));
        assert!(!wizard.chosen[1], "{:?}", wizard.chosen);
        let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
        terminal.draw(|frame| draw(frame, &wizard)).unwrap();
        let screen = rows(&terminal).join("\n");
        assert!(screen.contains("[ ]"), "{screen}");
    }

    #[test]
    fn the_cursor_stops_at_the_edges() {
        let mut wizard = Wizard::new();
        for _ in 0..10 {
            wizard.handle_key(key(KeyCode::Down));
        }
        assert_eq!(wizard.cursor, HOOKS.len() - 1);
        for _ in 0..10 {
            wizard.handle_key(key(KeyCode::Up));
        }
        assert_eq!(wizard.cursor, 0);
    }

    #[test]
    fn esc_leaves_on_the_choice_screen_and_enter_ends_the_result() {
        let wizard = Wizard::new();
        assert!(wizard.finished(&key(KeyCode::Esc)));
        let mut done = Wizard::new();
        done.step = super::Step::Result;
        assert!(done.finished(&key(KeyCode::Enter)));
        assert!(
            done.finished(&key(KeyCode::Char('x'))),
            "any key leaves the result"
        );
    }
}
