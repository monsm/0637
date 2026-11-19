import SwiftUI

/// The choreography strip: big tiles, one Record and one Play button. A child
/// authors by doing, not by editing JSON.
struct DanceSheet: View {
    private struct QuickStep {
        let kind: String
        let value: String
    }
    private let quickSteps = [
        QuickStep(kind: "skill", value: "roulade"),
        QuickStep(kind: "skill", value: "kick_left"),
        QuickStep(kind: "skill", value: "laugh"),
        QuickStep(kind: "skill", value: "yes"),
        QuickStep(kind: "sound", value: "chirp"),
        QuickStep(kind: "light", value: "yellow"),
        QuickStep(kind: "light", value: "off"),
    ]
    @ObservedObject var studio: DanceStudio
    let execute: (DanceStep) -> Void
    /// Record and perform a quick action without closing the sheet.
    let tryStep: (String, String) -> Void
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        VStack(alignment: .leading, spacing: DuckSpace.md) {
            HStack {
                Text(Copy.dance)
                    .font(.duckTitle.weight(.semibold))
                    .accessibilityIdentifier("dance-title")
                Spacer()
                Button { dismiss() } label: {
                    DuckCloseGlyph()
                }
                .buttonStyle(.plain)
                .accessibilityLabel(Copy.close)
                .accessibilityIdentifier("dance-close")
            }

            Text(Copy.danceHint)
                .font(.duckCaption)
                .foregroundStyle(.secondary)

            ScrollView {
                LazyVGrid(
                    columns: [GridItem(.adaptive(minimum: 88), spacing: DuckSpace.sm)],
                    spacing: DuckSpace.sm
                ) {
                    ForEach(Array(studio.steps.enumerated()), id: \.element.id) { index, step in
                        VStack(spacing: DuckSpace.xxs) {
                            Image(systemName: step.kind == "light" ? "lightbulb.fill" : (step.kind == "skill" ? "figure.walk" : "waveform"))
                                .font(DuckIcon.sm)
                            Text(step.label.isEmpty ? step.value : step.label)
                                .font(.duckCaption)
                                .lineLimit(1)
                                .minimumScaleFactor(0.7)
                            Text("\(index + 1)")
                                .font(.duckMicro)
                                .foregroundStyle(.secondary)
                        }
                        .foregroundStyle(studio.activeStep == index ? .black : .white.opacity(0.9))
                        .frame(maxWidth: .infinity, minHeight: 64)
                        .background(
                            studio.activeStep == index
                                ? Color.yellow.opacity(0.92)
                                : Color.white.opacity(0.08),
                            in: RoundedRectangle(cornerRadius: DuckRadius.thumb)
                        )
                    }
                    if studio.steps.isEmpty {
                        Text(Copy.danceEmpty)
                            .font(.duckCaption)
                            .foregroundStyle(.secondary)
                            .frame(maxWidth: .infinity, minHeight: 64)
                    }
                }
            }
            .frame(maxHeight: 230)

            Text(Copy.danceTry)
                .font(.duckCaption)
                .foregroundStyle(.secondary)
            ScrollView(.horizontal, showsIndicators: false) {
                HStack(spacing: DuckSpace.xs) {
                    ForEach(quickSteps.indices, id: \.self) { index in
                        let step = quickSteps[index]
                        Button {
                            tryStep(step.kind, step.value)
                        } label: {
                            Text(step.kind == "sound" ? Copy.danceStepSound : SkillLabel.of(step.value))
                                .font(.duckCaption.weight(.semibold))
                                .padding(.horizontal, DuckSpace.md)
                                .frame(minHeight: 40)
                                .background(Color.white.opacity(0.10), in: Capsule())
                                .contentShape(Capsule())
                        }
                        .buttonStyle(.plain)
                        .accessibilityIdentifier("dance-try-\(step.value)")
                    }
                }
            }

            HStack(spacing: DuckSpace.sm) {
                Button {
                    studio.toggleRecording()
                } label: {
                    Label(
                        studio.recording ? Copy.danceStopRecord : Copy.danceRecord,
                        systemImage: studio.recording ? "record.circle.fill" : "record.circle"
                    )
                    .font(.duckLabel.weight(.semibold))
                    .frame(maxWidth: .infinity, minHeight: 50)
                }
                .buttonStyle(.borderedProminent)
                .tint(studio.recording ? .red : Color.duckData)
                .accessibilityIdentifier("dance-record")

                Button {
                    if studio.playing { studio.stop() } else { studio.play(execute) }
                } label: {
                    Label(
                        studio.playing ? Copy.danceStop : Copy.dancePlay,
                        systemImage: studio.playing ? "stop.fill" : "play.fill"
                    )
                    .font(.duckLabel.weight(.semibold))
                    .frame(maxWidth: .infinity, minHeight: 50)
                }
                .buttonStyle(.bordered)
                .disabled(studio.steps.isEmpty)
                .accessibilityIdentifier("dance-play")

                Button {
                    studio.clear()
                } label: {
                    Image(systemName: "trash")
                        .font(DuckIcon.sm)
                        .frame(width: 50, height: 50)
                }
                .buttonStyle(.bordered)
                .disabled(studio.steps.isEmpty)
                .accessibilityLabel(Copy.deleteRule)
                .accessibilityIdentifier("dance-clear")
            }
            if studio.saved && !studio.steps.isEmpty {
                Text(Copy.danceSaved)
                    .font(.duckCaption)
                    .foregroundStyle(.secondary)
            }
        }
        .padding(DuckSpace.xl)
    }
}
