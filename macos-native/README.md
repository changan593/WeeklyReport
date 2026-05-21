# WeeklyReport macOS Native

This directory contains the SwiftUI native macOS rewrite of WeeklyReport. It is intentionally isolated from the current Tauri app so both implementations can coexist while the native app is built, reviewed, and merged incrementally.

## Platform Requirements

- macOS 14.0 or later.
- Xcode with Swift 6.2 toolchain, or a compatible command line Swift toolchain.
- Apple Silicon and Intel Macs are supported.
- The packaging script builds for the current host architecture by default. Use the universal build option when you need both `arm64` and `x86_64` slices.

## Local Build

This is a Swift Package Manager project. There is no checked-in `WeeklyReport.xcodeproj`.

From the repository root:

```bash
cd macos-native
swift build
swift test
```

Build a signed ad-hoc `.app` bundle:

```bash
cd macos-native
Scripts/package_app.sh release
open WeeklyReport.app
```

Build, test, package, and relaunch in one step:

```bash
cd macos-native
Scripts/compile_and_run.sh --test
```

Build a universal app bundle:

```bash
cd macos-native
Scripts/compile_and_run.sh --release-universal
```

Open in Xcode:

```bash
cd macos-native
open Package.swift
```

Build with `xcodebuild`:

```bash
cd macos-native
xcodebuild -scheme WeeklyReportMac -destination 'platform=macOS' build
```

The app bundle is created at:

```text
macos-native/WeeklyReport.app
```

Runtime data is stored in:

```text
~/Library/Application Support/WeeklyReport/
```

## Difference From The Tauri Version

The native app reuses the same JSON file names and Codable field names where possible, but it is not yet feature-complete with the Tauri app.

Supported in the SwiftUI native app:

- Native SwiftUI `NavigationSplitView` shell for workspaces, LLM sources, templates, reports, schedules, and settings.
- Local Claude Code and Codex log discovery, JSONL parsing, aggregation, and prompt construction.
- Manual weekly report generation from local logs.
- LLM providers for OpenAI-compatible APIs, Anthropic, Gemini, Claude Code CLI, and Codex CLI.
- Editable workspace and LLM provider configuration.
- Editable generation settings and SMTP configuration persistence.
- Report history list and Markdown report preview.
- SwiftPM tests for parsers, prompt building, LLM request construction, report generation, and configuration persistence.
- Ad-hoc `.app` packaging scripts.

Not yet supported or intentionally different:

- SSH workspace sync is modeled in configuration, but remote log collection is not implemented yet.
- Cron/background scheduled generation is not wired in the native app yet.
- SMTP send, test connection, and test email actions are not implemented yet; only config persistence is available.
- Schedule editing is not implemented yet.
- Template editing is not implemented yet; built-in and stored templates are displayed.
- Report deletion/export actions are not implemented yet.
- The native app does not use the Tauri/Rust backend, Vite frontend, or web UI components.

## Migration Direction

1. Keep the native implementation isolated under `macos-native/`.
2. Continue porting behavior in reviewable slices rather than replacing the Tauri app in one step.
3. Preserve JSON compatibility for shared configuration and report metadata.
4. Add missing native features in follow-up PRs: SSH sync, scheduling, SMTP sending, template editing, and report management.
