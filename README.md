# commit-tui

Terminal UI for writing and linting [Conventional Commits](https://www.conventionalcommits.org/).

You pick a type, type the subject, and the preview updates live. The checks pane
tells you what a reviewer or a hook would complain about: unknown types, subjects
that are too long, past tense instead of imperative, a `!` without a
`BREAKING CHANGE` footer, and so on.

```
┌ commit-tui conventional commits ──────────────────────────────┐
│ subject 19/72   0 errors  0 warnings                          │
└───────────────────────────────────────────────────────────────┘
┌ compose ──────────────────────────────────────────────────────┐
│ type        [ feat ]  ←/→ switch                              │
│ scope         optional, e.g. api                              │
│ subject     add commit composer                               │
│ body      ▌ because hooks need it                             │
│ breaking      what breaks, optional                           │
│ footer        Refs: #12, optional                             │
└───────────────────────────────────────────────────────────────┘
┌ preview ────────────────────┐┌ checks · 0 errors · 0 warnings ┐
│ feat: add commit composer   ││ ✓ ready to commit             │
│                             ││                               │
│ because hooks need it       ││                               │
└─────────────────────────────┘└───────────────────────────────┘
 Tab next  ←/→ type  Ctrl+S commit  Esc quit
```

## Install

```sh
cargo install --path .
```

Rust 1.85 or newer.

## Use

Open the composer:

```sh
commit-tui
```

Ctrl+S prints the finished message to stdout, so the tool composes into git:

```sh
git commit -m "$(commit-tui)"
```

### Lint an existing message

```sh
commit-tui check --message "feat(api): add rate limit"
commit-tui check --file .git/COMMIT_EDITMSG
git log -1 --pretty=%B | commit-tui check
```

Exit code is 1 on errors, 0 otherwise. `--strict` also fails on warnings.

### Use it as a commit-msg hook

```sh
./scripts/install-hook.sh
```

That drops a hook into the current repository; from then on git refuses messages
the linter rejects.

## Keys

| Key | Action |
|-----|--------|
| `Tab` / `↓` | next field |
| `Shift+Tab` / `↑` | previous field |
| `←` `→` | switch type (on the type field), move the cursor elsewhere |
| `Ctrl+S` | render the message and exit |
| `Esc`, `Ctrl+Q` | quit without output |
| `q` | quit, but only on the type field |

## Rules

Errors block a commit:

- type missing or not in `feat fix docs style refactor perf test build ci chore revert`
- empty subject
- subject longer than 72 characters
- header without `: ` separator, or `!` without a `BREAKING CHANGE` footer
- body without a blank line after the subject

Warnings are style advice: subject over 50 characters, trailing period, capital
first letter, past tense (`added` instead of `add`), multi-word scope, body lines
over 100 characters.

## Layout

```
src/
  main.rs      CLI: composer, `check`, `types`
  app.rs       state, field editing, key handling, event loop
  ui.rs        ratatui rendering
  commit.rs    the message model: render, parse, wrap
  validate.rs  lint rules and severities
```

`commit.rs` and `validate.rs` have no terminal dependency, so they can be reused
by other front ends or moved into a library crate later.

## Roadmap

- read `.commit-tui.toml` for custom types and limits
- multiline body editor with a real cursor
- `--from-diff` to guess type and scope from staged files
- optional `commitlint`-style JSON output for CI

## License

MIT, see [LICENSE](LICENSE).
