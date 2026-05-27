# WeeklyReport

> Cross-platform desktop app that turns your Claude Code / Codex CLI conversation logs into AI-generated weekly reports, delivered on a schedule.

English · [简体中文](./README.zh-CN.md)

<p align="center">
  <img alt="license" src="https://img.shields.io/badge/license-MIT-blue.svg">
  <img alt="platform" src="https://img.shields.io/badge/platform-macOS%20|%20Linux%20|%20Windows-lightgrey">
  <img alt="tauri" src="https://img.shields.io/badge/tauri-2-orange">
  <img alt="rust" src="https://img.shields.io/badge/rust-1.75+-dea584">
  <img alt="react" src="https://img.shields.io/badge/react-18-61dafb">
</p>

## Why

Writing weekly reports is painful, but your daily interactions with AI coding tools (Claude Code, Codex CLI) are already the truest signal of your work — they just sit scattered across machines as long, noisy JSONL files. This tool aggregates them, compresses them, asks an LLM to draft a clean report, and (optionally) emails it to your manager on a cron schedule.

## Download

Pre-built binaries: **[Releases](https://github.com/changan593/WeeklyReport/releases/latest)**

| Platform | File |
| --- | --- |
| Windows 10/11 x64 | `WeeklyReport_X.Y.Z_x64-setup.exe` (recommended) or `*.msi` |
| macOS Apple Silicon (Tauri) | `WeeklyReport_X.Y.Z_aarch64.dmg` |
| macOS Apple Silicon (Swift native) | `WeeklyReport-swift-X.Y.Z-arm64.dmg` (community contribution) |
| macOS Intel | `WeeklyReport_X.Y.Z_x64.dmg` |
| Debian / Ubuntu | `WeeklyReport_X.Y.Z_amd64.deb` |
| Fedora / RHEL | `WeeklyReport-X.Y.Z-1.x86_64.rpm` |
| Generic Linux | `WeeklyReport_X.Y.Z_amd64.AppImage` |

If your OS blocks the app on first launch:
- **Windows SmartScreen** — click "More info" → "Run anyway"
- **macOS Gatekeeper** — right-click the app → "Open", or run `xattr -dr com.apple.quarantine /Applications/WeeklyReport.app` in Terminal

For SSH remote-workspace setup see [docs/SSH.md](./docs/SSH.md).

## Releases

One line per release — full notes in [docs/CHANGELOG.md](./docs/CHANGELOG.md).

| Version | Date | Highlight |
| --- | --- | --- |
| **0.1.2** | 2026-05-27 | Major report-quality overhaul: `doc/docs` subdir scan with mtime-based strong/weak signal classification; prompt rewrite so user `extra_prompt` truly drives the format (no more tech-report template overriding custom templates); filler-word filtering (`好的` / `再试一次`); optional two-round LLM mode (extract → render); animated progress bar + bouncing dots + cycling logs during waiting. |
| **0.1.1** | 2026-05-21 | Three Windows-only fixes (console-window, modal drag-close, SSH key hint) and a complete SSH setup guide. |
| **0.1.0** | 2026-05-20 | First public release — end-to-end pipeline: log collection → compression → LLM generation → local archive → scheduled email. |

## Clients

This repo ships **two independent client implementations**, released together:

| Client | Stack | Platforms | Source | Maintainer |
| --- | --- | --- | --- | --- |
| **Tauri** (primary) | Rust + React + Tailwind | macOS / Linux / Windows | repo root | [@changan593](https://github.com/changan593) |
| **Swift native** (community) | Swift + SwiftUI | macOS 14+ (arm64 / Intel) | [`macos-native/`](./macos-native/) | [@Norvain](https://github.com/Norvain) |

The Swift port is in active iteration; some features (SSH sync, scheduling, SMTP send) are not wired yet. See [macos-native/README.md](./macos-native/README.md).

## Features

- 🪶 **Lightweight** — no database, JSON file storage, < 10 MB release binary
- 🌐 **Multi-LLM** — OpenAI / Anthropic / Gemini / DeepSeek / OpenRouter / Kimi / Qwen / local Ollama / vLLM with 10 built-in presets
- 🖥️ **Multi-workspace** — local + N SSH remote servers, logs aggregated into one report
- 📚 **Project doc awareness** — auto-scans project root + `doc/docs/` subdirs (≤ 2 levels deep) as background; classifies docs into strong/weak signals by modification time
- 🎯 **Format fidelity** — your template's `extra_prompt` (with concrete examples) drives the output verbatim; no built-in rules override custom templates
- 🔁 **Two-round mode** (optional) — first extract achievement JSON, then render; cuts noise from process-y prompts at the cost of 2× tokens
- ⏰ **Scheduled email** — cron expression + SMTP, weekly generation and delivery
- 🔒 **Data sovereignty** — everything stays local; back up, Git-sync, migrate freely
- 🌍 **Cross-platform** — macOS / Linux / Windows via Tauri 2

## Quick Start (build from source)

### Tauri client

Requirements:
- [Node.js](https://nodejs.org/) ≥ 18
- [Rust](https://rustup.rs/) ≥ 1.75
- Linux extras:

  ```bash
  sudo apt install libwebkit2gtk-4.1-dev build-essential curl wget \
                   file libssl-dev libgtk-3-dev libayatana-appindicator3-dev \
                   librsvg2-dev
  ```

Run:
```bash
git clone git@github.com:changan593/WeeklyReport.git
cd WeeklyReport
npm install
npm run tauri:dev          # dev mode with HMR
npm run tauri:build        # produce installers
```
Release artifacts land in `src-tauri/target/release/bundle/` (`.dmg` / `.AppImage` / `.deb` / `.msi`).

### Swift native client

Requirements:
- macOS 14+
- Xcode with the Swift 6.2 toolchain

Run:
```bash
cd macos-native
swift build
Scripts/package_app.sh release   # produce .app bundle
open WeeklyReport.app
```
Details in [macos-native/README.md](./macos-native/README.md).

## Usage

1. **Configure an LLM source** — on the "LLM Sources" page pick a preset (DeepSeek / OpenAI / local Ollama, etc.), paste the API key
2. **Add a workspace** — a local workspace is auto-created; for remote machines add an SSH workspace (setup in [docs/SSH.md](./docs/SSH.md))
3. **Configure SMTP (optional)** — on the "Settings" page; Gmail / QQ / 163 / WeCom presets included
4. **Generate manually** — click "Generate" in the sidebar to verify the flow
5. **Schedule** — on the "Schedules" page create a task with cron + recipients + template, enable it

## Data Storage

Everything is stored locally. Paths by platform:

| Platform | Path |
| --- | --- |
| macOS | `~/Library/Application Support/WeeklyReport/` |
| Linux | `~/.config/weekly-report/` |
| Windows | `%APPDATA%\WeeklyReport\` |

Data-model details in [docs/ARCHITECTURE.md](./docs/ARCHITECTURE.md).

## Documentation

> Internal docs are mostly in Chinese (this project started in a Chinese-speaking context). English users can rely on this README plus per-binary install notes above. PRs translating individual docs are welcome.

| Doc | Content |
| --- | --- |
| [docs/SPEC.md](./docs/SPEC.md) | Full functional spec |
| [docs/ARCHITECTURE.md](./docs/ARCHITECTURE.md) | Technical architecture, modules, data model |
| [docs/SSH.md](./docs/SSH.md) | SSH remote workspace setup (keygen + push + UI fields) |
| [docs/LLM.md](./docs/LLM.md) | Multi-LLM provider abstraction |
| [docs/JSONL.md](./docs/JSONL.md) | Claude Code / Codex CLI log schema |
| [docs/UI.md](./docs/UI.md) | UI / UX spec |
| [docs/TASKS.md](./docs/TASKS.md) | Phased implementation plan |
| [docs/DECISIONS.md](./docs/DECISIONS.md) | Architecture decision records |
| [docs/CONTRIBUTING.md](./docs/CONTRIBUTING.md) | Contributing guide |
| [docs/CHANGELOG.md](./docs/CHANGELOG.md) | Release notes |
| [macos-native/README.md](./macos-native/README.md) | Swift native client build & feature parity |

## Contributing

Issues and PRs are welcome. Code style:

- **Rust** — `cargo fmt` + `cargo clippy -- -D warnings`
- **React** — function components + Hooks, Tailwind utilities, no extra CSS-in-JS
- **Swift** — see [macos-native/README.md](./macos-native/README.md)
- **Commits** — [Conventional Commits](https://www.conventionalcommits.org/)

## License

[MIT](./LICENSE)
