import SwiftUI

/// The instrument: the numbers, the sensors and the time machine, pulled up
/// from the live view rather than living on it.
struct InstrumentSheet: View {
    @ObservedObject var controller: DuckController
    /// Flipping the language re-renders the sheet's computed strings.
    @State private var languageTick = 0

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 18) {
                header
                timeMachine
                telemetry
                face
                sensors
                provenance
                footer
            }
            .padding(20)
        }
        .background(Color(red: 0.055, green: 0.075, blue: 0.10))
        .id(languageTick)
        .preferredColorScheme(.dark)
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
                    let span = max(controller.oldestTick, 1)
                    ZStack(alignment: .leading) {
                        Capsule()
                            .fill(Color.white.opacity(0.08))
                            .frame(height: 6)
                        Capsule()
                            .fill(Color.accentColor.opacity(0.6))
                            .frame(
                                width: width * CGFloat(controller.simTick - controller.oldestTick)
                                    / CGFloat(max(span, 1)),
                                height: 6
                            )
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
            .frame(maxWidth: .infinity)
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
            // What the duck is doing with its head when nobody is steering:
            // an emotion timeline, or the idle look-around. The row is the
            // evidence that this is an animal, not a frozen model.
            row(Copy.behavior, controller.behaviorSummary)
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
                    .font(.system(size: 13))
                    .frame(width: 28, height: 28)
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
            .accessibilityIdentifier("sheet-mic-toggle")
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
