# commitsmith

Terminal UI for writing and linting [Conventional Commits](https://www.conventionalcommits.org/).

`commitsmith` forges commits into shape: the smith of your commit messages.
Short alias: `smith`.

Keyboard-first like [convinci](https://github.com/alexandrefelipea/convinci) and
[koji](https://github.com/cococonscious/koji), but with a live preview and a lint
pane instead of a step-by-step dialog.

```
╭ commitsmith ⚡ perf   performance improvement ────────────────── conventional ╮
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

### Linux

```sh
curl -sSL https://raw.githubusercontent.com/mhinte/commitsmith/main/scripts/install.sh | sh
```

Legt `commitsmith` in `~/.local/bin` ab, optional zusätzlich einen Alias:

```sh
./scripts/install.sh --alias smith   # ~/.local/bin/smith zeigt auf commitsmith
```

Auch hier macht die Exe die Arbeit (`commitsmith --install`): `~/.local/bin`,
PATH-Zeile in `.profile`/`.bashrc`/`.zshrc` falls nötig, Alias optional.

### Windows

```powershell
iwr https://raw.githubusercontent.com/mhinte/commitsmith/main/scripts/install.ps1 -OutFile install.ps1; .\install.ps1
```

Das Skript lädt das Release-Archiv und ruft `commitsmith --install` auf. Die Exe
macht die eigentliche Arbeit: sie kopiert sich nach
`%LOCALAPPDATA%\Programs\commitsmith\commitsmith.exe` und hängt das Verzeichnis an
den **Benutzer**-PATH (Registry, keine Adminrechte). Ein neues Terminal übernimmt
die Änderung. Optional mit kurzem Zweitnamen:

```powershell
.\install.ps1 -Alias smith        # zusätzlich smith.exe neben commitsmith.exe
```

Wer das Archiv schon entpackt hat, braucht gar kein Skript:

```powershell
.\commitsmith.exe --install --alias smith
```

`commitsmith --uninstall` entfernt die Kopien wieder; der PATH-Eintrag bleibt.

### Aus dem Quellcode

```sh
cargo install --path .
```

Rust 1.85 or newer. Needs `git` on PATH.

Prebuilt binaries come from the [releases](https://github.com/mhinte/commitsmith/releases):
`commitsmith-<version>-<target>.tar.gz` for Linux x86_64 (gnu and static musl) and
`commitsmith-<version>-x86_64-pc-windows-msvc.zip` for Windows. Each archive holds
the binary, README and LICENSE, and `SHA256SUMS` covers all of them. Other targets
are not built: nobody here could test them, and an untested binary is worse than
none.

## Use

```sh
commitsmith                  # compose, print the message to stdout
commitsmith --commit         # compose, then run `git commit -m <message>`
git commit -m "$(commitsmith)"
```

`Ctrl+S` finishes, but only when the message has no errors: the composer stays
open and names the first problem otherwise. Without `--commit` the message goes
to stdout and the footer says `C-s message`; with `--commit` git runs directly,
the footer says `C-s commit`, and git's output (hooks, passphrase prompts,
errors) is what you see. If git refuses, the message is printed again so it can
be retried.

`Ctrl+K` clears the field under the cursor — one field, nothing else. The type is
a choice from the picker rather than text, so it stays.

### Lint an existing message

```sh
commitsmith check --message "feat(api): add rate limit"
commitsmith check --file .git/COMMIT_EDITMSG
git log -1 --pretty=%B | commitsmith check
```

Exit code is 1 on errors, 0 otherwise. `--strict` also fails on warnings, which
is what you want in CI.

### First start

On the very first start the composer asks for your full name and derives a short
handle from it: first two letters of the surname, then the first three of the
given name. `Marc Hinte` becomes `himar`.

```
╭ commitsmith · first start ────────────────────────────╮
│  What is your full name?                             │
│                                                      │
│  Marc Hinte                                          │
│                                                      │
│  handle   himar                                      │
╰──────────────────────────────────────────────────────╯
  Enter save · Esc skip
```

Enter writes `~/.config/commitsmith/config.toml` (or `$XDG_CONFIG_HOME/commitsmith/`):

```toml
user = "Marc Hinte"
prefix = "himar"
```

Nothing else is written, so the rest keeps following defaults and project files.
The handle arrives as a suggested footer line, `Co-authored-by: himar`, which you
can edit or delete like any other text. Umlauts are spelled out (`Müller` -> `mu`,
`Jörg` -> `joe`), so the handle stays ASCII and usable in commit trailers.

Esc skips the dialog; `commitsmith --user "Firstname Lastname"` does the same job
non-interactively and takes precedence over any project file. `commitsmith config
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
commitsmith hooks install          # writes the hook
commitsmith hooks uninstall        # removes it again (and restores a chained hook)
commitsmith hooks install --force  # install into a husky/lefthook/pre-commit directory
```

The hook goes where git reads hooks: `.git/hooks`, or wherever `core.hooksPath`
points. It finds the binary through `git config commitsmith.binary` (set by
`install`), then `PATH`, then `~/.local/bin` and `~/.cargo/bin`. Without a binary
it prints one line and lets the commit through, so colleagues without the tool
are never blocked.

An existing hook is never replaced. It moves to `commit-msg.pre-commitsmith`,
runs first, and its exit code stands; `uninstall` puts it back. A directory that
husky, lefthook or pre-commit regenerates needs `--force`, because the next
`npm install` or `lefthook install` would drop the hook silently — register
`commitsmith check --file "$1"` in that tool instead.

### Configure it

```sh
scripts/release-local.sh v0.11.1          # build a release here, no runner minutes
scripts/release-local.sh v0.11.1 --publish # … and upload it to GitHub
commitsmith init     # writes .commitsmith.toml from the current defaults
commitsmith config   # shows the effective values and which files were read
commitsmith types    # the types of the active profile, one per line
```

Lookup order, later wins: built-in defaults, `~/.config/commitsmith/config.toml`,
`.commitsmith.toml` in the current directory. Lists replace each other rather than
merging, so a project file fully owns its own type list.

```toml
profile = "conventional"     # or "angular", or one you define below
emoji = true
history_depth = 500          # commits scanned for scope suggestions
max_subject = 72
recommended_subject = 50
scopes = ["api", "ui"]       # pinned scopes, listed before history

author = "hima"              # short name in branch names
ticket_prefix = "TICKET"     # TICKET-1234
reference = "Refs"           # or "Closes", the keyword before the ticket
branch_template = "{type}/{author}/{ticket}/{slug}"
co_author = true             # add "Co-authored-by: <handle>" to a new footer
co_author_name = "Marc Hinte <marc@example.com>"   # optional, replaces the handle

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

The footer of a new message is filled while it is still empty, and only then: a
branch that carries a ticket contributes `Refs TICKET-1234`, and `co_author`
adds the co-author line. Text you type yourself is never replaced — not even
when you switch branches. `co_author = false` leaves the line out.

The commit type follows the branch as long as you have not picked one yourself:
`hotfix/...` and `fix/...` mean `fix`, `docs/...` means `docs`, `chore/...` and
`release/...` mean `chore`. A branch that is not conventional, or a kind your
profile does not offer, leaves the current type alone.

## Keys

| Key | Action |
|-----|--------|
| `Tab` / `Shift+Tab`, `↑` `↓` | move between fields (inside multiline fields `↑`/`↓` walk lines first) |
| `Enter` | next field or the type picker; new line in body, breaking and footer |
| `Space`, `Ctrl+T` | type picker: type to filter, `1`–`9` jump, `0` last |
| `Ctrl+N` | scope suggestions: config entries, then scopes from commit history |
| `Ctrl+P` | switch the type profile |
| `Ctrl+K` | clear the field under the cursor (the type stays) |
| `Ctrl+R` | reuse type, scope and body of a recent commit, with preview |
| `←` `→`, `h` `l` | previous/next type on the type field, cursor movement elsewhere |
| `Ctrl+Space` | suggestions for the field: type, scope, recent subjects |
| `Ctrl+Enter`, `Ctrl+S` | finish: print the message, or commit with `--commit` |
| `Esc`, `Ctrl+Q` | quit without output |
| `?` | key help overlay, only what the current tab and pane can do |

On the file list (tab 2):

| Key | Action |
|-----|--------|
| `Space` | stage or unstage the file |
| `v` | show the diff; `Esc` closes it |
| `r` | take the file out of the index, the changes stay |
| `x` | throw the working-tree changes away (asks first) |
| `z` / `Z` | stash everything / take the newest stash back (asks first) |

On the log (tab 3): `/` filters, `c` clears it, `y` writes `CHANGELOG.md`,
`b` moves the window between the last tag and the last 20 or 50 commits, `e`
exports the section as `RELEASE-NOTES-<version>.md`.

## Editors and terminals

`keymap = "ide"` in `config.toml` leads the footer with `Ctrl+Enter` and
`Ctrl+Space`; `default` leads with `Ctrl+S` and `Ctrl+N`. Every key works in both
profiles — the profile only decides what is advertised first, so a hand that
knows one editor is not surprised.

**Visual Studio** (2022): `View > Terminal` or ``Ctrl+` `` opens a real terminal
window; type `commitsmith` there and every key above works, because the window is
a PTY and not an output pane. If you want a button for colleagues, add
`Tools > External Tools` with the command `commitsmith` and the initial directory
`$(SolutionDir)`, then bind `Tools.ExternalCommand1` under
`Tools > Options > Environment > Keyboard`. Committing through Visual Studio's own
Git UI bypasses the tool, so run `commitsmith hooks install` once per repository:
the `commit-msg` hook then checks the message however it was written.

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
