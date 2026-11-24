# commit-tui

Terminal UI for writing and linting [Conventional Commits](https://www.conventionalcommits.org/).

Keyboard-first like [convinci](https://github.com/alexandrefelipea/convinci) and
[koji](https://github.com/cococonscious/koji), but with a live preview and a lint
pane instead of a step-by-step dialog.

```
╭ commit-tui ⚡ perf   performance improvement ────────────────── conventional ╮
│ subject 3/72   0 errors  0 warnings                                          │
╰──────────────────────────────────────────────────────────────────────────────╯
╭ compose ─────────────────────────────────────────────────────────────────────╮
│ type        [ perf ]                                                         │
│ scope         api                                                            │
│ subject     speed up the diff walk                                           │
│ body        walk once instead of twice                                       │
│                                                                              │
│ breaking      Hooks now receive the raw list                                 │
╰──────────────────────────────────────────────────────────────────────────────╯
╭ preview ──────────────────────────╮╭ checks · 0 errors · 0 warnings ──────────╮
│perf(api)!: speed up the diff walk ││✓ ready to commit                         │
│                                   ││                                          │
│walk once instead of twice         ││                                          │
│                                   ││                                          │
│BREAKING CHANGE: Hooks now receive ││                                          │
│the raw list                       ││                                          │
╰───────────────────────────────────╯╰──────────────────────────────────────────╯
```

Space opens the type picker, `1`–`9` jump straight to an entry, `0` takes the last:

```
╭ commit type · filter: _ ────────────────────────────────────╮
│▸ 1 feat         ✨ a new feature                            │
│  2 fix          🐛 a bug fix                                │
│  3 docs         📚 documentation only                       │
╰─────────────────────────────────────────────────────────────╯
```

On terminals smaller than 24 rows or 62 columns the layout collapses to the
active field only, so it stays usable over a phone-sized ssh session.

## Install

```sh
cargo install --path .
```

Rust 1.85 or newer. Needs `git` on PATH.

Prebuilt binaries come from the [releases](https://github.com/mhinte/commit-tui/releases):
`commit-tui-<version>-<target>.tar.gz` for Linux x86_64 (gnu and static musl),
Linux aarch64 (gnu and musl) and macOS (Intel and Apple silicon). Each archive
holds the binary, README and LICENSE, and `SHA256SUMS` in the release covers all
of them.

## Use

```sh
commit-tui                  # compose, print the message to stdout
commit-tui --commit         # compose, then run `git commit -m <message>`
git commit -m "$(commit-tui)"
```

`Ctrl+S` finishes. Without `--commit` the message goes to stdout, so the shell
hands it to git; with `--commit` git runs directly and its output (hooks,
passphrase prompts, errors) is what you see.

### Lint an existing message

```sh
commit-tui check --message "feat(api): add rate limit"
commit-tui check --file .git/COMMIT_EDITMSG
git log -1 --pretty=%B | commit-tui check
```

Exit code is 1 on errors, 0 otherwise. `--strict` also fails on warnings, which
is what you want in CI.

### First start

On the very first start the composer asks for your full name and derives a short
handle from it: first two letters of the surname, then the first three of the
given name. `Marc Hinte` becomes `himar`.

```
╭ commit-tui · first start ────────────────────────────╮
│  What is your full name?                             │
│                                                      │
│  Marc Hinte                                          │
│                                                      │
│  handle   himar                                      │
╰──────────────────────────────────────────────────────╯
  Enter save · Esc skip
```

Enter writes `~/.config/commit-tui/config.toml` (or `$XDG_CONFIG_HOME/commit-tui/`):

```toml
user = "Marc Hinte"
prefix = "himar"
```

Nothing else is written, so the rest keeps following defaults and project files.
The handle arrives as a suggested footer line, `Co-authored-by: himar`, which you
can edit or delete like any other text. Umlauts are spelled out (`Müller` -> `mu`,
`Jörg` -> `joe`), so the handle stays ASCII and usable in commit trailers.

Esc skips the dialog; `commit-tui --user "Firstname Lastname"` does the same job
non-interactively and takes precedence over any project file. `commit-tui config
--identity` prints which name and handle are active.

### Reuse an old commit as a template

`Ctrl+R` opens the last commits. The one you highlight is shown in full in the
pane on its right: message, author, date and the files it touched. `Enter` takes
over its **type, scope and body** and leaves the subject and footer alone, so the
pattern of an earlier message carries into the new one without retyping it.

```
╭ recent commits · filter: _ · Enter reuses type, scope and body ╮╭ commit ─────╮
│▸ 1 56589fb feat(composer) add pickers, profiles, hooks a…      ││feat(composer│
│  2 339e4ea feat           add conventional commit composer     ││): add picker│
╰────────────────────────────────────────────────────────────────╯╰─────────────╯
```

Typing filters the list, `Ctrl+R` is also available while another picker is open.

### commit-msg hook

```sh
commit-tui hooks install          # writes the hook
commit-tui hooks uninstall        # removes it again
commit-tui hooks install --force  # backs up a foreign hook and replaces it
```

The hook calls `commit-tui check --file "$1"`, so `commit-tui` must be on PATH.
An existing hook that is not ours is only replaced with `--force`, and then the
original is kept as `commit-msg.bak`.

### Configure it

```sh
commit-tui init     # writes .commit-tui.toml from the current defaults
commit-tui config   # shows the effective values and which files were read
commit-tui types    # the types of the active profile, one per line
```

Lookup order, later wins: built-in defaults, `~/.config/commit-tui/config.toml`,
`.commit-tui.toml` in the current directory. Lists replace each other rather than
merging, so a project file fully owns its own type list.

```toml
profile = "conventional"     # or "angular", or one you define below
emoji = true
history_depth = 500          # commits scanned for scope suggestions
max_subject = 72
recommended_subject = 50
scopes = ["api", "ui"]       # pinned scopes, listed before history

[[profiles]]
name = "work"
description = "House types"

[[profiles.types]]
name = "feat"
description = "user visible change"
emoji = "✨"

[[profiles.types]]
name = "ops"
description = "deployments and runbooks"
emoji = "🚀"
```

A broken file never blocks the composer: the warning shows up in the status bar
and the defaults stay active.

## Keys

| Key | Action |
|-----|--------|
| `Tab` / `Shift+Tab`, `↑` `↓` | move between fields (inside multiline fields `↑`/`↓` walk lines first) |
| `Enter` | next field or the type picker; new line in body, breaking and footer |
| `Space`, `Ctrl+T` | type picker: type to filter, `1`–`9` jump, `0` last |
| `Ctrl+N` | scope suggestions: config entries, then scopes from commit history |
| `Ctrl+P` | switch the type profile |
| `Ctrl+R` | reuse type, scope and body of a recent commit, with preview |
| `←` `→`, `h` `l` | previous/next type on the type field, cursor movement elsewhere |
| `j` `k` | next/previous field on the type field |
| `Ctrl+S` | finish: print the message, or commit with `--commit` |
| `Esc`, `Ctrl+Q` | quit without output |
| `?` | key help overlay |

## Rules

Errors block a commit:

- type missing, or not in the active profile
- empty subject
- subject longer than `max_subject`
- header without the `: ` separator, or a body that starts without a blank line
- `(` or `)` inside the scope

Warnings are advice: subject over `recommended_subject`, trailing period,
capitalised subject, past-tense verb (`added` instead of `add`), multi-word
scope, body or footer lines over 100 characters, a breaking change shorter than
ten characters.

Parsing is done by [`conventional_commit_parser`](https://crates.io/crates/conventional_commit_parser),
the crate [cocogitto](https://github.com/cocogitto/cocogitto) uses, so a message
that passes here also passes cocogitto. A `!` on its own is valid: Conventional
Commits makes the footer optional.

The `check` subcommand reads the same config, so hooks and the composer agree.

## Development

```sh
cargo test --all-targets      # unit tests and the CLI tests in tests/cli.rs
cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check
```

The pipeline lives in `.github/workflows`:

| Workflow | Trigger | Jobs |
|----------|---------|------|
| `ci.yml` | push to `main`, pull request | `fmt`, `clippy`, `test` (incl. an end-to-end run against a scratch repository), `commit-message` (this tool lints its own commits), `audit` |
| `release.yml` | tag `v*`, or manual dispatch | `verify` (reuses `ci.yml`), `build` (matrix of six targets), `publish` (archives, `SHA256SUMS`, GitHub release) |

A tag therefore cannot ship without passing the same checks as `main`.

```sh
git tag v0.2.0 && git push origin v0.2.0    # builds and publishes
```

The audit job is informational for now: `cargo audit` still flags advisory issues
in crates that `gix` pulls in for its development setup.

## Layout

```
src/
  main.rs      CLI: composer, check, init, config, types, hooks
  app.rs       state, field editing, pickers, key handling, event loop
  ui.rs        rendering: full form, compact view, pickers, help
  commit.rs    message model: render, parse, wrap
  validate.rs  lint rules, severities, configurable type set
  config.rs    profiles, defaults, config discovery
  git.rs       scopes from history, staged summary
  setup.rs     first-start dialog for the name and handle
  hooks.rs     commit-msg hook install/uninstall
```

`commit.rs`, `validate.rs` and `config.rs` have no terminal dependency, so other
front ends or a future library crate can reuse them unchanged.

## Credits

- [convinci](https://github.com/alexandrefelipea/convinci) by Alexandre Felipe:
  numbered quick-select in the picker, committing from the tool instead of
  printing, hook install/uninstall with a guard against foreign hooks, and the
  single-field view for small terminals.
- [koji](https://github.com/cococonscious/koji) by Finley Thomalla and Danny
  Tatom: searchable type picker, scope suggestions from history, config profiles.
- [cocogitto](https://github.com/cocogitto/cocogitto): the parser this project
  validates with.

All three are MIT licensed.

## Roadmap

- pre-fill scope and type from the staged diff
- remember recently used scopes per repository
- `--json` output from `check` for CI dashboards

## License

MIT, see [LICENSE](LICENSE).
