// swift-tools-version: 6.2
import PackageDescription

let package = Package(
    name: "WeeklyReportMac",
    platforms: [
        .macOS(.v14),
    ],
    products: [
        .executable(name: "WeeklyReport", targets: ["WeeklyReport"]),
    ],
    targets: [
        .executableTarget(
            name: "WeeklyReport",
            path: "Sources/WeeklyReport"
        ),
        .testTarget(
            name: "WeeklyReportTests",
            dependencies: ["WeeklyReport"],
            path: "Tests/WeeklyReportTests"
        )
    ]
)
