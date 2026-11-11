import SwiftUI

/// The choreography strip: big tiles, one Record and one Play button. A child
/// authors by doing, not by editing JSON.
struct DanceSheet: View {
    @ObservedObject var studio: DanceStudio
    let execute: (DanceStep) -> Void
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            HStack {
                Text(Copy.dance)
                    .font(.system(size: 20, weight: .semibold, design: .rounded))
                    .accessibilityIdentifier("dance-title")
                Spacer()
                Button { dismiss() } label: {
                    Image(systemName: "xmark")
                        .font(.system(size: 12, weight: .semibold))
                        .foregroundStyle(.secondary)
                        .frame(width: 48, height: 48)
                        .background(Color.white.opacity(0.10), in: Circle())
                        .contentShape(Circle())
                }
                .buttonStyle(.plain)
                .accessibilityLabel(Copy.close)
                .accessibilityIdentifier("dance-close")
            }

            Text(Copy.danceHint)
                .font(.system(size: 11, design: .rounded))
                .foregroundStyle(.secondary)

            ScrollView {
                LazyVGrid(
                    columns: [GridItem(.adaptive(minimum: 88), spacing: 8)],
                    spacing: 8
                ) {
                    ForEach(Array(studio.steps.enumerated()), id: \.element.id) { index, step in
                        VStack(spacing: 3) {
                            Image(systemName: step.kind == "skill" ? "figure.walk" : "waveform")
                                .font(.system(size: 16))
                            Text(step.label.isEmpty ? step.value : step.label)
                                .font(.system(size: 10, design: .rounded))
                                .lineLimit(1)
                                .minimumScaleFactor(0.7)
                            Text("\(index + 1)")
                                .font(.system(size: 9, design: .monospaced))
                                .foregroundStyle(.secondary)
                        }
                        .foregroundStyle(studio.activeStep == index ? .black : .white.opacity(0.9))
                        .frame(maxWidth: .infinity, minHeight: 64)
                        .background(
                            studio.activeStep == index
                                ? Color.yellow.opacity(0.92)
                                : Color.white.opacity(0.08),
                            in: RoundedRectangle(cornerRadius: 12)
                        )
                    }
                    if studio.steps.isEmpty {
                        Text(Copy.danceEmpty)
                            .font(.system(size: 12, design: .rounded))
                            .foregroundStyle(.secondary)
                            .frame(maxWidth: .infinity, minHeight: 64)
                    }
                }
            }
            .frame(maxHeight: 230)

            HStack(spacing: 10) {
                Button {
                    studio.toggleRecording()
                } label: {
                    Label(
                        studio.recording ? Copy.danceStopRecord : Copy.danceRecord,
                        systemImage: studio.recording ? "record.circle.fill" : "record.circle"
                    )
                    .font(.system(size: 13, weight: .semibold, design: .rounded))
                    .frame(maxWidth: .infinity, minHeight: 50)
                }
                .buttonStyle(.borderedProminent)
                .tint(studio.recording ? .red : .blue)
                .accessibilityIdentifier("dance-record")

                Button {
                    if studio.playing { studio.stop() } else { studio.play(execute) }
                } label: {
                    Label(
                        studio.playing ? Copy.danceStop : Copy.dancePlay,
                        systemImage: studio.playing ? "stop.fill" : "play.fill"
                    )
                    .font(.system(size: 13, weight: .semibold, design: .rounded))
                    .frame(maxWidth: .infinity, minHeight: 50)
                }
                .buttonStyle(.bordered)
                .disabled(studio.steps.isEmpty)
                .accessibilityIdentifier("dance-play")

                Button {
                    studio.clear()
                } label: {
                    Image(systemName: "trash")
                        .font(.system(size: 14))
                        .frame(width: 50, height: 50)
                }
                .buttonStyle(.bordered)
                .disabled(studio.steps.isEmpty)
                .accessibilityLabel(Copy.deleteRule)
                .accessibilityIdentifier("dance-clear")
            }
            if studio.saved && !studio.steps.isEmpty {
                Text(Copy.danceSaved)
                    .font(.system(size: 10, design: .rounded))
                    .foregroundStyle(.secondary)
            }
        }
        .padding(20)
        .preferredColorScheme(.dark)
    }
}
