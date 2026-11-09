<div align="center">

# ⚡ Terminal AI (`tai`)

**Translate natural language requests directly into Bash/Linux commands with high performance, interactive explanations, and built-in disaster safety.**

[![Rust](https://img.shields.io/badge/Language-Rust-orange.svg?logo=rust)](https://www.rust-lang.org/)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![PRs Welcome](https://img.shields.io/badge/PRs-welcome-brightgreen.svg)](https://github.com/SrClauss/terminal_ai/pulls)
[![GitHub stars](https://img.shields.io/github/stars/SrClauss/terminal_ai?style=social)](https://github.com/SrClauss/terminal_ai)

[English](README.md) • [Português](README.pt-BR.md)

[Quick Install](#-quick-install) • [Usage](#-how-to-use) • [Safety](#-safety-and-disaster-prevention) • [Contributing](#-how-to-contribute-open-roadmap)

</div>

---

## 🎯 Why `tai`?

How often do you find yourself looking up complex `tar` flags, filtering memory-heavy processes with `ps`, or crafting convoluted `find` commands?

**Terminal AI (`tai`)** is built natively in **Rust** as a blazingly fast copilot right inside your terminal:
1. **Ultra-fast**: Cold start in **< 2ms** (zero runtime interpreter overhead).
2. **Explains Before Running**: Never run commands blindly. `tai` breaks down every generated flag, pipe, and parameter.
3. **Native Safety Guard**: Blocks and emits red alert warnings against catastrophic commands (`rm -rf /`, `mkfs`, `> /dev/sd*`).
4. **Native Clipboard Integration**: Seamless clipboard support across Wayland (`wl-copy`) and X11 (`xclip`).
5. **Multi-provider**: Connect to **DeepSeek**, **OpenAI**, or **local Ollama** (`yi-coder:1.5b`, `qwen2.5-coder`) to work 100% offline.
6. **Continuous Session Memory**: Keeps context from your recent turns (15-minute TTL). Say *"now compress them"* or *"add the -v flag"* and it knows what you're referring to.
7. **Chained Multi-step Pipelines**: Generates complex sequences (`&&`, `|`, `;`) visually numbered and previewed. Use `-n` or `tai clear` to reset context memory.
8. **Real-time Streaming (SSE)**: Responses and commands stream incrementally as tokens arrive from the LLM, reducing perceived latency to near zero.

---

## 📦 Quick Install

Clone and run the 1-command automated installer:

```bash
git clone https://github.com/SrClauss/terminal_ai.git
cd terminal_ai
./install.sh
```

The script compiles the optimized release binary, sets up global symlinks/binaries for `tai` and `termai` in `~/.local/bin/`, and configures your `$PATH`.

---

## 🚀 How to Use

### 1. Interactive Assisted Mode (Default)
```bash
tai "list all open tcp ports and their listening processes"
```
`tai` displays the generated command, explains what each parameter does, and presents an interactive action menu:
```text
────────────────────────────────────────────────────────────
Suggested Command: lsof -iTCP -sTCP:LISTEN -n -P
────────────────────────────────────────────────────────────

Detailed explanation:
  - lsof: lists open files and network connections.
  - -iTCP: restricts filtering to TCP connections.
  - -sTCP:LISTEN: selects ports that are actively listening.
  - -n -P: disables slow DNS and port name lookups.

? What do you want to do?
> Execute now
  Copy to clipboard
  Cancel
```

### 2. Fast / Direct Mode (`-y` / `--yes`)
Executes immediately in the shell (with catastrophic safety checks active):
```bash
tai -y "clean apt package cache"
```

### 3. Dry-Run / Pipe Mode (`-p` / `--print`)
Outputs only the raw command to `stdout` with no menus or decorative ANSI output—ideal for scripts and `eval`:
```bash
eval "$(tai -p 'show disk usage only for real filesystems')"
```

### 4. Copy to Clipboard Mode (`-c` / `--copy`)
```bash
tai -c "tar compress src folder excluding .git and target"
```

### 5. Disable Streaming on Demand (`--no-stream`)
By default, `tai` streams tokens in real time via SSE. To wait for the full response before rendering:
```bash
tai --no-stream "extract tar.gz archive"
```

### 6. Autonomous Loop Task Mode (`tai task`)
For complex multi-step objectives requiring consecutive execution, feedback inspection, and correction (e.g., build, diagnose, and test):
```bash
tai task "install project dependencies and run the test suite"
```
* **Difficulty Checkpoint**: Upon reaching the step limit (default 5 steps or `--max-steps <N>`), the agent summarizes errors encountered, assesses remaining difficulty, and interactively prompts whether to grant **5 more steps**.
* **Fully Autonomous Mode (`-y`)**: Runs steps sequentially without intermediate user confirmation:
```bash
tai task "prepare build environment and compile binary" -y
```

---

## ⚙️ Configuration

`tai` can be configured either via **CLI commands** or by editing the **configuration files**:

### 1. Via CLI Commands (`tai config`)

```bash
# Switch to local Ollama (offline):
tai config set provider ollama
# (tai automatically updates api_url to http://localhost:11434/v1 and sets the default model)

# Use a specific model installed in Ollama:
tai config set model "yi-coder:1.5b"

# Switch back to cloud DeepSeek:
tai config set provider deepseek
tai config set api_key "your-api-key-here"

# Toggle response streaming (default: true):
tai config set stream false

# View active configuration:
tai config show
```

### 2. Configuration File (`~/.config/tai/config.toml`)

Permanent configuration is stored at:
`~/.config/tai/config.toml` (secured with `0600` permissions)

```toml
provider = "ollama"
model = "yi-coder:1.5b"
api_url = "http://localhost:11434/v1"
api_key = "sk-..."
language = "en"
default_mode = "interactive"
stream = true
```

### 3. Custom System Rules & Preferences (`~/.config/tai/rules.md`)

You can teach persistent personal preferences by creating or editing `~/.config/tai/rules.md`:

```markdown
# Personal Preferences for tai
- Whenever suggesting disk commands (du, df), always append the -h flag for human-readable output.
- Prefer neovim (nvim) over nano when editors are needed.
- Always sort file listings by size or most recent modification date.
```
`tai` automatically reads this file and injects your custom rules into every future prompt.

---

## 🛡️ Safety and Disaster Prevention

`tai` includes an internal static command analyzer. If an LLM accidentally suggests a potentially catastrophic command:
- `rm -rf /` or `rm -rf /*`
- `mkfs.*` on system partitions
- Direct writes to `/dev/sd*` or `/dev/nvme*`
- Fork bombs

The command is **immediately flagged with a critical red warning** and direct execution (`-y`) is aborted to safeguard your data.

---

## 🤝 How to Contribute (We need your help!)

This is an **Open Source** project and contributions of all kinds are welcome! If you find `tai` useful, consider leaving a ⭐ on GitHub and collaborating with us.

### Areas where you can help right now:

- [ ] **Additional AI Providers**: Native integrations for Anthropic Claude, Google Gemini, and Groq.
- [ ] **Distribution Packaging**:
  - Arch Linux AUR package (`PKGBUILD`).
  - Homebrew tap for Linux/macOS.
  - Debian/Ubuntu package (`.deb`).
- [ ] **Local Model Benchmarking (Ollama)**:
  - > ⚠️ **Practical Note on Small Models**: Tests with ultra-lightweight SLMs (~0.8B to 1.5B, such as `yi-coder:1.5b` and `dgpl-linux-assistant`) showed poor output quality in real-world scenarios (high CPU latency, delimiter hallucinations, difficulty following strict formatting).
  - We encourage contributors to test and benchmark more capable models (7B to 14B, such as `qwen2.5-coder:7b`, `deepseek-coder:6.7b`, or `codellama`) for offline terminal use.
- [ ] **Internationalization (i18n)**:
  - Add language prompts and explanations for Spanish (`es`), French (`fr`), German (`de`), etc.
- [ ] **Shell Completions**:
  - Auto-completions for `fish`, `zsh`, and `bash`.

### Submitting a Contribution:
1. Fork the repository (`https://github.com/SrClauss/terminal_ai`).
2. Create your feature branch: `git checkout -b feature/my-enhancement`.
3. Develop and test with `cargo test`.
4. Open a Pull Request! We're glad to review and assist.

---

## 🔄 Self-Update

Keep your installation up to date with the latest release:
```bash
tai update
```

---

## 📝 License
Distributed under the MIT License. See `LICENSE` for details.
