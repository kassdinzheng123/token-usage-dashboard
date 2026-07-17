import Foundation

public enum TokenUsageBillingCurrency: String, CaseIterable, Identifiable {
    case usd
    case cny

    public var id: String { rawValue }

    var label: String {
        switch self {
        case .usd: "USD"
        case .cny: "CNY"
        }
    }

    fileprivate var currencyCode: String {
        switch self {
        case .usd: "USD"
        case .cny: "CNY"
        }
    }

    fileprivate var localeIdentifier: String {
        switch self {
        case .usd: "en_US"
        case .cny: "zh_CN"
        }
    }
}

@MainActor
public final class TokenUsageBillingCurrencyController: ObservableObject {
    @Published public var selectedCurrency: TokenUsageBillingCurrency {
        didSet {
            defaults.set(selectedCurrency.rawValue, forKey: Self.selectedCurrencyKey)
        }
    }

    @Published public private(set) var usdToCNYRate: Decimal
    @Published public private(set) var exchangeRateDate: String?
    @Published public private(set) var isRefreshingRate = false
    @Published public private(set) var rateErrorMessage: String?

    private let defaults: UserDefaults
    private let exchangeRateURL = URL(string: "https://api.frankfurter.dev/v2/rate/USD/CNY")!
    private var lastRefreshAttempt: Date?

    public init(defaults: UserDefaults = .standard) {
        self.defaults = defaults

        let rawCurrency = defaults.string(forKey: Self.selectedCurrencyKey) ?? TokenUsageBillingCurrency.usd.rawValue
        self.selectedCurrency = TokenUsageBillingCurrency(rawValue: rawCurrency) ?? .usd

        let storedRate = defaults.double(forKey: Self.usdToCNYRateKey)
        self.usdToCNYRate = storedRate > 0 ? Decimal(storedRate) : Decimal(string: "6.8335")!
        self.exchangeRateDate = defaults.string(forKey: Self.exchangeRateDateKey)
    }

    public func string(fromUSD amount: Decimal) -> String {
        let displayAmount = amountInSelectedCurrency(fromUSD: amount)
        let value = NSDecimalNumber(decimal: displayAmount)
        return formatterForSelectedCurrency().string(from: value) ?? fallbackText
    }

    public func refreshExchangeRateIfNeeded(force: Bool = false) async {
        guard force || shouldRefreshExchangeRate else {
            return
        }

        lastRefreshAttempt = Date()
        isRefreshingRate = true
        rateErrorMessage = nil

        do {
            let (data, _) = try await URLSession.shared.data(from: exchangeRateURL)
            let response = try JSONDecoder().decode(FrankfurterRateResponse.self, from: data)
            guard response.rate > 0 else {
                throw URLError(.badServerResponse)
            }

            usdToCNYRate = Decimal(response.rate)
            exchangeRateDate = response.date
            defaults.set(response.rate, forKey: Self.usdToCNYRateKey)
            defaults.set(response.date, forKey: Self.exchangeRateDateKey)
        } catch {
            rateErrorMessage = error.localizedDescription
        }

        isRefreshingRate = false
    }

    private var shouldRefreshExchangeRate: Bool {
        if selectedCurrency == .usd {
            return false
        }

        guard let lastRefreshAttempt else {
            return true
        }

        return Date().timeIntervalSince(lastRefreshAttempt) > 60 * 60
    }

    private func amountInSelectedCurrency(fromUSD amount: Decimal) -> Decimal {
        switch selectedCurrency {
        case .usd:
            amount
        case .cny:
            amount * usdToCNYRate
        }
    }

    /// NumberFormatters are expensive to create; cache one per currency since
    /// `string(fromUSD:)` is called for every row, legend entry, and tooltip.
    private var cachedFormatters: [TokenUsageBillingCurrency: NumberFormatter] = [:]

    private func formatterForSelectedCurrency() -> NumberFormatter {
        if let cached = cachedFormatters[selectedCurrency] {
            return cached
        }
        let formatter = makeFormatter(for: selectedCurrency)
        cachedFormatters[selectedCurrency] = formatter
        return formatter
    }

    private func makeFormatter(for currency: TokenUsageBillingCurrency) -> NumberFormatter {
        let formatter = NumberFormatter()
        formatter.numberStyle = .currency
        formatter.currencyCode = currency.currencyCode
        formatter.locale = Locale(identifier: currency.localeIdentifier)
        formatter.minimumFractionDigits = 2
        formatter.maximumFractionDigits = 2
        return formatter
    }

    private var fallbackText: String {
        switch selectedCurrency {
        case .usd: "$0.00"
        case .cny: "¥0.00"
        }
    }

    private struct FrankfurterRateResponse: Decodable {
        let date: String
        let rate: Double
    }

    private static let selectedCurrencyKey = "TokenUsage.selectedBillingCurrency"
    private static let usdToCNYRateKey = "TokenUsage.usdToCNYRate"
    private static let exchangeRateDateKey = "TokenUsage.exchangeRateDate"
}
