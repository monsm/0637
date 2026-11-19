import SwiftUI

/// The instrument: the numbers, the sensors and the time machine, pulled up
/// from the live view rather than living on it.
struct InstrumentSheet: View {
    @ObservedObject var controller: DuckController
    /// The shell's route opener (B7): the instrument's second set of entries
    /// no longer owns four booleans and four nested sheets.
    let onOpen: (DuckRoute) -> Void
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
    /// The locally drawn vet book card (boss ruling 2026-11-17): its sheet
    /// is attached to its own button, not the scroll view — the scroll view
    /// already owns the export file's share sheet, and SwiftUI is more
    /// reliable with one sheet per view.
    @State private var vetbookCardOpen = false

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: DuckSpace.lg) {
                header
                timeMachine
                face
                questCard
                careCard
                behaviorRow
                trickRow
                lifeRecord
                Button {
                    onOpen(.dex)
                } label: {
                    Label(Copy.collectionTitle, systemImage: "square.grid.3x3.fill")
                        .font(.duckLabel.weight(.semibold))
                        .frame(maxWidth: .infinity, minHeight: 48)
                }
                .buttonStyle(.bordered)
                .accessibilityIdentifier("collection-open")
                Button {
                    onOpen(.postcards)
                } label: {
                    Label(
                        "\(Copy.postcardShelf) · \(controller.postcards.cards.count)",
                        systemImage: "photo.on.rectangle.angled"
                    )
                    .font(.duckLabel.weight(.semibold))
                    .frame(maxWidth: .infinity, minHeight: 48)
                }
                .buttonStyle(.bordered)
                .accessibilityIdentifier("postcards-open")
                Button {
                    onOpen(.diary)
                } label: {
                    Label(Copy.diary, systemImage: "book.closed.fill")
                        .font(.duckLabel.weight(.semibold))
                        .frame(maxWidth: .infinity, minHeight: 48)
                }
                .buttonStyle(.bordered)
                .accessibilityIdentifier("diary-open")
                awayRow
                voiceCommands
                Button {
                    onOpen(.ai)
                } label: {
                    Label(Copy.ai, systemImage: "brain.head.profile")
                        .font(.duckLabel.weight(.semibold))
                        .frame(maxWidth: .infinity, minHeight: 48)
                }
                .buttonStyle(.bordered)
                .accessibilityIdentifier("ai-open")
                // Attached to the button, not the scroll view: the sheet for
                // the export file already lives on the scroll view, and
                // SwiftUI is more reliable with one sheet per view.
                Button {
                    withAnimation(.snappy(duration: 0.2)) { showDetails.toggle() }
                } label: {
                    Label(
                        showDetails ? Copy.showLess : Copy.showMore,
                        systemImage: showDetails ? "chevron.up" : "chevron.down"
                    )
                    .font(.duckCaption)
                }
                .buttonStyle(.bordered)
                if showDetails {
                    telemetry
                    sensors
                    provenance
                }
                footer
            }
            .padding(DuckSpace.xl)
        }
        .background(Color.duckStageInstrument)
        .id(languageTick)
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
        VStack(alignment: .leading, spacing: DuckSpace.sm) {
            sectionTitle(Copy.lifeRecord, detail: nil)
            HStack(spacing: DuckSpace.sm) {
                lifeStat("\(Int(controller.vetbookMetres.rounded())) m", Copy.walked)
                lifeStat("\(controller.vetbookRolls)", Copy.rolls)
                lifeStat("\(controller.vetbookPickups)", Copy.pickups)
            }
            // The per-day rows the core already publishes, newest last — the
            // same last seven the Android shell shows. The data was loading
            // since batch six; this is the view that was missing (FE-3).
            if !controller.vetbookDays.isEmpty {
                Text(controller.vetbookDays.suffix(7).joined(separator: " | "))
                    .font(.duckCaption)
                    .foregroundStyle(.secondary)
                    .lineLimit(3)
                    .accessibilityIdentifier("day-rows")
            }
            Button {
                shareURL = controller.vetbookFile()
            } label: {
                Label(Copy.exportVetbook, systemImage: "square.and.arrow.up")
                    .font(.duckLabel.weight(.semibold))
                    .frame(maxWidth: .infinity, minHeight: 48)
            }
            .buttonStyle(.borderedProminent)
            .accessibilityIdentifier("export-vetbook")
            // The card: the same numbers, drawn into one image a person can
            // share or keep — generated on this phone, sent nowhere.
            Button {
                vetbookCardOpen = true
            } label: {
                Label(Copy.vetbookCardShare, systemImage: "square.and.arrow.up.on.square")
                    .font(.duckLabel.weight(.semibold))
                    .frame(maxWidth: .infinity, minHeight: 48)
            }
            .buttonStyle(.bordered)
            .accessibilityIdentifier("vetbook-card-open")
            .sheet(isPresented: $vetbookCardOpen) {
                VetbookCardSheet()
            }
        }
    }

    private func lifeStat(_ value: String, _ label: String) -> some View {
        VStack(spacing: DuckSpace.xxs) {
            Text(value)
                .font(.duckTitle.weight(.semibold))
                .foregroundStyle(.white)
            Text(label)
                .font(.duckCaption)
                .foregroundStyle(.secondary)
        }
        .frame(maxWidth: .infinity)
        .padding(.vertical, DuckSpace.sm)
        .background(Color.white.opacity(0.06), in: RoundedRectangle(cornerRadius: DuckRadius.thumb))
    }

    /// The companion's three numbers. Fullness, not hunger, fills the bar:
    /// a full bar must mean a fed duck. The pin is the core's published value;
    /// nothing here is computed by the shell except the percentage.
    /// Three quests from the core's real counters. Each row is a single-series
    /// progress meter: the text and x/y numbers carry the meaning, not colour,
    /// and completion has a checkmark as well as the word "Done".
    private var questCard: some View {
        VStack(alignment: .leading, spacing: DuckSpace.sm) {
            HStack {
                Text(Copy.quests)
                    .font(.duckLabel.weight(.semibold))
                    .accessibilityIdentifier("quests-title")
                Spacer()
                if controller.questStreak > 0 {
                    Text(Copy.questStreak(controller.questStreak))
                        .font(.duckCaption)
                        .foregroundStyle(.orange)
                }
            }
            if controller.quests.isEmpty {
                Text("…")
                    .font(.duckCaption)
                    .foregroundStyle(.secondary)
            } else {
                ForEach(Array(controller.quests.enumerated()), id: \.element.id) { index, quest in
                    VStack(alignment: .leading, spacing: DuckSpace.xs) {
                        HStack(spacing: DuckSpace.xs) {
                            Text(quest.done ? "✓" : "\(index + 1)")
                                .font(.duckCaption.weight(.bold))
                                .foregroundStyle(quest.done ? .green : .secondary)
                                .frame(width: 16)
                            Text(quest.title)
                                .font(.duckCaption)
                                .foregroundStyle(.white.opacity(0.85))
                                .lineLimit(1)
                                .minimumScaleFactor(0.8)
                            Spacer()
                            Text(quest.done ? Copy.questDone : "\(Int(quest.progress * 100))%")
                                .font(.duckMicro)
                                .foregroundStyle(.secondary)
                        }
                        ProgressView(value: quest.progress)
                            .tint(quest.done ? .green : .blue)
                            .accessibilityIdentifier("quest-progress-\(index)")
                    }
                }
                if controller.quests.allSatisfy({ $0.done }) {
                    Text(Copy.questAllDone)
                        .font(.duckCaption)
                        .foregroundStyle(.green)
                        .accessibilityIdentifier("quest-all-done")
                }
            }
        }
        .padding(DuckSpace.md)
        .background(Color.white.opacity(0.05), in: RoundedRectangle(cornerRadius: DuckRadius.control))
    }

    private var careCard: some View {
        VStack(alignment: .leading, spacing: DuckSpace.sm) {
            sectionTitle(Copy.care, detail: controller.careAsleep ? Copy.dozing : nil)
            HStack(alignment: .top, spacing: DuckSpace.md) {
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
                .font(.duckCaption)
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
            HStack(spacing: DuckSpace.sm) {
                Text(Copy.nightQuiet(nightMinutes))
                    .font(.duckCaption)
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
            HStack(spacing: DuckSpace.xs) {
                Image(systemName: icon)
                    .font(DuckIcon.xs)
                Text(title)
                    .font(.duckCaption.weight(.semibold))
                    .lineLimit(1)
                    .minimumScaleFactor(0.8)
            }
            .padding(.horizontal, DuckSpace.md)
            .frame(minHeight: 44)
            .background(Color.white.opacity(0.07), in: RoundedRectangle(cornerRadius: DuckRadius.thumb))
            .contentShape(RoundedRectangle(cornerRadius: DuckRadius.thumb))
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
        VStack(alignment: .leading, spacing: DuckSpace.xs) {
            Text(label)
                .font(.duckCaption.weight(.medium))
                .foregroundStyle(.secondary)
            ProgressView(value: min(max(value, 0), 1))
                .tint(tint)
                .accessibilityIdentifier(id)
            Text("\(Int((min(max(value, 0), 1) * 100).rounded()))%")
                .font(.duckData)
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
                .font(.duckTitle.weight(.semibold))
            Spacer()
            Button(Copy.language.rawValue) {
                Copy.language = Copy.language == .english ? .chinese : .english
                languageTick &+= 1
            }
            .font(.duckCaption)
            .buttonStyle(.bordered)
            Text(controller.label)
                .font(.duckData)
                .foregroundStyle(.secondary)
                .accessibilityIdentifier("sheet-label")
            // A deterministic dismissal: the UI test used to swipe the sheet
            // down, which in a portrait medium detent scrolls instead.
            Button {
                dismiss()
            } label: {
                DuckCloseGlyph()
            }
            .buttonStyle(.plain)
            .accessibilityLabel(Copy.close)
            .accessibilityIdentifier("sheet-close")
        }
    }

    // MARK: - Time machine

    private var timeMachine: some View {
        VStack(alignment: .leading, spacing: DuckSpace.sm) {
            sectionTitle(Copy.timeMachine, detail: "tick \(controller.simTick)")

            // The scrubber spans the checkpoints the core actually kept.
            VStack(spacing: DuckSpace.xs) {
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
                .font(.duckMicro)
                .foregroundStyle(.secondary)
            }

            HStack(spacing: DuckSpace.sm) {
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
                // Rewind-play left the drawer for the time bar (the IA
                // ruling cites UI_B4_PLAN §5:116: time-type controls live
                // on the time rail). Same verb Android has always had here.
                timeButton(
                    controller.rewinding ? "stop.circle" : "arrow.uturn.backward.circle",
                    Copy.rewindPlay
                ) {
                    controller.toggleRewindPlay()
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
        .padding(DuckSpace.md)
        .background(Color.white.opacity(0.04), in: RoundedRectangle(cornerRadius: DuckRadius.control))
    }

    private func timeButton(_ icon: String, _ title: String, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            VStack(spacing: DuckSpace.xs) {
                Image(systemName: icon)
                    .font(DuckIcon.sm)
                Text(title)
                    .font(.duckCaption)
            }
            .frame(maxWidth: .infinity, minHeight: 48)
            .padding(.vertical, DuckSpace.sm)
            .background(Color.white.opacity(0.07), in: RoundedRectangle(cornerRadius: DuckRadius.thumb))
        }
        .buttonStyle(.plain)
        .accessibilityIdentifier("time-\(icon)")
    }

    // MARK: - Telemetry

    private var telemetry: some View {
        VStack(alignment: .leading, spacing: DuckSpace.sm) {
            sectionTitle(Copy.state, detail: nil)
            row(Copy.bringup, controller.bringupText)
            row(Copy.gain, "\(controller.gain)")
            row(Copy.fallen, controller.fallen ? Copy.yes : Copy.no)
            row(Copy.trunkZ, String(format: "%.4f m", controller.trunk.z))
            row(Copy.gravityZ, String(format: "%.4f", controller.gravity.z))
            row(Copy.branches, "\(controller.branchCount)")
            HStack {
                Text(Copy.drive)
                    .font(.duckCaption)
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
        .padding(DuckSpace.md)
        .background(Color.white.opacity(0.04), in: RoundedRectangle(cornerRadius: DuckRadius.control))
    }

    // MARK: - Face

    /// The virtual face: the verdict a status LED would give, the four-light
    /// battery gauge, the flashlight. A phone has no LEDs, so the sheet is
    /// where the face lives.
    private var face: some View {
        VStack(alignment: .leading, spacing: DuckSpace.sm) {
            sectionTitle(Copy.face, detail: nil)
            row(Copy.ledStatus, Copy.faceStatus(controller.faceStatus))
            // The gauge as its four LEDs: filled bars, then hollow ones, the
            // order the upstream gauge fills. Text keeps it monospaced like
            // every other value in the sheet.
            HStack {
                Text(Copy.gauge)
                    .font(.duckCaption)
                    .foregroundStyle(.secondary)
                Spacer()
                Text(gaugeBars)
                    .font(.duckData)
                    .foregroundStyle(.white)
                    .accessibilityIdentifier("sheet-gauge-bars")
                Text(Copy.faceGauge(controller.faceGauge))
                    .font(.duckData)
                    .foregroundStyle(.secondary)
            }
            row(Copy.battery, controller.faceBattery)
            flashlightRow
            tiltRow
        }
        .padding(DuckSpace.md)
        .background(Color.white.opacity(0.04), in: RoundedRectangle(cornerRadius: DuckRadius.control))
    }

    /// Tilt-to-look is a drive mode, not a separate feature: it belongs with
    /// the face's controls in "This duck" (design decision 2026-11-13). It is
    /// iOS only — the phone's IMU is the head; Android does not ship it.
    private var tiltRow: some View {
        HStack {
            Text(Copy.tiltLook)
                .font(.duckCaption)
                .foregroundStyle(.secondary)
            Spacer()
            Toggle("", isOn: $controller.tiltLook)
                .labelsHidden()
                .tint(Color.duckData)
                .accessibilityLabel(Copy.tiltLook)
                .accessibilityIdentifier("sheet-tilt-look")
        }
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
                .font(.duckCaption)
                .foregroundStyle(.secondary)
            Spacer()
            if let color = controller.flashlightColor {
                Text("\(Copy.faceColor(color)) · \(Copy.stateOn)")
                    .font(.duckData)
                    .foregroundStyle(.white)
            } else {
                Text(Copy.stateOff)
                    .font(.duckData)
                    .foregroundStyle(.secondary)
            }
            Button {
                controller.toggleFlashlight()
            } label: {
                Image(systemName: "flashlight.on.fill")
                    .font(DuckIcon.sm)
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
        VStack(alignment: .leading, spacing: DuckSpace.sm) {
            sectionTitle(Copy.whatPhoneSenses, detail: controller.sensorMode)
            Text(Copy.sensorNote)
                .font(.duckCaption)
                .foregroundStyle(.secondary)
            // A refused camera has no reading, so "—" must not hide the denial.
            // The launch LiDAR scan has its own line below the depth row:
            // unsupported hardware says so, a refused camera says denied,
            // and a completed scan says what it scanned and when.
            row(Copy.camera, controller.cameraDenied ? Copy.denied : controller.cameraSummary)
            micRow
            // The one note the pet-detect microphone branch adds. The mic
            // row is the sound switch on this phone; no second control.
            if controller.petSoundAvailable {
                Text(Copy.petSoundNote)
                    .font(.duckCaption)
                    .foregroundStyle(.secondary)
                    .accessibilityIdentifier("sheet-pet-sound-note")
            }
            row(Copy.depth, controller.depthSummary)
            // The depth viewfinder's honest line: unavailable hardware,
            // refused permission, or the one-shot scan's real result. The
            // scan never runs again until the next launch.
            Text(controller.depthScanLine)
                .font(.duckCaption)
                .foregroundStyle(.secondary)
                .accessibilityIdentifier("sheet-depth-scan")
            row("IMU", controller.imuSummary)
        }
        .padding(DuckSpace.md)
        .background(Color.white.opacity(0.04), in: RoundedRectangle(cornerRadius: DuckRadius.control))
    }

    /// The microphone is the one sensor the user switches on. Off shows "—";
    /// enabling asks permission (never at launch); refusal says "denied";
    /// granted shows the live 1 Hz values read back from GET /sensors.
    private var micRow: some View {
        HStack {
            Text(Copy.microphone)
                .font(.duckCaption)
                .foregroundStyle(.secondary)
            Spacer()
            Text(controller.micSummary)
                .font(.duckData)
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
        VStack(alignment: .leading, spacing: DuckSpace.sm) {
            sectionTitle(Copy.voiceCommands, detail: nil)
            Text(Copy.voiceNote)
                .font(.duckCaption)
                .foregroundStyle(.secondary)
            HStack {
                Text(Copy.voiceEar)
                    .font(.duckCaption)
                    .foregroundStyle(.secondary)
                Spacer()
                Text(speechStatus)
                    .font(.duckData)
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
                    .font(.duckCaption)
                    .foregroundStyle(.secondary)
                Spacer()
                Text(controller.heardText.isEmpty ? "—" : controller.heardText)
                    .font(.duckData)
                    .lineLimit(1)
                    .truncationMode(.tail)
            }
        }
        .padding(DuckSpace.md)
        .background(Color.white.opacity(0.04), in: RoundedRectangle(cornerRadius: DuckRadius.control))
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
        VStack(alignment: .leading, spacing: DuckSpace.sm) {
            sectionTitle(Copy.provenance, detail: "SHA-256")
            Text(controller.fingerprint)
                .font(.duckMicro)
                .foregroundStyle(.secondary)
                .lineLimit(3)
                .textSelection(.enabled)
            // The token a tool on another machine has to send. The duck's own
            // apps never need it, so it is here rather than in the way.
            if !controller.intentToken.isEmpty {
                Text(Copy.apiToken)
                    .font(.duckMicro)
                    .foregroundStyle(.secondary)
                Text(controller.intentToken)
                    .font(.duckMicro)
                    .foregroundStyle(.secondary)
                    .lineLimit(2)
                    .textSelection(.enabled)
            }
        }
        .padding(DuckSpace.md)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Color.white.opacity(0.04), in: RoundedRectangle(cornerRadius: DuckRadius.control))
    }

    private var footer: some View {
        HStack {
            Button(Copy.reload) { controller.reloadPolicies() }
                .buttonStyle(.bordered)
            Spacer()
            Text(Copy.api)
                .font(.duckMicro)
                .foregroundStyle(.secondary)
        }
    }

    // MARK: - Pieces

    private func sectionTitle(_ title: String, detail: String?) -> some View {
        HStack(alignment: .firstTextBaseline) {
            Text(title)
                .font(.duckLabel.weight(.semibold))
            Spacer()
            if let detail {
                Text(detail)
                    .font(.duckMicro)
                    .foregroundStyle(.secondary)
            }
        }
    }

    private func row(_ key: String, _ value: String) -> some View {
        HStack {
            Text(key)
                .font(.duckCaption)
                .foregroundStyle(.secondary)
            Spacer()
            Text(value)
                .font(.duckData)
        }
    }
}
