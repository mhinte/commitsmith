use std::process::ExitCode;

/// `commitsmith hints`: say what is worth saying about the index, then get out
/// of the way.
///
/// Always exit 0. The hints go to stderr so a hook never writes into a pipe
/// somebody parses, and a broken installation (no repository, unreadable index)
/// is not a reason to stop a commit.
/// `commitsmith update`: fetch the newest release, verify it, put it in place.
///
/// Exit codes are part of the contract: 0 means "nothing to do or done", 1 means
/// "something went wrong", 2 means "there is a newer release" — so a script can
/// ask with `--check` and act on the answer.
pub(crate) fn run_update(check_only: bool) -> ExitCode {
    use commitsmith::infrastructure::update::{self, Outcome};

    match update::run(check_only) {
        Ok(Outcome::UpToDate { .. }) => {
            println!(
                "commitsmith {} is the newest release",
                env!("CARGO_PKG_VERSION")
            );
            ExitCode::SUCCESS
        }
        Ok(Outcome::Updated { from, to }) => {
            if check_only {
                println!("commitsmith {to} is available (you run {from})");
                println!("run `commitsmith update` to install it");
                return ExitCode::from(2);
            }
            let pending = std::env::current_exe()
                .ok()
                .is_some_and(|path| commitsmith::infrastructure::update::is_pending(&path));
            match pending {
                true => println!(
                    "commitsmith {from} -> {to}\nthe new binary is staged and replaces this one when it exits"
                ),
                false => println!("commitsmith {from} -> {to}"),
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("commitsmith: {error}");
            ExitCode::FAILURE
        }
    }
}
