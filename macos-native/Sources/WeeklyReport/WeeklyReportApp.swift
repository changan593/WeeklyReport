import SwiftUI

@main
struct WeeklyReportApp: App {
    @State private var appModel = AppModel(store: JSONStore())

    var body: some Scene {
        WindowGroup {
            ContentView(model: appModel)
                .frame(minWidth: 960, minHeight: 620)
                .task {
                    await appModel.load()
                }
        }
        .windowStyle(.titleBar)
        .commands {
            CommandGroup(replacing: .newItem) {}
        }
    }
}
