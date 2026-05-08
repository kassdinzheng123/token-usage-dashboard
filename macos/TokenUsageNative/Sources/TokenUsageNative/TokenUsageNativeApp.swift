import SwiftUI

@main
struct TokenUsageNativeApp: App {
    @StateObject private var store = LiveTokenUsageDashboardStore()
    @StateObject private var menuBarStore = LiveTokenUsageDashboardStore()
    @StateObject private var currencyController = TokenUsageBillingCurrencyController()

    var body: some Scene {
        WindowGroup("Token Usage", id: "dashboard") {
            TokenUsageDashboardView(store: store, currencyController: currencyController)
        }

        MenuBarExtra {
            TokenUsageMenuBarExtraView(store: menuBarStore, currencyController: currencyController)
        } label: {
            TokenUsageMenuBarLabel(store: menuBarStore)
        }
        .menuBarExtraStyle(.window)
    }
}
