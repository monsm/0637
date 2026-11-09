import Foundation
import SwiftUI
import Combine
import CoreMotion
import simd

/// Owns the Rust core and ticks it at 50 Hz on the main run loop.
///
/// The renderer and the core share the main thread, so Metal never reads
/// MuJoCo state while it is being written.
final class DuckController: ObservableObject {
    @Published var ready = false
    @Published var errorMessage: String?
    /// Set when the Metal renderer could not start. A failed renderer used to
    /// mean a black screen and no clue; this is what the user sees instead.
    @Published var renderError: String?
    @Published var bringup = 0
    @Published var fallen = false
    @Published var gain = 200
    /// What the controller is doing right now (walk / stand / sit / skill).
    /// Shown in the status panel; the UI test reads it to verify the buttons.
    @Published var label = "idle"
    @Published var joystick = CGPoint.zero
    @Published var trunk = SIMD3<Float>(0, 0, 0.12)
    @Published var gravity = SIMD3<Float>(0, 0, -1)
    /// The time machine, as the shell shows it.
    /// The simulation tick, distinct from `tick()` the 50 Hz step.
    @Published var simTick: UInt64 = 0
    @Published var oldestTick: UInt64 = 0
    @Published var paused = false
    @Published var speed = 1.0
    @Published var branchCount = 0
    @Published var fingerprint = ""
    /// The latest phone-sensor readings, already formatted for display.
    @Published var sensorMode = "twin"
    @Published var cameraSummary = "—"
    @Published var microphoneSummary = "—"
    @Published var depthSummary = "—"
    @Published var imuSummary = "—"

    private(set) var core: OpaquePointer?
    private var timer: Timer?
    private var lastStatus = Date.distantPast
    private var policyDirectory = ""
    private let motion = CMMotionManager()
    private var lastPush = Date.distantPast

    /// The renderer's triangle soups, built on the worker queue before the
    /// first frame, keyed by MuJoCo mesh id.
    private(set) var meshVertices: [Int32: [Float]] = [:]

    init() {
        // Creating the core loads the MuJoCo scene (215k vertices) and the
        // ONNX policies — seconds on a device. Doing that here blocked the
        // main thread, so SwiftUI never rendered a frame and the app showed a
        // black screen. Build it on a worker queue and publish on the main
        // thread; the core itself is only touched from the main thread
        // afterwards (timer and renderer).
        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            self?.createCore()
        }
    }

    /// Runs off the main thread; hops back to publish the result.
    private func createCore() {
        guard let resourceURL = Bundle.main.resourceURL else {
            DispatchQueue.main.async { self.errorMessage = "Bundle resources not found" }
            return
        }
        let scene = resourceURL
            .appendingPathComponent("DuckResources/robot/microduck/scene_backlash.xml")
            .path
        let policies = resourceURL.appendingPathComponent("DuckResources/policies").path
        policyDirectory = policies
        // Startup trail in the unified log, so CI can tell where it stopped.
        NSLog("duck: scene=%@", scene)
        NSLog("duck: policies=%@", policies)

        var error = [CChar](repeating: 0, count: 1024)
        let created = scene.withCString { scenePtr in
            policies.withCString { policyPtr in
                error.withUnsafeMutableBufferPointer { buffer in
                    duck_core_new(
                        scenePtr, policyPtr,
                        buffer.baseAddress, Int32(buffer.count)
                    )
                }
            }
        }

        // Parse the meshes on this worker queue: the renderer only has to
        // make GPU buffers when the first frame is built.
        let meshes: [Int32: [Float]]
        if let created {
            meshes = DuckController.parseMeshes(core: created)
        } else {
            meshes = [:]
        }

        // `[weak self]`: inside createCore `self` is non-optional, so the
        // inner closure has to capture it weakly before it can be unwrapped.
        DispatchQueue.main.async { [weak self] in
            guard let self else { return }
            guard created != nil else {
                self.errorMessage = error.withUnsafeBufferPointer {
                    String(cString: $0.baseAddress!)
                }
                NSLog("duck: duck_core_new failed: %@", self.errorMessage ?? "unknown")
                return
            }

            self.core = created
            self.meshVertices = meshes
            self.ready = true
            NSLog("duck: core created")
            let port = duck_core_start_intent_server(created, 8765)
            NSLog("duck: intent server -> %d", port)
            self.startTimer()
            self.startMotion()
            self.refreshDetails()
            self.startSensorPolling()
        }
    }

    /// Every visual geom's triangle soup, taken from MuJoCo's own mesh data
    /// on the worker queue.
    ///
    /// The renderer must not parse the STL files: MuJoCo re-centres and
    /// re-orients every mesh when it compiles the model, so `mesh_vert` and
    /// the raw file are different coordinates. Feeding the raw vertices to
    /// the renderer spreads the parts around and the duck comes apart.
    private static func parseMeshes(core: OpaquePointer) -> [Int32: [Float]] {
        var parsed: [Int32: [Float]] = [:]
        let count = Int(duck_core_geom_count(core))
        for geom in 0..<count {
            var body: Int32 = 0
            var type: Int32 = 0
            var meshID: Int32 = -1
            var group: Int32 = 0
            var position = [Double](repeating: 0, count: 3)
            var rotation = [Double](repeating: 0, count: 4)
            var color = [Float](repeating: 0, count: 4)
            let result = position.withUnsafeMutableBufferPointer { pos in
                rotation.withUnsafeMutableBufferPointer { quat in
                    color.withUnsafeMutableBufferPointer { rgba in
                        duck_core_geom_info(
                            core, Int32(geom), &body, &type, &meshID, &group,
                            pos.baseAddress, quat.baseAddress, rgba.baseAddress
                        )
                    }
                }
            }
            guard result == 0, group == 2, type == 7, meshID >= 0 else { continue }
            if parsed[meshID] != nil { continue }
            let floats = Int(duck_core_mesh_soup(core, meshID, nil, 0))
            guard floats > 0 else { continue }
            var soup = [Float](repeating: 0, count: floats)
            let written = soup.withUnsafeMutableBufferPointer { buffer in
                duck_core_mesh_soup(core, meshID, buffer.baseAddress, Int32(buffer.count))
            }
            guard Int(written) == floats else { continue }
            parsed[meshID] = soup
        }
        NSLog("duck: built %d meshes off the main thread", parsed.count)
        return parsed
    }

    /// The 50 Hz control tick, on the main run loop.
    ///
    /// The timer always fires at 20 ms; the core decides whether this frame
    /// advances simulation time. A paused duck still gets its state published,
    /// and slow motion runs one tick every few frames — the loop's own timers
    /// are on the simulation clock, so they slow down with it.
    private func startTimer() {
        let timer = Timer(timeInterval: 0.02, repeats: true) { [weak self] _ in
            self?.tick()
        }
        RunLoop.main.add(timer, forMode: .common)
        self.timer = timer
    }

    /// How many ticks this frame should advance, from the speed setting.
    private var speedRemainder: Double = 0

    private func ticksForThisFrame() -> Int {
        guard let core else { return 0 }
        // Always tick at least once: the core drains the intent queue and
        // publishes state on every call, and a paused duck must still be able
        // to receive the request that unpauses it. When paused the core does
        // not advance the simulation, so the tick is cheap.
        if duck_core_paused(core) != 0 {
            speedRemainder = 0
            return 1
        }
        let speed = duck_core_speed(core)
        speedRemainder += speed
        let whole = Int(speedRemainder)
        speedRemainder -= Double(whole)
        return max(1, min(whole, 8))
    }

    deinit {
        timer?.invalidate()
        motion.stopDeviceMotionUpdates()
        if let core {
            duck_core_free(core)
        }
    }

    /// The duck is in Twin mode: the phone's IMU does not replace its
    /// proprioception, it applies external disturbances (a shake is a shove).
    private func startMotion() {
        guard motion.isDeviceMotionAvailable else { return }
        motion.deviceMotionUpdateInterval = 0.02
        motion.startDeviceMotionUpdates(to: .main) { [weak self] motion, _ in
            guard let self, let motion else { return }
            let acceleration = motion.userAcceleration
            let magnitude = sqrt(
                acceleration.x * acceleration.x
                    + acceleration.y * acceleration.y
                    + acceleration.z * acceleration.z
            )
            guard magnitude > 1.2,
                  Date().timeIntervalSince(self.lastPush) > 0.4 else { return }
            self.lastPush = Date()
            let scale = 0.8
            self.push(
                x: acceleration.x * scale,
                y: acceleration.y * scale,
                z: acceleration.z * scale
            )
        }
    }

    /// The sensor sheet reads what the core last saw. The shell polls its own
    /// server so the phone sensors have one path in (POST /sensor) and one out.
    private var sensorTimer: Timer?

    func startSensorPolling() {
        guard sensorTimer == nil else { return }
        let timer = Timer(timeInterval: 1.0, repeats: true) { [weak self] _ in
            self?.pollSensors()
        }
        RunLoop.main.add(timer, forMode: .common)
        sensorTimer = timer
    }

    private func pollSensors() {
        guard let url = URL(string: "http://127.0.0.1:8765/sensors") else { return }
        var request = URLRequest(url: url)
        request.timeoutInterval = 1.5
        URLSession.shared.dataTask(with: request) { [weak self] data, _, _ in
            guard let data,
                  let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any]
            else { return }
            DispatchQueue.main.async {
                guard let self else { return }
                self.sensorMode = json["mode"] as? String ?? "twin"
                self.cameraSummary = Self.summary(json["camera"], keys: ["width", "height", "exposure_s"])
                self.microphoneSummary = Self.summary(json["microphone"], keys: ["rms", "peak_hz"])
                self.depthSummary = Self.summary(json["depth"], keys: ["rows", "cols", "min_m", "max_m"])
                self.imuSummary = Self.summary(json["imu"], keys: ["acceleration"])
            }
        }.resume()
    }

    private static func summary(_ reading: Any?, keys: [String]) -> String {
        guard let reading = reading as? [String: Any],
              let value = reading["value"] as? [String: Any] else { return "—" }
        let parts: [String] = keys.compactMap { key in
            guard let raw = value[key] else { return nil }
            if let number = raw as? NSNumber { return "\(key) \(number)" }
            if let array = raw as? [NSNumber] {
                return "\(key) " + array.map { String(format: "%.2f", $0.doubleValue) }.joined(separator: ",")
            }
            return nil
        }
        return parts.isEmpty ? "—" : parts.joined(separator: " · ")
    }

    private func refreshClock() {
        guard let core else { return }
        simTick = duck_core_tick_index(core)
        let oldest = duck_core_oldest_tick(core)
        oldestTick = oldest == 0 ? simTick : oldest
        paused = duck_core_paused(core) != 0
        speed = duck_core_speed(core)
    }

    /// The time machine, from the shell's own buttons.
    func setPaused(_ value: Bool) {
        guard let core else { return }
        duck_core_set_paused(core, value ? 1 : 0)
        paused = value
    }

    func step() {
        guard let core else { return }
        duck_core_step(core, 1)
        refreshClock()
    }

    func setSpeed(_ value: Double) {
        guard let core else { return }
        duck_core_set_speed(core, value)
        speed = value
    }

    /// Step back half a second and resume from there.
    func rewind() {
        guard let core else { return }
        _ = duck_core_seek(core, 25)
        refreshClock()
    }

    func fork() {
        guard let core else { return }
        if duck_core_fork(core, nil) == 0 {
            branchCount += 1
        }
    }

    /// Load the fingerprint and the sensor summaries once the core is up.
    func refreshDetails() {
        guard let core else { return }
        if let pointer = duck_core_fingerprint(core) {
            fingerprint = String(cString: pointer)
        }
    }

    /// robotd's AxisBinding::evaluate: inside 0.1 the axis reads zero.
    private func deadzone(_ value: Float) -> Float {
        abs(value) < 0.1 ? 0 : value
    }

    private func push(x: Double, y: Double, z: Double) {
        guard let core else { return }
        _ = duck_core_push(core, x, y, z)
    }

    private func tick() {
        guard let core else { return }

        // The core has robotd's 500 ms deadman: a twist must keep arriving.
        // The joystick is sent every tick, zero included.
        // robotd's pad: a 0.1 deadzone per axis, yaw negated (clockwise is
        // positive on the real controller).
        let x = deadzone(Float(joystick.x))
        let y = deadzone(Float(joystick.y))
        duck_core_set_twist(core, Double(-y) * 0.4, 0.0, Double(-x) * 1.0)
        // Paused or slowed: the timer keeps firing, simulation time does not.
        for _ in 0..<ticksForThisFrame() {
            _ = duck_core_tick(core)
        }

        if Date().timeIntervalSince(lastStatus) > 0.1 {
            var status = DuckStatus()
            if duck_core_status(core, &status) == 0 {
                bringup = Int(status.bringup)
                fallen = status.fallen != 0
                gain = Int(status.gain)
                trunk = SIMD3<Float>(
                    Float(status.trunk.0),
                    Float(status.trunk.1),
                    Float(status.trunk.2)
                )
                gravity = SIMD3<Float>(
                    Float(status.gravity.0),
                    Float(status.gravity.1),
                    Float(status.gravity.2)
                )
                // `char label[64]` imports as a 64-tuple of CChar.
                label = withUnsafeBytes(of: status.label) { raw in
                    guard let base = raw.baseAddress else { return "" }
                    return String(cString: base.assumingMemoryBound(to: CChar.self))
                }
            }
            lastStatus = Date()
            refreshClock()
        }
    }

    func sit() {
        guard let core else { return }
        _ = duck_core_sit_toggle(core)
    }

    func groundPick() {
        guard let core else { return }
        _ = duck_core_ground_pick(core)
    }

    func trigger(_ name: String) {
        guard let core else { return }
        name.withCString { _ = duck_core_trigger_skill(core, $0) }
    }

    /// Reload a user-supplied policy set from Documents/policies when it has a
    /// manifest, otherwise the bundled official set. Custom ONNX runs under the
    /// same 61-D observation contract as the robot.
    func reloadPolicies() {
        guard let core else { return }
        let documents = FileManager.default.urls(for: .documentDirectory, in: .userDomainMask).first
        let userPolicies = documents?.appendingPathComponent("policies")
        let directory: String
        if let userPolicies,
           FileManager.default.fileExists(atPath: userPolicies.appendingPathComponent("manifest.json").path) {
            directory = userPolicies.path
        } else {
            directory = policyDirectory
        }
        directory.withCString { _ = duck_core_reload_policies(core, $0) }
    }

    func skillNames() -> [String] {
        guard let core else { return [] }
        let count = Int(duck_core_skill_count(core))
        return (0..<count).compactMap { index in
            guard let pointer = duck_core_skill_name(core, Int32(index)) else { return nil }
            return String(cString: pointer)
        }
    }

    var bringupText: String {
        switch bringup {
        case 0: return "Limp"
        case 1: return "Homing"
        case 2: return "Ready"
        default: return "?"
        }
    }
}
