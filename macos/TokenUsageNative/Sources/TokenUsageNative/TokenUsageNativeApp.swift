import SwiftUI

@main
struct TokenUsageNativeApp: App {
    @StateObject private var store = LiveTokenUsageDashboardStore()
    @StateObject private var currencyController = TokenUsageBillingCurrencyController()

    var body: some Scene {
        WindowGroup("Token Usage", id: "dashboard") {
            TokenUsageDashboardView(store: store, currencyController: currencyController)
        }

        MenuBarExtra {
            TokenUsageMenuBarExtraView(store: store, currencyController: currencyController)
        } label: {
            TokenUsageMenuBarLabel(store: store)
        }
        .menuBarExtraStyle(.window)
    }
}
