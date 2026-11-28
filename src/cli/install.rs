use std::process::ExitCode;

use crate::config::Config;
use crate::install;

pub(crate) fn run_install(alias: Option<&str>) -> ExitCode {
    match install::install(alias) {
        Ok(report) => {
            println!("installed {}", report.binary.display());
            if let Some(alias) = &report.alias {
                println!("alias     {}", alias.display());
            }
            println!("directory {}", report.directory.display());
            if report.added_to_path {
                println!("added the directory to your PATH");
                if report.restart_shell {
                    println!("open a new terminal so it takes effect");
                }
            } else {
                println!("the directory was already on your PATH");
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("commitsmith: {error}");
            ExitCode::FAILURE
        }
    }
}

pub(crate) fn run_uninstall() -> ExitCode {
    match install::uninstall() {
        Ok(report) => {
            println!("removed {}", report.binary.display());
            if let Some(alias) = &report.alias {
                println!("removed {}", alias.display());
            }
            println!(
                "the PATH entry stays; remove {} by hand if you want it gone",
                report.directory.display()
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("commitsmith: {error}");
            ExitCode::FAILURE
        }
    }
}

/// `--user "Full Name"`: write the identity, print the derived handle.
pub(crate) fn set_user(name: &str) -> ExitCode {
    match Config::save_identity(name) {
        Ok(path) => {
            let prefix = crate::config::prefix_from_name(name).unwrap_or_default();
            println!("wrote {}", path.display());
            println!("user:   {}", name.trim());
            println!("handle: {prefix}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("commitsmith: {error}");
            ExitCode::FAILURE
        }
    }
}
