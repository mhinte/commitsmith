//! The setup dialog inside the composer: `S` opens it, Esc leaves without change.
//!
//! One question at a time in the same visual language as the first-start dialog,
//! because a form with seven fields in a terminal is a worse form than seven
//! questions with one field each. Every answer is validated by
//! [`SetupAnswers`], so a typo keeps you on the question instead of writing junk.

use crate::ui::theme;
use std::io;

use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::layout::{Constraint, Layout, Position};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, BorderType, Paragraph};

use commitsmith::domain::setup::SetupAnswers;

/// What the dialog ended with.
pub enum Outcome {
    Saved(SetupAnswers),
    Cancelled,
}

/// The questions, in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    Author,
    Prefix,
    Reference,
    Template,
    Profile,
    Emoji,
    Footer,
    Alias,
}

const STEPS: [Step; 8] = [
    Step::Author,
    Step::Prefix,
    Step::Reference,
    Step::Template,
    Step::Profile,
    Step::Emoji,
    Step::Footer,
    Step::Alias,
];

impl Step {
    fn question(self) -> &'static str {
        match self {
            Self::Author => "Author shortcut for branches (a-z, 0-9, ., -, _)",
            Self::Prefix => "Ticket prefix, e.g. TICKET",
            Self::Reference => "Footer keyword (Refs or Closes)",
            Self::Template => "Branch template ({type}/{author}/{ticket}/{slug})",
            Self::Profile => "Commit profile",
            Self::Emoji => "Emojis in the type picker? (y/n)",
            Self::Footer => "Add `Co-authored-by:` to the commit footer? (y/n)",
            Self::Alias => "Install `git smith` alias? (y/n)",
        }
    }

    fn value(self, answers: &SetupAnswers) -> String {
        match self {
            Self::Author => answers.author.as_str().to_string(),
            Self::Prefix => answers.ticket_prefix.clone(),
            Self::Reference => answers.reference.to_string(),
            Self::Template => answers.branch_template.as_str().to_string(),
            Self::Profile => answers.profile.clone(),
            Self::Emoji => yes_no(answers.emoji),
            Self::Footer => yes_no(answers.co_author),
            Self::Alias => yes_no(answers.git_alias),
        }
    }
}

fn yes_no(value: bool) -> String {
    if value { "y" } else { "n" }.to_string()
}

pub struct Wizard {
    answers: SetupAnswers,
    step: usize,
    input: String,
    error: Option<String>,
    done: Option<Outcome>,
    profiles: Vec<String>,
}

impl Wizard {
    pub fn new(answers: SetupAnswers, profiles: Vec<String>) -> Self {
        let input = STEPS[0].value(&answers);
        Self {
            answers,
            step: 0,
            input,
            error: None,
            done: None,
            profiles,
        }
    }

    fn current(&self) -> Step {
        STEPS[self.step.min(STEPS.len() - 1)]
    }

    fn insert(&mut self, c: char) {
        if c.is_control() {
            return;
        }
        self.input.push(c);
        self.error = None;
    }

    fn backspace(&mut self) {
        self.input.pop();
        self.error = None;
    }

    /// Apply the input to the answers; on success move on, on failure stay.
    fn submit(&mut self) {
        let value = self.input.trim().to_string();
        let step = self.current();
        let result = match step {
            Step::Author => self.answers.clone().with_author(&value),
            Step::Prefix => self.answers.clone().with_ticket_prefix(&value),
            Step::Reference => self.answers.clone().with_reference(&value),
            Step::Template => self.answers.clone().with_template(&value),
            Step::Profile => self.answers.clone().with_profile(&value, &self.profiles),
            Step::Emoji => {
                let mut next = self.answers.clone();
                next.emoji = truthy(&value, self.answers.emoji);
                Ok(next)
            }
            Step::Footer => {
                let mut next = self.answers.clone();
                next.co_author = truthy(&value, self.answers.co_author);
                Ok(next)
            }
            Step::Alias => {
                let mut next = self.answers.clone();
                next.git_alias = truthy(&value, self.answers.git_alias);
                Ok(next)
            }
        };
        match result {
            Ok(next) => {
                self.answers = next;
                self.error = None;
                if self.step + 1 >= STEPS.len() {
                    self.done = Some(Outcome::Saved(self.answers.clone()));
                } else {
                    self.step += 1;
                    self.input = self.current().value(&self.answers);
                }
            }
            Err(error) => self.error = Some(error.to_string()),
        }
    }

    fn handle_key(&mut self, key: KeyEvent) {
        if key.kind == KeyEventKind::Release || self.done.is_some() {
            return;
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Enter => self.submit(),
            KeyCode::Esc => self.done = Some(Outcome::Cancelled),
            KeyCode::Char('c') if ctrl => self.done = Some(Outcome::Cancelled),
            // Ctrl+U empties the line, so a prefilled value does not have to be
            // deleted character by character.
            KeyCode::Char('u') if ctrl => {
                self.input.clear();
                self.error = None;
            }
            KeyCode::Backspace => self.backspace(),
            KeyCode::Char(c) if !ctrl => self.insert(c),
            _ => {}
        }
    }
}

fn truthy(text: &str, fallback: bool) -> bool {
    match text.to_lowercase().as_str() {
        "y" | "yes" | "j" | "ja" => true,
        "n" | "no" | "nein" => false,
        _ => fallback,
    }
}

/// Run the dialog until it is saved or cancelled.
pub fn run(terminal: &mut DefaultTerminal, wizard: &mut Wizard) -> io::Result<Outcome> {
    let mut outcome: Option<Outcome> = None;
    crate::ui::event_loop(
        terminal,
        wizard,
        || match crate::ui::poll_tick()? {
            true => Ok(Some(crate::ui::read_event()?)),
            false => Ok(None),
        },
        |wizard, event| {
            match event {
                Some(Event::Key(key)) => wizard.handle_key(key),
                Some(Event::Paste(text)) => {
                    for c in text.chars().filter(|c| !c.is_control()) {
                        wizard.insert(c);
                    }
                }
                _ => {}
            }
            outcome = wizard.done.take();
            outcome.is_some()
        },
        draw,
    )?;
    Ok(outcome.expect("the loop only returns once the wizard is done"))
}

fn draw(frame: &mut ratatui::Frame, wizard: &Wizard) {
    let area = frame.area();
    let [_, body, _] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(8),
        Constraint::Length(1),
    ])
    .areas(area);

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(theme::accent())
        .title(Span::styled(
            format!(" commitsmith · setup {}/{} ", wizard.step + 1, STEPS.len()),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ));
    frame.render_widget(ratatui::widgets::Clear, body);
    let inner = block.inner(body);
    frame.render_widget(block, body);

    let [form, hint] = Layout::vertical([Constraint::Min(5), Constraint::Length(2)]).areas(inner);

    let mut lines = vec![
        Line::styled(
            format!("  {}", wizard.current().question()),
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Line::raw(""),
        Line::from(vec![
            Span::raw("  "),
            Span::styled(wizard.input.clone(), theme::bright()),
        ]),
    ];
    if let Some(error) = &wizard.error {
        lines.push(Line::raw(""));
        lines.push(Line::styled(format!("  {error}"), theme::bad()));
    }
    frame.render_widget(Paragraph::new(Text::from(lines)), form);
    frame.render_widget(
        Paragraph::new(Text::from(vec![
            Line::styled(
                "  Enter next · Ctrl+U clear the line · Esc cancel without saving",
                theme::hint(),
            ),
            Line::styled(
                "  written to the user config, not to this repository",
                theme::muted(),
            ),
        ])),
        hint,
    );

    let typed = wizard.input.chars().count() as u16;
    frame.set_cursor_position(Position::new(form.x + 2 + typed, form.y + 2));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wizard() -> Wizard {
        Wizard::new(
            SetupAnswers::defaults(&["conventional".to_string(), "angular".to_string()]),
            vec!["conventional".to_string(), "angular".to_string()],
        )
    }

    fn press(wizard: &mut Wizard, code: KeyCode) {
        wizard.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
    }

    fn replace(wizard: &mut Wizard, text: &str) {
        wizard.input.clear();
        for c in text.chars() {
            press(wizard, KeyCode::Char(c));
        }
    }

    #[test]
    fn every_step_advances_and_the_last_one_saves() {
        let mut wizard = wizard();
        for (value, step) in [
            ("mhi", Step::Author),
            ("ABC", Step::Prefix),
            ("Closes", Step::Reference),
            ("{type}/{author}/{slug}", Step::Template),
            ("angular", Step::Profile),
            ("y", Step::Emoji),
            ("n", Step::Footer),
            ("n", Step::Alias),
        ] {
            assert_eq!(wizard.current(), step);
            replace(&mut wizard, value);
            press(&mut wizard, KeyCode::Enter);
        }
        match wizard.done {
            Some(Outcome::Saved(answers)) => {
                assert_eq!(answers.author.as_str(), "mhi");
                assert_eq!(answers.ticket_prefix, "ABC");
                assert_eq!(answers.profile, "angular");
                assert!(answers.emoji);
                // The footer question is part of the same dialog now.
                assert!(!answers.co_author);
                assert!(!answers.git_alias);
            }
            _ => panic!("expected a saved outcome"),
        }
    }

    #[test]
    fn a_bad_answer_keeps_the_question_open() {
        let mut wizard = wizard();
        replace(&mut wizard, "MH");
        press(&mut wizard, KeyCode::Enter);
        assert_eq!(wizard.current(), Step::Author);
        assert!(wizard.error.as_deref().unwrap_or("").contains("lower case"));
        assert!(wizard.done.is_none());
    }

    #[test]
    fn esc_cancels_without_saving() {
        let mut wizard = wizard();
        press(&mut wizard, KeyCode::Esc);
        assert!(matches!(wizard.done, Some(Outcome::Cancelled)));
    }

    #[test]
    fn ctrl_u_empties_a_prefilled_answer() {
        let mut wizard = wizard();
        wizard.handle_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
        assert!(wizard.input.is_empty());
        for c in "mhi".chars() {
            press(&mut wizard, KeyCode::Char(c));
        }
        press(&mut wizard, KeyCode::Enter);
        // Accepted, so the wizard moved on with the new value.
        assert_eq!(wizard.current(), Step::Prefix);
        assert_eq!(wizard.answers.author.as_str(), "mhi");
    }

    #[test]
    fn backspace_edits_the_current_answer() {
        let mut wizard = wizard();
        replace(&mut wizard, "mm");
        press(&mut wizard, KeyCode::Backspace);
        assert_eq!(wizard.input, "m");
        press(&mut wizard, KeyCode::Enter);
        assert_eq!(wizard.current(), Step::Author, "one character is too short");
    }

    #[test]
    fn the_next_question_starts_from_the_stored_value() {
        let mut wizard = wizard();
        replace(&mut wizard, "mhi");
        press(&mut wizard, KeyCode::Enter);
        assert_eq!(wizard.current(), Step::Prefix);
        assert_eq!(wizard.input, "TICKET");
    }
}
