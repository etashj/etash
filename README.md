# etash

**ExTremely Ahh SHell** — a shell written from the ground up in Rust with a custom lexer, parser, and executor.

## Building

```sh
cargo build --release
./target/release/etash
```

## Features

### Interactive editing
- **History** — up/down arrows, Ctrl+R reverse search, persisted to `~/.etash_history`
- **Tab completion** — first word completes commands from `$PATH` and builtins; subsequent words complete filenames
- **Line editing** — Ctrl+A/E (start/end), Ctrl+W (delete word), Ctrl+D (exit)

### Commands
```sh
ls | grep foo | wc -l        # pipelines
cmd1; cmd2                   # sequential
cmd1 && cmd2                 # and
cmd1 || cmd2                 # or
sleep 10 &                   # background
(cd /tmp && ls)              # subshell
ls *.rs                      # glob expansion
```

### Redirects
| Syntax | Effect |
|--------|--------|
| `cmd > file` | redirect stdout |
| `cmd >> file` | append stdout |
| `cmd < file` | redirect stdin |
| `cmd 2> file` | redirect stderr |
| `cmd 2>&1` | stderr to stdout |
| `cmd > file 2>&1` | both to file |

### Variables and expansion
```sh
NAME=etash
export PATH="/usr/local/bin:$PATH"
echo "Hello, $NAME"        # variable expansion
echo "pid: $$"             # shell pid
echo "status: $?"          # last exit code
echo "${NAME}_rc"          # braced expansion
cd ~/projects              # tilde expansion
unset NAME
```

### Builtins
| Command | Description |
|---------|-------------|
| `cd [dir]` | change directory (no arg → `$HOME`) |
| `pwd` | print working directory |
| `export KEY=val` | set and export variable |
| `unset KEY` | remove variable |
| `fg [%N]` | bring job N to foreground |
| `bg [%N]` | resume job N in background |
| `exit [code]` | exit with optional status |

### Job control
- `Ctrl+Z` stops the foreground process and parks it in the job table
- `fg` / `bg` resume stopped jobs
- Completed background jobs are reported at the next prompt: `[1] done  pid=X exit=0`

### Startup file
etash reads `~/.etashrc` on startup:

```sh
export EDITOR=vim
HISTSIZE=1000
# comments and blank lines are ignored
```

## Architecture

```
src/
  lexer/      tokenizer — words, quotes, escapes, redirects, operators
  parser/     shunting-yard parser → AST
  shell/      state — variables, exports, cwd, word expansion
  executor/   runs AST nodes, pipelines, redirects, job control, builtins
  main.rs     REPL (rustyline)
tests/
  lexer.rs    }
  parser.rs   }  69 tests
  shell.rs    }
```

## Not yet implemented

- Pipelines with non-command stages (`(a; b) | cmd`)
- Command substitution (`$(cmd)`)
- Arithmetic expansion (`$((expr))`)
- Here-documents (`<<EOF`)
- Shell functions
- `source` / `.`
- `~username` expansion
- `jobs` builtin
