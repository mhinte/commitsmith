//! Tab 2 as a whole: the branch list, which pane has the keyboard, the wizard.

use super::*;

#[derive(Default)]
pub struct BranchesView {
    /// The branches and their sync state.
    pub list: BranchList,
    /// Which pane has the keyboard.
    pub panel: Panel,
    /// The conventional branch wizard, when it is open.
    pub wizard: BranchWizard,
}
