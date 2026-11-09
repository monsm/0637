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
        let meshes: [String: [Float]]
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
    private func startTimer() {
        let timer = Timer(timeInterval: 0.02, repeats: true) { [weak self] _ in
            self?.tick()
        }
        RunLoop.main.add(timer, forMode: .common)
        self.timer = timer
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

    private func push(x: Double, y: Double, z: Double) {
        guard let core else { return }
        _ = duck_core_push(core, x, y, z)
    }

    private func tick() {
        guard let core else { return }

        // The core has robotd's 500 ms deadman: a twist must keep arriving.
        // The joystick is sent every tick, zero included.
        let x = Float(joystick.x)
        let y = Float(joystick.y)
        duck_core_set_twist(core, Double(-y) * 0.4, 0.0, Double(x) * 1.0)
        _ = duck_core_tick(core)

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
                // `char label[64]` imports as a 64-tuple of CChar.
                label = withUnsafeBytes(of: status.label) { raw in
                    guard let base = raw.baseAddress else { return "" }
                    return String(cString: base.assumingMemoryBound(to: CChar.self))
                }
            }
            lastStatus = Date()
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
