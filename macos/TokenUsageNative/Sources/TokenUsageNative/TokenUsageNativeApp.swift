import SwiftUI

@main
struct TokenUsageNativeApp: App {
    @StateObject private var preferences: TokenUsagePreferencesController
    @StateObject private var store: LiveTokenUsageDashboardStore
    @StateObject private var currencyController = TokenUsageBillingCurrencyController()
    @NSApplicationDelegateAdaptor(AppDelegate.self) private var appDelegate

    init() {
        let preferences = TokenUsagePreferencesController()
        _preferences = StateObject(wrappedValue: preferences)
        _store = StateObject(wrappedValue: LiveTokenUsageDashboardStore(preferences: preferences))
    }

    var body: some Scene {
        WindowGroup("Token Usage", id: "dashboard") {
            TokenUsageDashboardView(
                store: store,
                currencyController: currencyController,
                preferences: preferences
            )
            .onAppear {
                appDelegate.configureMenuBar(store: store, currencyController: currencyController)
            }
        }
        .defaultSize(width: 1160, height: 780)
        .windowResizability(.contentMinSize)
        .commands {
            CommandGroup(replacing: .appVisibility) {
                Button("Show Token Usage") {
                    NSApp.activate(ignoringOtherApps: true)
                    for window in NSApp.windows where window.title == "Token Usage" {
                        window.makeKeyAndOrderFront(nil)
                        return
                    }
                }
            }
        }

        Settings {
            TokenUsageSettingsView(preferences: preferences)
        }
    }
}

final class AppDelegate: NSObject, NSApplicationDelegate {
    func applicationShouldHandleReopen(_ sender: NSApplication, hasVisibleWindows flag: Bool) -> Bool {
        if !flag {
            for window in sender.windows where window.title == "Token Usage" {
                window.makeKeyAndOrderFront(nil)
                return false
            }
        }
        return true
    }

    @MainActor
    func configureMenuBar(
        store: LiveTokenUsageDashboardStore,
        currencyController: TokenUsageBillingCurrencyController
    ) {
        MenuBarStatusItemController.shared.configure(
            store: store,
            currencyController: currencyController
        )
    }
}
