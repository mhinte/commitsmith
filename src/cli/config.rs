use std::process::ExitCode;

use super::load_config;
use crate::ui;

#[allow(unused_imports)]
use commitsmith::domain::ports::ConfigStore as _;

pub(crate) fn show_config(identity: bool) -> ExitCode {
    let (config, warnings) = load_config();
    if identity {
        println!("user:   {}", config.user.as_deref().unwrap_or("-"));
        println!("handle: {}", config.prefix.as_deref().unwrap_or("-"));
        if config.user.is_none() {
            println!(
                "       set one with `commitsmith --user \"Firstname Lastname\"` or on first start"
            );
        }
        return ExitCode::SUCCESS;
    }
    println!("profile: {}", config.profile);
    println!("user:    {}", config.user.as_deref().unwrap_or("-"));
    println!("author:  {}", config.author.as_deref().unwrap_or("-"));
    println!("tickets: {}-1234", config.ticket_prefix);
    println!("footer:  {}", config.reference);
    println!(
        "co-author: {}",
        if config.co_author {
            config
                .co_author_name
                .clone()
                .or_else(|| config.prefix.clone())
                .unwrap_or_else(|| "on, but no handle yet".to_string())
        } else {
            "off".to_string()
        }
    );
    println!("branch:  {}", config.branch_template);
    println!("types:   {}", config.type_names().join(", "));
    println!("scopes:  {}", join_or_dash(&config.scopes));
    println!(
        "limits:  subject {}/{} chars, history depth {}",
        config.recommended_subject, config.max_subject, config.history_depth
    );
    println!("emoji:   {}", if config.emoji { "on" } else { "off" });
    println!("files:");
    if config.sources.is_empty() {
        println!("  (none, using built-in defaults)");
    } else {
        for source in &config.sources {
            println!("  {}", source.display());
        }
    }
    for warning in warnings {
        eprintln!("warning: {warning}");
    }
    ExitCode::SUCCESS
}

pub(crate) fn join_or_dash(items: &[String]) -> String {
    if items.is_empty() {
        "-".to_string()
    } else {
        items.join(", ")
    }
}

/// `commitsmith init`: ask the setup questions and store them for this user.
///
/// The project file is still written when one exists or `--force` is given, so
/// the wizard can also provide a repository default without touching the user
/// config.
pub(crate) fn init_config(defaults: bool, force: bool) -> ExitCode {
    let store = commitsmith::infrastructure::config_toml::TomlConfigStore::new();
    let existing = store.user_document().ok().flatten();
    // `--defaults` skips the questions, not the confirmation: an existing file
    // is only replaced on request.
    if existing.is_some() && !force {
        eprintln!(
            "commitsmith: {} exists, pass --force to overwrite",
            existing
                .as_ref()
                .map(|doc| doc.path.display().to_string())
                .unwrap_or_default()
        );
        return ExitCode::FAILURE;
    }

    let (config, _) = load_config();
    let profiles: Vec<String> = config.profiles.keys().cloned().collect();
    let current = if existing.is_some() {
        Some(
            commitsmith::domain::setup::SetupAnswers::defaults(&profiles)
                .with_author(config.author.as_deref().unwrap_or("mmu"))
                .unwrap_or_else(|_| commitsmith::domain::setup::SetupAnswers::defaults(&profiles))
                .with_ticket_prefix(&config.ticket_prefix)
                .unwrap_or_else(|_| commitsmith::domain::setup::SetupAnswers::defaults(&profiles))
                .with_reference(&config.reference)
                .unwrap_or_else(|_| commitsmith::domain::setup::SetupAnswers::defaults(&profiles)),
        )
    } else {
        None
    };

    let Some(answers) = ui::init::ask(&ui::init::Options { defaults }, &profiles, current.as_ref())
    else {
        eprintln!("commitsmith: setup aborted, nothing written");
        return ExitCode::FAILURE;
    };

    match commitsmith::infrastructure::config_toml::save_setup(&store, &answers) {
        Ok(path) => {
            println!("wrote {}", path.display());
            println!("author   {}", answers.author);
            println!("tickets  {}-1234", answers.ticket_prefix);
            println!("footer   {}", answers.reference);
            println!("branches {}", answers.branch_template);
            if answers.git_alias {
                let binary = std::env::current_exe()
                    .map(|path| path.display().to_string())
                    .unwrap_or_else(|_| "commitsmith".to_string());
                match commitsmith::infrastructure::git_cli::install_alias("smith", &binary) {
                    Ok(()) => println!("git alias: git smith"),
                    Err(error) => eprintln!("commitsmith: alias not set: {error}"),
                }
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("commitsmith: {error}");
            ExitCode::FAILURE
        }
    }
}
