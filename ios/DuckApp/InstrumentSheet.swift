import SwiftUI

/// The instrument: the numbers, the sensors and the time machine, pulled up
/// from the live view rather than living on it.
struct InstrumentSheet: View {
    @ObservedObject var controller: DuckController
    /// Flipping the language re-renders the sheet's computed strings.
    @State private var languageTick = 0
    @Environment(\.dismiss) private var dismiss

    /// The sheet's first screen is the time machine and the living duck:
    /// what it is doing, its face, its history. The dense telemetry, the
    /// phone's own sensors and the provenance hash live behind "More" —
    /// they are answers to questions a person asks rarely, and eleven
    /// always-open rows made the sheet a wall of text on a small screen.
    @State private var showDetails = false
    /// The vet book file the export button produced. Non-nil presents the
    /// system share sheet for exactly that file.
    @State private var shareURL: URL?
    /// The user's own model.
    @State private var showAI = false
    /// The kept postcards.
    @State private var showPostcards = false
    /// The collection page.
    @State private var showDex = false

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 18) {
                header
                timeMachine
                face
                questCard
                careCard
                behaviorRow
                trickRow
                lifeRecord
                Button {
                    showDex = true
                } label: {
                    Label(Copy.dex, systemImage: "square.grid.3x3.fill")
                        .font(.system(size: 13, weight: .semibold, design: .rounded))
                        .frame(maxWidth: .infinity, minHeight: 48)
                }
                .buttonStyle(.bordered)
                .accessibilityIdentifier("dex-open")
                .sheet(isPresented: $showDex) {
                    DexSheet(controller: controller)
                        .presentationDetents([.large])
                        .presentationDragIndicator(.visible)
                }
                Button {
                    showPostcards = true
                } label: {
                    Label(
                        "\(Copy.postcardShelf) · \(controller.postcards.cards.count)",
                        systemImage: "photo.on.rectangle.angled"
                    )
                    .font(.system(size: 13, weight: .semibold, design: .rounded))
                    .frame(maxWidth: .infinity, minHeight: 48)
                }
                .buttonStyle(.bordered)
                .accessibilityIdentifier("postcards-open")
                .sheet(isPresented: $showPostcards) {
                    PostcardShelfView(shelf: controller.postcards)
                        .presentationDetents([.large])
                        .presentationDragIndicator(.visible)
                }
                awayRow
                voiceCommands
                Button {
                    showAI = true
                } label: {
                    Label(Copy.ai, systemImage: "brain.head.profile")
                        .font(.system(size: 13, weight: .semibold, design: .rounded))
                        .frame(maxWidth: .infinity, minHeight: 48)
                }
                .buttonStyle(.bordered)
                .accessibilityIdentifier("ai-open")
                // Attached to the button, not the scroll view: the sheet for
                // the export file already lives on the scroll view, and
                // SwiftUI is more reliable with one sheet per view.
                .sheet(isPresented: $showAI) {
                    AISheet(controller: controller)
                        .presentationDetents([.large])
                        .presentationDragIndicator(.visible)
                }
                Button {
                    withAnimation(.snappy(duration: 0.2)) { showDetails.toggle() }
                } label: {
                    Label(
                        showDetails ? Copy.showLess : Copy.showMore,
                        systemImage: showDetails ? "chevron.up" : "chevron.down"
                    )
                    .font(.system(size: 12, design: .rounded))
                }
                .buttonStyle(.bordered)
                if showDetails {
                    telemetry
                    sensors
                    provenance
                }
                footer
            }
            .padding(20)
        }
        .background(Color(red: 0.055, green: 0.075, blue: 0.10))
        .id(languageTick)
        .preferredColorScheme(.dark)
        .sheet(
            isPresented: Binding(
                get: { shareURL != nil },
                set: { if !$0 { shareURL = nil } }
            )
        ) {
            if let shareURL {
                ShareSheet(items: [shareURL])
            }
        }
    }

    /// The lifetime record's three headline numbers, and the file. The numbers
    /// are the core's own counters; the button writes the core's own document.
    private var lifeRecord: some View {
        VStack(alignment: .leading, spacing: 10) {
            sectionTitle(Copy.lifeRecord, detail: nil)
            HStack(spacing: 10) {
                lifeStat("\(Int(controller.vetbookMetres.rounded())) m", Copy.walked)
                lifeStat("\(controller.vetbookRolls)", Copy.rolls)
                lifeStat("\(controller.vetbookPickups)", Copy.pickups)
            }
            Button {
                shareURL = controller.vetbookFile()
            } label: {
                Label(Copy.exportVetbook, systemImage: "square.and.arrow.up")
                    .font(.system(size: 13, weight: .semibold, design: .rounded))
                    .frame(maxWidth: .infinity, minHeight: 48)
            }
            .buttonStyle(.borderedProminent)
            .accessibilityIdentifier("export-vetbook")
        }
    }

    private func lifeStat(_ value: String, _ label: String) -> some View {
        VStack(spacing: 3) {
            Text(value)
                .font(.system(size: 20, weight: .semibold, design: .rounded))
                .foregroundStyle(.white)
            Text(label)
                .font(.system(size: 10, design: .rounded))
                .foregroundStyle(.secondary)
        }
        .frame(maxWidth: .infinity)
        .padding(.vertical, 10)
        .background(Color.white.opacity(0.06), in: RoundedRectangle(cornerRadius: 12))
    }

    /// The companion's three numbers. Fullness, not hunger, fills the bar:
    /// a full bar must mean a fed duck. The pin is the core's published value;
    /// nothing here is computed by the shell except the percentage.
    /// Three quests from the core's real counters. Each row is a single-series
    /// progress meter: the text and x/y numbers carry the meaning, not colour,
    /// and completion has a checkmark as well as the word "Done".
    private var questCard: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack {
                Text(Copy.quests)
                    .font(.system(size: 14, weight: .semibold, design: .rounded))
                    .accessibilityIdentifier("quests-title")
                Spacer()
                if controller.questStreak > 0 {
                    Text(Copy.questStreak(controller.questStreak))
                        .font(.system(size: 11, design: .rounded))
                        .foregroundStyle(.orange)
                }
            }
            if controller.quests.isEmpty {
                Text("…")
                    .font(.system(size: 12, design: .rounded))
                    .foregroundStyle(.secondary)
            } else {
                ForEach(Array(controller.quests.enumerated()), id: \.element.id) { index, quest in
                    VStack(alignment: .leading, spacing: 4) {
                        HStack(spacing: 6) {
                            Text(quest.done ? "✓" : "\(index + 1)")
                                .font(.system(size: 11, weight: .bold, design: .rounded))
                                .foregroundStyle(quest.done ? .green : .secondary)
                                .frame(width: 16)
                            Text(quest.title)
                                .font(.system(size: 12, design: .rounded))
                                .foregroundStyle(.white.opacity(0.85))
                                .lineLimit(1)
                                .minimumScaleFactor(0.8)
                            Spacer()
                            Text(quest.done ? Copy.questDone : "\(Int(quest.progress * 100))%")
                                .font(.system(size: 10, design: .monospaced))
                                .foregroundStyle(.secondary)
                        }
                        ProgressView(value: quest.progress)
                            .tint(quest.done ? .green : .blue)
                            .accessibilityIdentifier("quest-progress-\(index)")
                    }
                }
                if controller.quests.allSatisfy({ $0.done }) {
                    Text(Copy.questAllDone)
                        .font(.system(size: 11, design: .rounded))
                        .foregroundStyle(.green)
                        .accessibilityIdentifier("quest-all-done")
                }
            }
        }
        .padding(12)
        .background(Color.white.opacity(0.05), in: RoundedRectangle(cornerRadius: 14))
    }

    private var careCard: some View {
        VStack(alignment: .leading, spacing: 10) {
            sectionTitle(Copy.care, detail: controller.careAsleep ? Copy.dozing : nil)
            HStack(alignment: .top, spacing: 14) {
                careBar(
                    Copy.fullness,
                    value: 1 - controller.careHunger,
                    tint: .orange,
                    id: "care-hunger"
                )
                careBar(
                    Copy.mood,
                    value: (controller.careMood + 1) / 2,
                    tint: .pink,
                    id: "care-mood"
                )
                careBar(
                    Copy.bond,
                    value: controller.careBond,
                    tint: .cyan,
                    id: "care-bond"
                )
            }
            if controller.careAsleep {
                Text(Copy.careLine(
                    hunger: controller.careHunger,
                    mood: controller.careMood,
                    bond: controller.careBond,
                    asleep: true
                ))
                .font(.system(size: 11, design: .rounded))
                .foregroundStyle(.secondary)
            }
            nightRow
        }
    }

    /// Sleep mode: the chosen night, and the morning that ends it. This is
    /// not a skill the duck performs — it is the same doze a long neglect
    /// reaches, entered on purpose. The quiet number is the core's own
    /// simulated clock, divided into whole minutes for the reader; the
    /// morning is the same chip in reverse.
    @ViewBuilder private var nightRow: some View {
        if controller.nightActive {
            HStack(spacing: 10) {
                Text(Copy.nightQuiet(nightMinutes))
                    .font(.system(size: 12, design: .rounded))
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
                    .minimumScaleFactor(0.8)
                    .accessibilityIdentifier("night-quiet")
                Spacer()
                nightChip(Copy.goodMorning, icon: "sun.max.fill", id: "night-wake") {
                    controller.postNight(on: false)
                }
            }
        } else {
            HStack {
                nightChip(Copy.goodNight, icon: "moon.zzz.fill", id: "night-sleep") {
                    controller.postNight(on: true)
                }
                Spacer()
            }
        }
    }

    /// The night's chip, in the sheet's own treatment: a rounded tile over
    /// the card, a small glyph and a word, plain style so the row reads like
    /// the time machine's buttons rather than a system button.
    private func nightChip(
        _ title: String,
        icon: String,
        id: String,
        action: @escaping () -> Void
    ) -> some View {
        Button(action: action) {
            HStack(spacing: 5) {
                Image(systemName: icon)
                    .font(.system(size: 12, weight: .semibold))
                Text(title)
                    .font(.system(size: 11, weight: .semibold, design: .rounded))
                    .lineLimit(1)
                    .minimumScaleFactor(0.8)
            }
            .padding(.horizontal, 12)
            .frame(minHeight: 44)
            .background(Color.white.opacity(0.07), in: RoundedRectangle(cornerRadius: 10))
            .contentShape(RoundedRectangle(cornerRadius: 10))
        }
        .buttonStyle(.plain)
        .accessibilityLabel(title)
        .accessibilityIdentifier(id)
    }

    /// Whole minutes of quiet, floored: "Quiet for 1 min" only once a full
    /// minute has passed on the night's own clock.
    private var nightMinutes: Int {
        Int(max(controller.nightQuietSeconds, 0) / 60)
    }

    private func careBar(_ label: String, value: Double, tint: Color, id: String) -> some View {
        VStack(alignment: .leading, spacing: 5) {
            Text(label)
                .font(.system(size: 11, weight: .medium, design: .rounded))
                .foregroundStyle(.secondary)
            ProgressView(value: min(max(value, 0), 1))
                .tint(tint)
                .accessibilityIdentifier(id)
            Text("\(Int((min(max(value, 0), 1) * 100).rounded()))%")
                .font(.system(size: 11, design: .monospaced))
                .foregroundStyle(.secondary)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    /// The behaviour row already lived in telemetry; up here it is one of the
    /// two lines that answer "what is it doing right now".
    private var behaviorRow: some View {
        row(Copy.behavior, controller.behaviorSummary)
    }

    private var trickRow: some View {
        row(Copy.trickBook, controller.trickBook)
    }

    /// What the duck got up to while the app was away: rituals it played,
    /// falls it took. One line, only when there is something to say.
    @ViewBuilder private var awayRow: some View {
        if let summary = controller.awaySummary {
            row(Copy.awayTitle, summary)
        }
    }

    private var header: some View {
        HStack(alignment: .firstTextBaseline) {
            Text(Copy.thisDuck)
                .accessibilityIdentifier("sheet-title")
                .font(.system(size: 20, weight: .semibold, design: .rounded))
            Spacer()
            Button(Copy.language.rawValue) {
                Copy.language = Copy.language == .english ? .chinese : .english
                languageTick &+= 1
            }
            .font(.system(size: 11, design: .rounded))
            .buttonStyle(.bordered)
            Text(controller.label)
                .font(.system(size: 12, design: .monospaced))
                .foregroundStyle(.secondary)
                .accessibilityIdentifier("sheet-label")
            // A deterministic dismissal: the UI test used to swipe the sheet
            // down, which in a portrait medium detent scrolls instead.
            Button {
                dismiss()
            } label: {
                Image(systemName: "xmark")
                    .font(.system(size: 12, weight: .semibold))
                    .foregroundStyle(.secondary)
                    .frame(width: 48, height: 48)
                    .background(Color.white.opacity(0.10), in: Circle())
                    .contentShape(Circle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel(Copy.close)
            .accessibilityIdentifier("sheet-close")
        }
    }

    // MARK: - Time machine

    private var timeMachine: some View {
        VStack(alignment: .leading, spacing: 10) {
            sectionTitle(Copy.timeMachine, detail: "tick \(controller.simTick)")

            // The scrubber spans the checkpoints the core actually kept.
            VStack(spacing: 6) {
                GeometryReader { geometry in
                    let width = geometry.size.width
                    // The share of history still retained, matching the
                    // portrait rail. Dividing by oldestTick mislabelled the
                    // bar once ticks 0 aged out.
                    let newest = max(CGFloat(controller.simTick), 1)
                    let fraction = max(0, min(1,
                        (newest - CGFloat(controller.oldestTick)) / newest
                    ))
                    ZStack(alignment: .leading) {
                        Capsule()
                            .fill(Color.white.opacity(0.08))
                            .frame(height: 6)
                        Capsule()
                            .fill(Color.accentColor.opacity(0.6))
                            .frame(width: width * fraction, height: 6)
                    }
                }
                .frame(height: 10)
                HStack {
                    Text("\(controller.oldestTick)")
                    Spacer()
                    Text("\(controller.simTick)")
                }
                .font(.system(size: 9, design: .monospaced))
                .foregroundStyle(.secondary)
            }

            HStack(spacing: 8) {
                timeButton(
                    controller.paused ? "play.fill" : "pause.fill",
                    controller.paused ? Copy.resume : Copy.freeze
                ) {
                    controller.setPaused(!controller.paused)
                }
                timeButton("forward.frame.fill", Copy.step) {
                    controller.step()
                }
                timeButton("gobackward.5", Copy.rewind) {
                    controller.rewind()
                }
                timeButton("arrow.triangle.branch", Copy.fork) {
                    controller.fork()
                }
            }

            Picker("Speed", selection: Binding(
                get: { controller.speed },
                set: { controller.setSpeed($0) }
            )) {
                Text("0.25×").tag(0.25)
                Text("0.5×").tag(0.5)
                Text("1×").tag(1.0)
                Text("2×").tag(2.0)
            }
            .pickerStyle(.segmented)
        }
        .padding(14)
        .background(Color.white.opacity(0.04), in: RoundedRectangle(cornerRadius: 16))
    }

    private func timeButton(_ icon: String, _ title: String, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            VStack(spacing: 4) {
                Image(systemName: icon)
                    .font(.system(size: 15))
                Text(title)
                    .font(.system(size: 9, design: .rounded))
            }
            .frame(maxWidth: .infinity, minHeight: 48)
            .padding(.vertical, 9)
            .background(Color.white.opacity(0.07), in: RoundedRectangle(cornerRadius: 10))
        }
        .buttonStyle(.plain)
        .accessibilityIdentifier("time-\(icon)")
    }

    // MARK: - Telemetry

    private var telemetry: some View {
        VStack(alignment: .leading, spacing: 10) {
            sectionTitle(Copy.state, detail: nil)
            row(Copy.bringup, controller.bringupText)
            row(Copy.gain, "\(controller.gain)")
            row(Copy.fallen, controller.fallen ? Copy.yes : Copy.no)
            row(Copy.trunkZ, String(format: "%.4f m", controller.trunk.z))
            row(Copy.gravityZ, String(format: "%.4f", controller.gravity.z))
            row(Copy.branches, "\(controller.branchCount)")
            HStack {
                Text(Copy.drive)
                    .font(.system(size: 12, design: .rounded))
                    .foregroundStyle(.secondary)
                Spacer()
                Picker(Copy.drive, selection: Binding(
                    get: { controller.driveMode },
                    set: { controller.setMode($0) }
                )) {
                    Text(Copy.walk).tag("walk")
                    Text(Copy.roller).tag("roller")
                }
                .pickerStyle(.segmented)
                .frame(width: 150)
                .accessibilityIdentifier("drive-mode")
            }
            row(Copy.imu, controller.imuReady ? Copy.yes : Copy.no)
            row(Copy.held, controller.pickedUp ? Copy.yes : Copy.no)
            if controller.hasMouth {
                row(Copy.mouth, String(format: "%.2f", controller.mouth))
            }
            if controller.volts > 0 {
                row(Copy.battery, String(format: "%.2f V", controller.volts))
            }
            // What the phone itself achieved, once a second. The 50 Hz target
            // is robotd's; a tick rate below it means the phone cannot keep up.
            if controller.fps > 0 {
                row("fps / Hz", String(format: "%.0f / %.1f", controller.fps, controller.ticksPerSecond))
            }
        }
        .padding(14)
        .background(Color.white.opacity(0.04), in: RoundedRectangle(cornerRadius: 16))
    }

    // MARK: - Face

    /// The virtual face: the verdict a status LED would give, the four-light
    /// battery gauge, the flashlight. A phone has no LEDs, so the sheet is
    /// where the face lives.
    private var face: some View {
        VStack(alignment: .leading, spacing: 10) {
            sectionTitle(Copy.face, detail: nil)
            row(Copy.ledStatus, Copy.faceStatus(controller.faceStatus))
            // The gauge as its four LEDs: filled bars, then hollow ones, the
            // order the upstream gauge fills. Text keeps it monospaced like
            // every other value in the sheet.
            HStack {
                Text(Copy.gauge)
                    .font(.system(size: 12, design: .rounded))
                    .foregroundStyle(.secondary)
                Spacer()
                Text(gaugeBars)
                    .font(.system(size: 12, design: .monospaced))
                    .foregroundStyle(.white)
                    .accessibilityIdentifier("sheet-gauge-bars")
                Text(Copy.faceGauge(controller.faceGauge))
                    .font(.system(size: 12, design: .monospaced))
                    .foregroundStyle(.secondary)
            }
            row(Copy.battery, controller.faceBattery)
            flashlightRow
        }
        .padding(14)
        .background(Color.white.opacity(0.04), in: RoundedRectangle(cornerRadius: 16))
    }

    /// "▮▮▯▯" for two lit LEDs of four. The core's `bars` is 0…4; an
    /// out-of-range poll answer still renders four hollow lights.
    private var gaugeBars: String {
        let lit = max(0, min(4, controller.faceBars))
        return String(repeating: "▮", count: lit) + String(repeating: "▯", count: 4 - lit)
    }

    /// The flashlight row: state in words where the other values sit, and a
    /// toggle button at the row's right edge like the mic's toggle.
    private var flashlightRow: some View {
        HStack {
            Text(Copy.flashlight)
                .font(.system(size: 12, design: .rounded))
                .foregroundStyle(.secondary)
            Spacer()
            if let color = controller.flashlightColor {
                Text("\(color) · \(Copy.flashlightOn)")
                    .font(.system(size: 12, design: .monospaced))
                    .foregroundStyle(.white)
            } else {
                Text(Copy.flashlightOff)
                    .font(.system(size: 12, design: .monospaced))
                    .foregroundStyle(.secondary)
            }
            Button {
                controller.toggleFlashlight()
            } label: {
                Image(systemName: "flashlight.on.fill")
                    .font(.system(size: 14))
                    .frame(width: 48, height: 48)
                    .background(Color.white.opacity(0.10), in: Circle())
                    .contentShape(Circle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel(Copy.flashlight)
            .accessibilityIdentifier("sheet-flashlight-toggle")
        }
    }

    // MARK: - Sensors

    private var sensors: some View {
        VStack(alignment: .leading, spacing: 10) {
            sectionTitle(Copy.whatPhoneSenses, detail: controller.sensorMode)
            Text(Copy.sensorNote)
                .font(.system(size: 10, design: .rounded))
                .foregroundStyle(.secondary)
            // A refused camera has no reading, so "—" must not hide the denial.
            // Depth has no app-owned feed yet and stays "—" on devices without
            // LiDAR, which is honest; on LiDAR hardware a camera denial is a
            // depth denial too (the controller already formats that).
            row(Copy.camera, controller.cameraDenied ? Copy.denied : controller.cameraSummary)
            micRow
            row(Copy.depth, controller.depthSummary)
            row("IMU", controller.imuSummary)
        }
        .padding(14)
        .background(Color.white.opacity(0.04), in: RoundedRectangle(cornerRadius: 16))
    }

    /// The microphone is the one sensor the user switches on. Off shows "—";
    /// enabling asks permission (never at launch); refusal says "denied";
    /// granted shows the live 1 Hz values read back from GET /sensors.
    private var micRow: some View {
        HStack {
            Text(Copy.microphone)
                .font(.system(size: 12, design: .rounded))
                .foregroundStyle(.secondary)
            Spacer()
            Text(controller.micSummary)
                .font(.system(size: 12, design: .monospaced))
                .accessibilityIdentifier("sheet-mic-summary")
            Toggle("", isOn: Binding(
                get: { controller.micEnabled },
                set: { controller.setMicEnabled($0) }
            ))
            .labelsHidden()
            .accessibilityLabel(Copy.microphone)
            .accessibilityIdentifier("sheet-mic-toggle")
        }
    }

    // MARK: - Voice commands

    /// The duck's ear: Mandarin in, verbs out.
    ///
    /// Off by default, and asking for the microphone and the speech
    /// permission happens only when the user flips this toggle — never at
    /// launch. The words are the duck's own input (翻滚 becomes the roulade
    /// skill in the core), so this has its own section rather than living
    /// with the room's sensors.
    private var voiceCommands: some View {
        VStack(alignment: .leading, spacing: 10) {
            sectionTitle(Copy.voiceCommands, detail: nil)
            Text(Copy.voiceNote)
                .font(.system(size: 10, design: .rounded))
                .foregroundStyle(.secondary)
            HStack {
                Text(Copy.voiceEar)
                    .font(.system(size: 12, design: .rounded))
                    .foregroundStyle(.secondary)
                Spacer()
                Text(speechStatus)
                    .font(.system(size: 12, design: .monospaced))
                    .accessibilityIdentifier("sheet-voice-status")
                Toggle("", isOn: Binding(
                    get: { controller.voiceCommandsEnabled },
                    set: { controller.setVoiceCommands($0) }
                ))
                .labelsHidden()
                .accessibilityLabel(Copy.voiceEar)
                .accessibilityIdentifier("sheet-voice-toggle")
            }
            // A long sentence must not push the label off the row, so the
            // transcript is one line and truncates.
            HStack {
                Text(Copy.heard)
                    .font(.system(size: 12, design: .rounded))
                    .foregroundStyle(.secondary)
                Spacer()
                Text(controller.heardText.isEmpty ? "—" : controller.heardText)
                    .font(.system(size: 12, design: .monospaced))
                    .lineLimit(1)
                    .truncationMode(.tail)
            }
        }
        .padding(14)
        .background(Color.white.opacity(0.04), in: RoundedRectangle(cornerRadius: 16))
    }

    /// What the ear is doing, in words. A refused permission and a device
    /// with no recogniser are real states, not an absent reading.
    private var speechStatus: String {
        switch controller.speechState {
        case .off: return Copy.speechOff
        case .listening: return Copy.speechListening
        case .denied: return Copy.denied
        case .unavailable: return Copy.speechUnavailable
        case .onDeviceMissing: return Copy.speechOnDeviceMissing
        }
    }

    private var provenance: some View {
        VStack(alignment: .leading, spacing: 8) {
            sectionTitle(Copy.provenance, detail: "SHA-256")
            Text(controller.fingerprint)
                .font(.system(size: 9, design: .monospaced))
                .foregroundStyle(.secondary)
                .lineLimit(3)
                .textSelection(.enabled)
        }
        .padding(14)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Color.white.opacity(0.04), in: RoundedRectangle(cornerRadius: 16))
    }

    private var footer: some View {
        HStack {
            Button(Copy.reload) { controller.reloadPolicies() }
                .buttonStyle(.bordered)
            Spacer()
            Text(Copy.api)
                .font(.system(size: 10, design: .monospaced))
                .foregroundStyle(.secondary)
        }
    }

    // MARK: - Pieces

    private func sectionTitle(_ title: String, detail: String?) -> some View {
        HStack(alignment: .firstTextBaseline) {
            Text(title)
                .font(.system(size: 13, weight: .semibold, design: .rounded))
            Spacer()
            if let detail {
                Text(detail)
                    .font(.system(size: 10, design: .monospaced))
                    .foregroundStyle(.secondary)
            }
        }
    }

    private func row(_ key: String, _ value: String) -> some View {
        HStack {
            Text(key)
                .font(.system(size: 12, design: .rounded))
                .foregroundStyle(.secondary)
            Spacer()
            Text(value)
                .font(.system(size: 12, design: .monospaced))
        }
    }
}
