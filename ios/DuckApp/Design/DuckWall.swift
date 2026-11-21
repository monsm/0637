import SwiftUI

/// Direction A: the wall carries two objects. The wooden long-case clock is
/// the time machine (tap opens it, hold freezes); the brass barometer is the
/// readings (the instrument surface). Same identifiers and focus values the
/// old time rail had, so the UI tests and the focus return are unchanged.
struct DuckWallClock: View {
    var focusActive = false
    let stageFocus: AccessibilityFocusState<DuckStageFocus?>.Binding
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            VStack(spacing: DuckSpace.xxs) {
                ZStack(alignment: .top) {
                    RoundedRectangle(cornerRadius: DuckRadius.tiny)
                        .fill(
                            LinearGradient(
                                colors: [Color.duckWoodHi, Color.duckWood, Color.duckWoodDeep],
                                startPoint: .top, endPoint: .bottom
                            )
                        )
                        .frame(width: 34, height: 76)
                        .overlay(
                            RoundedRectangle(cornerRadius: DuckRadius.tiny)
                                .stroke(Color.black.opacity(0.65), lineWidth: 1)
                        )
                        .overlay(alignment: .top) {
                            Capsule()
                                .fill(Color.duckTextPrimary.opacity(0.18))
                                .frame(width: 22, height: 1)
                                .padding(.top, DuckSpace.xs)
                        }
                    // The face: a vertical tick ladder and the amber hand you
                    // pull back. No round dial — a regulator case.
                    VStack(spacing: DuckSpace.xxs) {
                        ForEach(0..<12, id: \.self) { _ in
                            Capsule()
                                .fill(Color.duckTextPrimary.opacity(0.28))
                                .frame(width: 18, height: 1)
                        }
                    }
                    .frame(width: 20, height: 56)
                    .background(Color.black.opacity(0.45), in: RoundedRectangle(cornerRadius: DuckRadius.tiny))
                    .padding(.top, DuckSpace.sm)
                    Rectangle()
                        .fill(Color.duckAction)
                        .frame(width: 26, height: 3)
                        .overlay(alignment: .trailing) {
                            Circle().fill(Color.duckBrass).frame(width: 5, height: 5)
                        }
                        .offset(y: 36)
                    Circle()
                        .fill(
                            LinearGradient(colors: [Color.duckBrass, Color.duckBrassDeep],
                                           startPoint: .top, endPoint: .bottom)
                        )
                        .frame(width: 8, height: 8)
                        .offset(y: 66)
                }
                .frame(width: 34, height: 76)
                Text(Copy.wallClock)
                    .font(.duckMicro)
                    .foregroundStyle(Color.duckTextPrimary.opacity(0.5))
            }
        }
        .buttonStyle(DuckPressStyle())
        .accessibilityLabel(Copy.time)
        .accessibilityIdentifier("duck-time")
        .modifier(StageFocusModifier(focus: .time, active: focusActive, binding: stageFocus))
    }
}

/// The brass barometer: a reading device — dense ticks, two needles.
struct DuckWallGauge: View {
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            VStack(spacing: DuckSpace.xxs) {
                ZStack {
                    Circle()
                        .fill(
                            RadialGradient(
                                colors: [Color.duckWoodDeep, Color.black.opacity(0.9)],
                                center: .topLeading, startRadius: 2, endRadius: 34
                            )
                        )
                        .frame(width: 46, height: 46)
                        .overlay(Circle().stroke(Color.duckBrass.opacity(0.55), lineWidth: 3))
                        .overlay(alignment: .top) {
                            TickRing()
                        }
                    Needle(angle: 24, length: 15)
                        .stroke(Color.duckBrass, style: StrokeStyle(lineWidth: 1.4, lineCap: .round))
                    Needle(angle: -38, length: 11)
                        .stroke(Color.duckAction.opacity(0.85), style: StrokeStyle(lineWidth: 1, lineCap: .round))
                    Circle().fill(Color.duckBrass).frame(width: 4, height: 4)
                }
                .frame(width: 46, height: 46)
                Text(Copy.readings)
                    .font(.duckMicro)
                    .foregroundStyle(Color.duckTextPrimary.opacity(0.5))
            }
        }
        .buttonStyle(DuckPressStyle())
        .accessibilityLabel(Copy.readings)
        .accessibilityIdentifier("duck-readings")
    }
}

private struct Needle: Shape {
    let angle: Double
    let length: CGFloat

    func path(in rect: CGRect) -> Path {
        var path = Path()
        let center = CGPoint(x: rect.midX, y: rect.midY)
        let radians = angle * .pi / 180
        path.move(to: center)
        path.addLine(to: CGPoint(x: center.x + sin(radians) * length,
                                 y: center.y - cos(radians) * length))
        return path
    }
}

private struct TickRing: View {
    var body: some View {
        ZStack {
        ForEach(0..<12, id: \.self) { index in
            Capsule()
                .fill(Color.duckTextPrimary.opacity(0.32))
                .frame(width: 1, height: index % 3 == 0 ? 4 : 2)
                .offset(y: -17)
                .rotationEffect(.degrees(Double(index) * 30))
        }
        }
    }
}
