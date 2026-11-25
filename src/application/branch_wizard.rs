//! The conventional branch wizard of tab 2.
//!
//! It turns three inputs — kind, ticket, description — into a branch name from
//! the template in the config. The naming rules live in the domain; this holds
//! what the user typed and what went wrong.

use std::fmt;

use commitsmith::domain::branch::{
    BranchError, BranchKind, BranchTemplate, ConventionalBranchName,
};
use commitsmith::domain::errors::GitError;
use commitsmith::domain::ports::GitRepository;
use commitsmith::domain::ticket::TicketId;

/// Which input has the focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WizardField {
    Kind,
    Ticket,
    Description,
}

impl WizardField {
    pub const ALL: [Self; 3] = [Self::Kind, Self::Ticket, Self::Description];

    pub fn label(self) -> &'static str {
        match self {
            Self::Kind => "kind",
            Self::Ticket => "ticket",
            Self::Description => "description",
        }
    }

    pub fn index(self) -> usize {
        Self::ALL
            .iter()
            .position(|field| *field == self)
            .unwrap_or(0)
    }

    /// The next field, wrapping at the end.
    pub fn next(self) -> Self {
        Self::ALL[(self.index() + 1) % Self::ALL.len()]
    }

    pub fn previous(self) -> Self {
        Self::ALL[(self.index() + Self::ALL.len() - 1) % Self::ALL.len()]
    }
}

/// State of the wizard. Closed means tab 2 shows the list alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchWizard {
    pub open: bool,
    pub field: WizardField,
    pub kind: BranchKind,
    pub ticket: String,
    pub description: String,
    /// Set when the last attempt failed, shown under the preview.
    pub error: Option<String>,
}

impl Default for BranchWizard {
    fn default() -> Self {
        Self {
            open: false,
            field: WizardField::Kind,
            kind: BranchKind::Feat,
            ticket: String::new(),
            description: String::new(),
            error: None,
        }
    }
}

/// What to tell the user when the name cannot be built yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WizardPreview {
    pub name: String,
    pub is_complete: bool,
}

impl BranchWizard {
    /// Open with the defaults; closing keeps nothing.
    pub fn start(&mut self) {
        self.open = true;
        self.field = WizardField::Kind;
        self.kind = BranchKind::Feat;
        self.ticket.clear();
        self.description.clear();
        self.error = None;
    }

    pub fn close(&mut self) {
        self.open = false;
        self.error = None;
    }

    /// The name the current inputs produce.
    ///
    /// `template` comes from the config and may be nonsense; then the standard
    /// template is used instead of refusing to work.
    pub fn preview(
        &self,
        author: Option<&str>,
        ticket_prefix: &str,
        template: &str,
    ) -> Result<WizardPreview, BranchError> {
        let author = author.unwrap_or_default();
        if author.trim().is_empty() {
            return Err(BranchError::Part(
                "author",
                "no author yet · run `commitsmith init`".to_string(),
            ));
        }
        let ticket = match self.ticket.trim() {
            "" => None,
            value => Some((value, ticket_prefix)),
        };
        let configured =
            BranchTemplate::parse(template).unwrap_or_else(|_| BranchTemplate::standard());
        // Without a ticket the four-part template would leave an empty path
        // segment; the shorter form is used instead.
        let template = if ticket.is_none() && configured.uses_ticket() {
            BranchTemplate::without_ticket()
        } else {
            configured
        };

        // An empty description is not an error, it is an unfinished form: show
        // the shape the name will have instead of scolding the user.
        if self.description.trim().is_empty() {
            let mut shape = template.as_str().to_string();
            shape = shape.replace("{type}", self.kind.as_str());
            shape = shape.replace("{author}", author);
            shape = shape.replace("{slug}", "<description>");
            if let Some((value, prefix)) = ticket {
                let ticket = TicketId::parse(value, prefix)?;
                shape = shape.replace("{ticket}", &ticket.to_string());
            }
            return Ok(WizardPreview {
                name: shape,
                is_complete: false,
            });
        }

        let problem = ConventionalBranchName::from_parts(
            self.kind,
            author,
            ticket,
            self.description.as_str(),
        )?;
        let rendered = problem.render(&template)?;
        Ok(WizardPreview {
            is_complete: !rendered.contains("//"),
            name: rendered,
        })
    }

    /// Create the branch and check it out. On success the wizard closes.
    pub fn create(
        &mut self,
        repo: &dyn GitRepository,
        author: Option<&str>,
        ticket_prefix: &str,
        template: &str,
    ) -> Result<String, String> {
        let preview = self
            .preview(author, ticket_prefix, template)
            .map_err(describe)?;
        if !preview.is_complete {
            let message = "describe the change first".to_string();
            self.error = Some(message.clone());
            return Err(message);
        }
        let checkout_failure = repo
            .create_branch(&preview.name)
            .map_err(describe_git)
            .and_then(|()| repo.checkout(&preview.name).map_err(describe_git));
        if let Err(message) = checkout_failure {
            self.error = Some(message.clone());
            return Err(message);
        }
        self.close();
        Ok(preview.name)
    }

    pub fn push_char(&mut self, c: char) {
        match self.field {
            WizardField::Description => self.description.push(c),
            WizardField::Ticket => self.ticket.push(c),
            WizardField::Kind => {}
        }
        self.error = None;
    }

    pub fn backspace(&mut self) {
        match self.field {
            WizardField::Description => {
                self.description.pop();
            }
            WizardField::Ticket => {
                self.ticket.pop();
            }
            WizardField::Kind => {}
        }
        self.error = None;
    }

    pub fn cycle_kind(&mut self, forward: bool) {
        let kinds = BranchKind::ALL;
        let index = kinds
            .iter()
            .position(|kind| *kind == self.kind)
            .unwrap_or(0);
        let next = if forward {
            (index + 1) % kinds.len()
        } else {
            (index + kinds.len() - 1) % kinds.len()
        };
        self.kind = kinds[next];
        self.error = None;
    }
}

/// Show a domain error in the wizard without the enum wrapper.
fn describe(error: BranchError) -> String {
    error.to_string()
}

/// Same for the git error: the message is what the user reads.
fn describe_git(error: GitError) -> String {
    error.to_string()
}

impl fmt::Display for WizardField {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        out.write_str(self.label())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open() -> BranchWizard {
        let mut wizard = BranchWizard::default();
        wizard.start();
        wizard
    }

    fn standard() -> (&'static str, &'static str) {
        ("hima", "{type}/{author}/{ticket}/{slug}")
    }

    #[test]
    fn the_name_is_built_from_kind_author_ticket_and_slug() {
        let mut wizard = open();
        wizard.ticket = "1".to_string();
        wizard.description = "Add a thing".to_string();
        let (author, template) = standard();
        let preview = wizard.preview(Some(author), "TICKET", template).unwrap();
        assert_eq!(preview.name, "feat/hima/TICKET-1/add-a-thing");
        assert!(preview.is_complete);
    }

    #[test]
    fn without_a_ticket_the_shorter_template_is_used() {
        let mut wizard = open();
        wizard.description = "Add a thing".to_string();
        let (author, template) = standard();
        let preview = wizard.preview(Some(author), "TICKET", template).unwrap();
        assert_eq!(preview.name, "feat/hima/add-a-thing", "no empty segment");
        assert!(!preview.name.contains("//"));
    }

    #[test]
    fn a_missing_author_says_what_to_do() {
        let mut wizard = open();
        wizard.description = "x".to_string();
        let error = wizard
            .preview(None, "TICKET", "{type}/{author}/{slug}")
            .unwrap_err();
        assert!(error.to_string().contains("commitsmith init"), "{error}");
    }

    #[test]
    fn a_nameless_description_is_not_complete() {
        let wizard = open();
        let preview = wizard
            .preview(Some("hima"), "TICKET", "{type}/{author}/{slug}")
            .unwrap();
        assert!(!preview.is_complete);
    }

    #[test]
    fn a_broken_template_falls_back_to_the_standard_one() {
        let mut wizard = open();
        wizard.ticket = "9".to_string();
        wizard.description = "Fix it".to_string();
        let preview = wizard
            .preview(Some("hima"), "TICKET", "{nonsense}/{slug}")
            .unwrap();
        assert_eq!(preview.name, "feat/hima/TICKET-9/fix-it");
    }

    #[test]
    fn the_kind_cycles_through_all_branch_kinds() {
        let mut wizard = open();
        assert_eq!(wizard.kind, BranchKind::Feat);
        wizard.cycle_kind(true);
        assert_eq!(wizard.kind, BranchKind::Fix);
        wizard.cycle_kind(false);
        assert_eq!(wizard.kind, BranchKind::Feat);
        for _ in 0..BranchKind::ALL.len() {
            wizard.cycle_kind(false);
        }
        assert_eq!(wizard.kind, BranchKind::Feat, "wraps around");
    }

    #[test]
    fn creating_without_a_description_refuses_and_explains() {
        use crate::application::test_support::RecordingGit;
        let repo = RecordingGit::new(&["main"], "main");
        let mut wizard = open();
        let error = wizard
            .create(&repo, Some("hima"), "TICKET", "{type}/{author}/{slug}")
            .unwrap_err();
        assert!(error.contains("describe"), "{error}");
        assert_eq!(wizard.error.as_deref(), Some(error.as_str()));
        assert!(repo.calls().borrow().is_empty(), "nothing reached git");
    }
}
