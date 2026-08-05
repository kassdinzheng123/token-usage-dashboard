import AppKit
import SwiftUI

/// Shared design tokens for the native macOS look.
/// Cards follow the System Settings / Screen Time grouped-inset language:
/// `controlBackgroundColor` surfaces on the window background, 10pt
/// continuous corners, and a hairline separator stroke.
enum AppDesign {
    static let cardCornerRadius: CGFloat = 10
    static let cardBackground = Color(nsColor: .controlBackgroundColor)
    static let groupedBackground = Color(nsColor: .windowBackgroundColor)
    static let hairline = Color(nsColor: .separatorColor)
}

private struct AppGroupedSurfaceModifier: ViewModifier {
    let cornerRadius: CGFloat

    func body(content: Content) -> some View {
        let shape = RoundedRectangle(cornerRadius: cornerRadius, style: .continuous)

        content
            .background(AppDesign.groupedBackground, in: shape)
            .overlay(
                shape
                    .stroke(AppDesign.hairline.opacity(0.35), lineWidth: 1)
                    .allowsHitTesting(false)
            )
    }
}

private struct FloatingOverlaySurfaceModifier: ViewModifier {
    let cornerRadius: CGFloat
    @Environment(\.accessibilityReduceTransparency) private var reduceTransparency

    @ViewBuilder
    func body(content: Content) -> some View {
        let shape = RoundedRectangle(cornerRadius: cornerRadius, style: .continuous)

        if #available(macOS 26.0, *) {
            content.glassEffect(.regular, in: shape)
        } else if reduceTransparency {
            content
                .background(AppDesign.cardBackground, in: shape)
                .overlay(shape.stroke(AppDesign.hairline.opacity(0.55), lineWidth: 1))
                .shadow(color: Color.black.opacity(0.10), radius: 8, y: 4)
        } else {
            content
                .background(.regularMaterial, in: shape)
                .overlay(shape.stroke(AppDesign.hairline.opacity(0.35), lineWidth: 1))
                .shadow(color: Color.black.opacity(0.12), radius: 8, y: 4)
        }
    }
}

private struct InteractiveGlassControlModifier<ControlShape: InsettableShape>: ViewModifier {
    let shape: ControlShape
    @Environment(\.accessibilityReduceTransparency) private var reduceTransparency

    @ViewBuilder
    func body(content: Content) -> some View {
        if #available(macOS 26.0, *) {
            content.glassEffect(.regular.interactive(), in: shape)
        } else if reduceTransparency {
            content
                .background(AppDesign.cardBackground, in: shape)
                .overlay(shape.stroke(AppDesign.hairline.opacity(0.55), lineWidth: 1))
        } else {
            content
                .background(.ultraThinMaterial, in: shape)
                .overlay(shape.stroke(Color.primary.opacity(0.10), lineWidth: 1))
                .shadow(color: Color.black.opacity(0.06), radius: 3, y: 1)
        }
    }
}

private struct AppCardModifier: ViewModifier {
    func body(content: Content) -> some View {
        content
            .background(
                AppDesign.cardBackground,
                in: RoundedRectangle(cornerRadius: AppDesign.cardCornerRadius, style: .continuous)
            )
            .overlay(
                RoundedRectangle(cornerRadius: AppDesign.cardCornerRadius, style: .continuous)
                    .stroke(AppDesign.hairline.opacity(0.45), lineWidth: 1)
                    .allowsHitTesting(false)
            )
    }
}

extension View {
    /// Native grouped-inset card surface used across dashboard, brief, and menu bar.
    func appCard() -> some View {
        modifier(AppCardModifier())
    }

    /// Floating functional layer for chart hover panels.
    func appFloatingOverlaySurface(cornerRadius: CGFloat = 10) -> some View {
        modifier(FloatingOverlaySurfaceModifier(cornerRadius: cornerRadius))
    }

    /// Custom interactive control group that floats above content.
    func appInteractiveGlassControl<S: InsettableShape>(in shape: S = Capsule()) -> some View {
        modifier(InteractiveGlassControlModifier(shape: shape))
    }

    /// Opaque grouped surface for dense content. It follows the content's
    /// intrinsic height and keeps backdrop colors from bleeding into cards.
    func appGroupedSurface(cornerRadius: CGFloat) -> some View {
        modifier(AppGroupedSurfaceModifier(cornerRadius: cornerRadius))
    }

}

// MARK: - AppPalette
//
// One canonical, deliberately restrained color system. Hues stay close to
// Apple's system palette: slightly desaturated, even in perceived weight, and
// legible on both light and dark grouped-inset surfaces.
enum AppPalette {
    // MARK: CLI brand colors

    static let cliClaude = Color(red: 0.84, green: 0.46, blue: 0.33)      // Anthropic clay
    static let cliCodex = Color(red: 0.10, green: 0.62, blue: 0.47)       // OpenAI green
    static let cliOpenCode = Color(red: 0.10, green: 0.56, blue: 0.72)    // teal
    static let cliHermes = Color(red: 0.50, green: 0.38, blue: 0.86)      // violet
    static let cliOpenClaw = Color(red: 0.90, green: 0.62, blue: 0.14)    // amber
    static let cliPi = Color(red: 0.88, green: 0.34, blue: 0.50)          // pink
    static let cliGrok = Color(red: 0.40, green: 0.42, blue: 0.48)        // graphite
    static let cliCursor = Color(red: 0.34, green: 0.50, blue: 0.94)      // blue
    static let cliCherry = Color(red: 0.86, green: 0.26, blue: 0.36)      // red
    static let cliClaudeScience = Color(red: 0.93, green: 0.66, blue: 0.40) // light clay
    static let cliZCode = Color(red: 0.26, green: 0.58, blue: 0.94)       // sky
    static let cliKimi = Color(red: 0.30, green: 0.42, blue: 0.98)        // indigo
    static let cliReasonIX = Color(red: 0.39, green: 0.31, blue: 0.86)    // violet

    /// CLI colors keyed by `TokenUsageSource.label` (chart domains are label strings).
    static let cliColorsByLabel: [String: Color] = [
        "Claude Code": cliClaude,
        "Codex": cliCodex,
        "OpenCode": cliOpenCode,
        "Hermes": cliHermes,
        "OpenClaw": cliOpenClaw,
        "Pi Agent": cliPi,
        "Grok CLI": cliGrok,
        "Cursor": cliCursor,
        "Cherry Studio": cliCherry,
        "Claude Science": cliClaudeScience,
        "ZCode": cliZCode,
        "Kimi": cliKimi,
        "ReasonIX": cliReasonIX,
    ]

    static func cliColor(forLabel label: String) -> Color {
        cliColorsByLabel[label] ?? Color(red: 0.48, green: 0.52, blue: 0.60)
    }

    // MARK: Model provider families

    static let providerClaude = cliClaude
    static let providerOpenAI = cliCodex
    static let providerGemini = Color(red: 0.34, green: 0.50, blue: 0.94)
    static let providerDeepSeek = Color(red: 0.38, green: 0.40, blue: 0.90)
    static let providerKimi = Color(red: 0.34, green: 0.37, blue: 0.45)
    static let providerMiniMax = Color(red: 0.88, green: 0.30, blue: 0.34)
    static let providerMiMo = Color(red: 0.90, green: 0.62, blue: 0.14)
    static let providerGLM = Color(red: 0.14, green: 0.52, blue: 0.86)
    static let providerCursor = Color(red: 0.44, green: 0.48, blue: 0.58)
    static let providerGrok = Color(red: 0.30, green: 0.32, blue: 0.38)
    static let providerStepFun = Color(red: 0.10, green: 0.48, blue: 0.58)
    static let providerQwen = Color(red: 0.56, green: 0.38, blue: 0.82)
    static let providerLongCat = Color(red: 0.16, green: 0.60, blue: 0.44)
    static let providerMistral = Color(red: 0.92, green: 0.56, blue: 0.20)
    static let providerLlama = Color(red: 0.38, green: 0.66, blue: 0.94)
    static let providerFallback = Color(red: 0.48, green: 0.52, blue: 0.60)

    /// Deterministic per-model color: the provider's brand hue with a stable
    /// shade variation, so sibling models read as one family while remaining
    /// distinguishable as separate donut slices / legend entries.
    static func modelColor(for modelName: String) -> Color {
        let metadata = ProviderMetadata.forModel(modelName)
        let base = metadata.color
        var hash: UInt64 = 14695981039346656037
        for byte in modelName.lowercased().utf8 {
            hash = (hash ^ UInt64(byte)) &* 1099511628211
        }
        switch hash % 3 {
        case 1:
            return base.appBlended(with: .white, fraction: 0.28)
        case 2:
            return base.appBlended(with: .black, fraction: 0.18)
        default:
            return base
        }
    }

    // MARK: Semantic chart colors

    static let chartCost = Color.accentColor
    static let chartCache = Color(red: 0.16, green: 0.60, blue: 0.44)
    static let chartWeekday = Color.accentColor

    static let compositionInput = Color(red: 0.34, green: 0.50, blue: 0.94)
    static let compositionCacheRead = Color(red: 0.62, green: 0.72, blue: 0.96)
    static let compositionOutput = Color(red: 0.84, green: 0.46, blue: 0.38)

    static let semanticError = Color(nsColor: .systemRed)
    static let semanticWarning = Color(nsColor: .systemOrange)
}

private extension Color {
    /// Linear RGB blend, compatible with the macOS 14 deployment target
    /// (`Color.mix(with:by:)` requires macOS 15).
    func appBlended(with other: Color, fraction: Double) -> Color {
        let lhs = NSColor(self).usingColorSpace(.deviceRGB) ?? NSColor(self)
        let rhs = NSColor(other).usingColorSpace(.deviceRGB) ?? NSColor(other)
        let fraction = min(max(fraction, 0), 1)
        return Color(
            red: lhs.redComponent + (rhs.redComponent - lhs.redComponent) * fraction,
            green: lhs.greenComponent + (rhs.greenComponent - lhs.greenComponent) * fraction,
            blue: lhs.blueComponent + (rhs.blueComponent - lhs.blueComponent) * fraction
        )
    }
}

// MARK: - ProviderMetadata

struct ProviderMetadata {
    let label: String
    let abbreviation: String
    let color: Color
    let imageAssetName: String?
    let preservesOriginalImageColor: Bool

    static func forModel(_ modelName: String) -> ProviderMetadata {
        let model = modelName.lowercased()

        if model.contains("deepseek") {
            return ProviderMetadata(label: "DeepSeek", abbreviation: "DS", color: AppPalette.providerDeepSeek, imageAssetName: "deepseek-mark", preservesOriginalImageColor: true)
        }
        if model.contains("longcat") {
            return ProviderMetadata(label: "LongCat", abbreviation: "LC", color: AppPalette.providerLongCat, imageAssetName: "longcat-mark", preservesOriginalImageColor: true)
        }
        if model.contains("kimi") || model.contains("moonshot") || model.hasPrefix("k3") {
            return ProviderMetadata(label: "Kimi", abbreviation: "KM", color: AppPalette.providerKimi, imageAssetName: "kimi-mark")
        }
        if model.contains("minimax") {
            return ProviderMetadata(label: "MiniMax", abbreviation: "MM", color: AppPalette.providerMiniMax, imageAssetName: "minimax-mark", preservesOriginalImageColor: true)
        }
        if model.contains("mimo") || model.contains("xiaomi") {
            return ProviderMetadata(label: "MiMo", abbreviation: "MO", color: AppPalette.providerMiMo, imageAssetName: "xiaomi-mi-mark", preservesOriginalImageColor: true)
        }
        if model.contains("claude") || model.contains("opus") || model.contains("sonnet") || model.contains("haiku") {
            return ProviderMetadata(label: "Claude", abbreviation: "CL", color: AppPalette.providerClaude, imageAssetName: "anthropic-mark", preservesOriginalImageColor: true)
        }
        if model.contains("gpt") || model.contains("openai") || model.contains("chatgpt") || model.hasPrefix("o1") || model.hasPrefix("o3") || model.hasPrefix("o4") {
            return ProviderMetadata(label: "OpenAI", abbreviation: "AI", color: AppPalette.providerOpenAI, imageAssetName: "openai-mark", preservesOriginalImageColor: true)
        }
        if model.contains("glm") || model.contains("zai") || model.contains("z.ai") {
            return ProviderMetadata(label: "GLM", abbreviation: "GL", color: AppPalette.providerGLM, imageAssetName: "zai-mark", preservesOriginalImageColor: true)
        }
        if model.contains("gemini") || model.contains("google") {
            return ProviderMetadata(label: "Gemini", abbreviation: "GM", color: AppPalette.providerGemini, imageAssetName: "gemini-mark", preservesOriginalImageColor: true)
        }
        if model.contains("composer-2.5") || model.contains("composer-2-5") {
            return ProviderMetadata(label: "Cursor", abbreviation: "CR", color: AppPalette.providerCursor, imageAssetName: "cursor-mark", preservesOriginalImageColor: true)
        }
        if model.contains("grok") || model.contains("xai") || model.contains("x.ai") || model.contains("x-ai") {
            return ProviderMetadata(label: "Grok", abbreviation: "GK", color: AppPalette.providerGrok, imageAssetName: "grok-mark")
        }
        if model.contains("stepfun") || model.contains("step-3") {
            return ProviderMetadata(label: "StepFun", abbreviation: "ST", color: AppPalette.providerStepFun, imageAssetName: "stepfun-mark", preservesOriginalImageColor: true)
        }
        if model.contains("qwen") || model.contains("qwq") {
            return ProviderMetadata(label: "Qwen", abbreviation: "QW", color: AppPalette.providerQwen, imageAssetName: "qwen-mark", preservesOriginalImageColor: true)
        }
        if model.contains("mistral") || model.contains("mixtral") || model.contains("codestral") || model.contains("ministral") {
            return ProviderMetadata(label: "Mistral", abbreviation: "MI", color: AppPalette.providerMistral)
        }
        if model.contains("llama") || model.contains("meta") {
            return ProviderMetadata(label: "Llama", abbreviation: "LL", color: AppPalette.providerLlama)
        }

        return ProviderMetadata(
            label: "Model",
            abbreviation: String(modelName.prefix(2)).uppercased(),
            color: AppPalette.providerFallback
        )
    }

    init(label: String, abbreviation: String, color: Color, imageAssetName: String? = nil, preservesOriginalImageColor: Bool = false) {
        self.label = label
        self.abbreviation = abbreviation
        self.color = color
        self.imageAssetName = imageAssetName
        self.preservesOriginalImageColor = preservesOriginalImageColor
    }
}
