# SwiftUI Migration Notes

## Module Mapping

| Tauri/Rust | SwiftUI Native |
| --- | --- |
| `src/App.jsx` | `Sources/WeeklyReport/ContentView.swift` |
| `src/api.js` | direct calls into `AppModel` services |
| `src-tauri/src/workspace.rs` | `Workspace` in `Models.swift` |
| `src-tauri/src/store.rs` | `JSONStore.swift` |
| `src-tauri/src/state/*` | `AppModel` plus future repository types |
| `src-tauri/src/logs*` | `LogCore.swift`, `ClaudeLogParser.swift`, `CodexLogParser.swift`, `LogCollector.swift` |
| `src-tauri/src/llm*` | `LLMClient.swift` |
| `src-tauri/src/report.rs` | `ReportPromptBuilder.swift`, `ReportGenerator.swift` |
| `src-tauri/src/ssh.rs` | future `SSHSync.swift` using `Process` |
| `src-tauri/src/email.rs` | future mail sender; SMTP library decision still needed |
| `src-tauri/src/scheduler.rs` | future menu-bar/LaunchAgent-backed scheduler |

## Compatibility Contract

The native app currently reads the same data directory as the Tauri app on macOS:

```text
~/Library/Application Support/WeeklyReport/
```

It keeps the same JSON filenames and snake_case field names so existing data can be reused.

## Known Design Decisions

- The Swift version targets macOS 14+ so it can use modern SwiftUI and Observation.
- The rewrite is staged. This avoids mixing UI migration with LLM, SSH, SMTP, and scheduler behavior in one change.
- Local log parsing and prompt construction are now Swift-native and covered by SwiftPM tests.
- Manual report generation is wired through the native service layer and covered by a mock LLM test.
- The bundle identifier is `io.github.changan593.weeklyreport.native` for now, avoiding collision with the existing Tauri bundle id during development.
