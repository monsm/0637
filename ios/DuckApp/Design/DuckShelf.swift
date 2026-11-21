import SwiftUI

/// Direction A (design-demos/A-complete.html): the Live shell's verbs are
/// objects on the duck's bedside shelf, not floating buttons. One object per
/// verb, same identifiers as the old deck so every test and the focus return
/// keep working.
struct DuckShelfObject<Visual: View>: View {
    let id: String
    let label: String
    var focus: DuckStageFocus?
    var focusActive = false
    let stageFocus: AccessibilityFocusState<DuckStageFocus?>.Binding
    var wheee: AnyGesture<Bool>? = nil
    let action: () -> Void
    @ViewBuilder let visual: () -> Visual

    var body: some View {
        Button(action: action) {
            VStack(spacing: DuckSpace.xs) {
                visual()
                    .frame(width: 54, height: 54, alignment: .bottom)
                Text(label)
                    .font(.duckCaption)
                    .foregroundStyle(Color.duckTextPrimary.opacity(0.85))
                    .lineLimit(2)
                    .multilineTextAlignment(.center)
                    .frame(width: 62)
            }
        }
        .buttonStyle(DuckPressStyle())
        .accessibilityLabel(label)
        .accessibilityIdentifier(id)
        .modifier(StageFocusModifier(focus: focus, active: focusActive, binding: stageFocus))
        .modifier(OptionalHoldGesture(gesture: wheee))
    }
}

/// The quack object keeps the press-and-hold wheee ride; every other object
/// has no hold gesture, so the shelf stays one behaviour per verb.
private struct OptionalHoldGesture: ViewModifier {
    let gesture: AnyGesture<Bool>?

    func body(content: Content) -> some View {
        if let gesture {
            content.simultaneousGesture(gesture)
        } else {
            content
        }
    }
}

/// The ceramic bowl: the food level IS the duck's hunger (stranger = emptier).
struct DuckBowl: View {
    var hunger: Double

    var body: some View {
        ZStack(alignment: .top) {
            RoundedRectangle(cornerRadius: DuckRadius.control)
                .fill(
                    LinearGradient(
                        colors: [Color.duckCeramic, Color.duckCeramic.opacity(0.72)],
                        startPoint: .top, endPoint: .bottom
                    )
                )
                .frame(width: 54, height: 30)
                .overlay(alignment: .top) {
                    Capsule()
                        .fill(Color.duckFood)
                        .frame(width: 46, height: max(3, 3 + 9 * min(max(hunger, 0), 1)))
                        .padding(.top, DuckSpace.xxs)
                }
                .overlay(
                    RoundedRectangle(cornerRadius: DuckRadius.control)
                        .stroke(Color.black.opacity(0.35), lineWidth: 1)
                )
        }
        .frame(height: 34, alignment: .bottom)
        .shadow(color: .black.opacity(0.45), radius: 4, y: 3)
    }
}

/// The card box: the skill deck, one amber pip on top.
struct DuckCardBox: View {
    var body: some View {
        ZStack {
            card.rotationEffect(.degrees(-7)).offset(x: -4)
            card.rotationEffect(.degrees(4)).offset(x: 4)
            card
            Diamond().fill(Color.duckAction).frame(width: 8, height: 8)
        }
        .frame(width: 44, height: 54, alignment: .bottom)
        .shadow(color: .black.opacity(0.45), radius: 4, y: 3)
    }

    private var card: some View {
        RoundedRectangle(cornerRadius: DuckRadius.tiny)
            .fill(
                LinearGradient(colors: [Color.duckWoodHi, Color.duckWoodDeep],
                               startPoint: .top, endPoint: .bottom)
            )
            .overlay(
                RoundedRectangle(cornerRadius: DuckRadius.tiny)
                    .stroke(Color.duckTextPrimary.opacity(0.22), lineWidth: 1)
            )
            .frame(width: 34, height: 48)
    }
}

struct Diamond: Shape {
    func path(in rect: CGRect) -> Path {
        var path = Path()
        path.move(to: CGPoint(x: rect.midX, y: rect.minY))
        path.addLine(to: CGPoint(x: rect.maxX, y: rect.midY))
        path.addLine(to: CGPoint(x: rect.midX, y: rect.maxY))
        path.addLine(to: CGPoint(x: rect.minX, y: rect.midY))
        path.closeSubpath()
        return path
    }
}

/// The brass bell: quack.
struct DuckBell: View {
    var body: some View {
        ZStack(alignment: .bottom) {
            RoundedRectangle(cornerRadius: DuckRadius.tiny)
                .fill(
                    LinearGradient(
                        colors: [Color.duckBrass.opacity(0.95), Color.duckBrassDeep],
                        startPoint: .topLeading, endPoint: .bottomTrailing
                    )
                )
                .frame(width: 40, height: 34)
            Circle()
                .fill(Color.duckBrassDeep)
                .frame(width: 10, height: 10)
                .offset(y: 4)
        }
        .frame(height: 40, alignment: .bottom)
        .shadow(color: .black.opacity(0.5), radius: 4, y: 3)
    }
}

/// The open hand: pick up / put down.
struct DuckHand: View {
    var held: Bool

    var body: some View {
        ZStack(alignment: .bottom) {
            HStack(spacing: DuckSpace.xxs) {
                finger(20); finger(24); finger(22); finger(16)
            }
            .offset(y: -18)
            RoundedRectangle(cornerRadius: DuckRadius.thumb)
                .fill(Color.duckCeramic)
                .frame(width: 36, height: 24)
        }
        .frame(width: 44, height: 50, alignment: .bottom)
        .rotationEffect(.degrees(held ? -8 : 0))
        .animation(UIAccessibility.isReduceMotionEnabled ? nil : DuckMotion.quick, value: held)
        .shadow(color: .black.opacity(0.45), radius: 4, y: 3)
    }

    private func finger(_ height: CGFloat) -> some View {
        Capsule().fill(Color.duckCeramic).frame(width: 6, height: height)
    }
}

/// The wooden shelf with the four objects; the board itself is the cabinet's
/// pull (tap the wood, not a fifth verb). Direction A keeps the Live screen
/// at seven controls: four objects, the board (More), the wall clock and the
/// brass gauge.
struct DuckShelf: View {
    var hunger: Double
    var carrying: Bool
    var sheetClosed: Bool
    var lastOpener: DuckStageFocus
    let stageFocus: AccessibilityFocusState<DuckStageFocus?>.Binding
    let onFeed: () -> Void
    let onSkills: () -> Void
    let onQuack: () -> Void
    let onGrab: () -> Void
    let onMore: () -> Void
    var wheee: AnyGesture<Bool>? = nil

    private func active(_ focus: DuckStageFocus) -> Bool {
        sheetClosed && focus == lastOpener
    }

    var body: some View {
        ZStack {
            Button(action: onMore) {
                VStack(spacing: 0) {
                    Spacer(minLength: 0)
                    HStack(spacing: DuckSpace.sm) {
                        ForEach(0..<3, id: \.self) { _ in
                            Circle()
                                .fill(Color.duckTextPrimary.opacity(0.4))
                                .frame(width: 3, height: 3)
                        }
                    }
                    .padding(.bottom, DuckSpace.sm)
                }
                .frame(maxWidth: .infinity, minHeight: 118)
                .background(
                    LinearGradient(
                        colors: [Color.duckWoodHi, Color.duckWood, Color.duckWoodDeep],
                        startPoint: .top, endPoint: .bottom
                    ),
                    in: RoundedRectangle(cornerRadius: DuckRadius.panel)
                )
                .overlay(
                    RoundedRectangle(cornerRadius: DuckRadius.panel)
                        .stroke(Color.black.opacity(0.45), lineWidth: 1)
                )
                .overlay(alignment: .top) {
                    Capsule()
                        .fill(Color.duckTextPrimary.opacity(0.12))
                        .frame(height: 1)
                        .padding(.horizontal, DuckSpace.lg)
                        .padding(.top, DuckSpace.sm)
                }
            }
            .buttonStyle(.plain)
            .accessibilityLabel(Copy.more)
            .accessibilityIdentifier("duck-more")
            .modifier(StageFocusModifier(focus: .more, active: active(.more), binding: stageFocus))

            HStack(spacing: 0) {
                Spacer(minLength: 0)
                DuckShelfObject(id: "duck-feed", label: Copy.feed, focus: .feed,
                                focusActive: active(.feed), stageFocus: stageFocus,
                                action: onFeed) { DuckBowl(hunger: hunger) }
                Spacer(minLength: 0)
                DuckShelfObject(id: "duck-skills", label: Copy.skills, focus: .skills,
                                focusActive: active(.skills), stageFocus: stageFocus,
                                action: onSkills) { DuckCardBox() }
                Spacer(minLength: 0)
                DuckShelfObject(id: "duck-quack", label: Copy.quack, focus: .quack,
                                focusActive: active(.quack), stageFocus: stageFocus,
                                wheee: wheee, action: onQuack) { DuckBell() }
                Spacer(minLength: 0)
                DuckShelfObject(id: "duck-grab",
                                label: carrying ? Copy.putDown : Copy.grab,
                                focus: .grab, focusActive: active(.grab),
                                stageFocus: stageFocus, action: onGrab) {
                    DuckHand(held: carrying)
                }
                Spacer(minLength: 0)
            }
            .padding(.horizontal, DuckSpace.sm)
            .allowsHitTesting(true)
        }
    }
}
