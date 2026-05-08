import Foundation

actor TokenUsageAPIClient {
    enum ClientError: LocalizedError {
        case invalidURL
        case unexpectedStatus(Int)

        var errorDescription: String? {
            switch self {
            case .invalidURL:
                "Invalid API URL."
            case let .unexpectedStatus(statusCode):
                "Unexpected API status code: \(statusCode)."
            }
        }
    }

    let baseURL: URL
    private let session: URLSession
    private let decoder: JSONDecoder

    init(
        baseURL: URL = URL(string: "http://127.0.0.1:3456/api")!,
        session: URLSession = .shared,
        decoder: JSONDecoder = JSONDecoder()
    ) {
        self.baseURL = baseURL
        self.session = session
        self.decoder = decoder
    }

    func fetchHealth() async throws -> HealthStatus {
        try await fetch("health")
    }

    func fetchTodaySummary(refresh: Bool = false) async throws -> TodaySummaryResponse {
        try await fetch("today", queryItems: refreshQueryItems(refresh: refresh))
    }

    func warmDashboardCache() async throws {
        try await fetchIgnoringBody("refresh")
    }

    func fetchDaily(
        source: UsageSource,
        since: String? = nil,
        until: String? = nil,
        refresh: Bool = false
    ) async throws -> [DailyUsageEntry] {
        try await fetchUsage(source: source, view: .daily, since: since, until: until, refresh: refresh)
    }

    func fetchMonthly(
        source: UsageSource,
        since: String? = nil,
        until: String? = nil,
        refresh: Bool = false
    ) async throws -> [MonthlyUsageEntry] {
        try await fetchUsage(source: source, view: .monthly, since: since, until: until, refresh: refresh)
    }

    func fetchSessions(
        source: UsageSource,
        since: String? = nil,
        until: String? = nil,
        refresh: Bool = false
    ) async throws -> [SessionUsageEntry] {
        try await fetchUsage(source: source, view: .sessions, since: since, until: until, refresh: refresh)
    }

    func fetchBlocks(
        source: UsageSource,
        since: String? = nil,
        until: String? = nil,
        refresh: Bool = false
    ) async throws -> [BlockUsageEntry] {
        try await fetchUsage(source: source, view: .blocks, since: since, until: until, refresh: refresh)
    }

    private func fetchUsage<T: Decodable>(
        source: UsageSource,
        view: UsageView,
        since: String?,
        until: String?,
        refresh: Bool
    ) async throws -> T {
        var queryItems: [URLQueryItem] = []
        if let since {
            queryItems.append(URLQueryItem(name: "since", value: since))
        }
        if let until {
            queryItems.append(URLQueryItem(name: "until", value: until))
        }
        queryItems.append(contentsOf: refreshQueryItems(refresh: refresh))

        return try await fetch("\(source.rawValue)/\(view.rawValue)", queryItems: queryItems)
    }

    private func refreshQueryItems(refresh: Bool) -> [URLQueryItem] {
        refresh ? [URLQueryItem(name: "refresh", value: "true")] : []
    }

    private func fetch<T: Decodable>(
        _ path: String,
        queryItems: [URLQueryItem] = []
    ) async throws -> T {
        let url = try makeURL(path: path, queryItems: queryItems)
        let (data, response) = try await session.data(from: url)
        try validate(response: response)

        return try decoder.decode(T.self, from: data)
    }

    private func fetchIgnoringBody(
        _ path: String,
        queryItems: [URLQueryItem] = []
    ) async throws {
        let url = try makeURL(path: path, queryItems: queryItems)
        let (_, response) = try await session.data(from: url)
        try validate(response: response)
    }

    private func validate(response: URLResponse) throws {
        if let httpResponse = response as? HTTPURLResponse,
           !(200...299).contains(httpResponse.statusCode) {
            throw ClientError.unexpectedStatus(httpResponse.statusCode)
        }
    }

    private func makeURL(path: String, queryItems: [URLQueryItem]) throws -> URL {
        let url = baseURL.appendingPathComponent(path)
        guard var components = URLComponents(url: url, resolvingAgainstBaseURL: false) else {
            throw ClientError.invalidURL
        }
        if !queryItems.isEmpty {
            components.queryItems = queryItems
        }
        guard let result = components.url else {
            throw ClientError.invalidURL
        }
        return result
    }
}
