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
    @Published var bringup = 0
    @Published var fallen = false
    @Published var gain = 200
    @Published var joystick = CGPoint.zero
    @Published var trunk = SIMD3<Float>(0, 0, 0.12)

    private(set) var core: OpaquePointer?
    private var timer: Timer?
    private var lastStatus = Date.distantPast
    private var policyDirectory = ""
    private let motion = CMMotionManager()
    private var lastPush = Date.distantPast

    init() {
        guard let resourceURL = Bundle.main.resourceURL else {
            errorMessage = "Bundle resources not found"
            return
        }
        let scene = resourceURL
            .appendingPathComponent("Resources/robot/microduck/scene_backlash.xml")
            .path
        let policies = resourceURL.appendingPathComponent("Resources/policies").path
        policyDirectory = policies

        var error = [CChar](repeating: 0, count: 1024)
        core = scene.withCString { scenePtr in
            policies.withCString { policyPtr in
                error.withUnsafeMutableBufferPointer { buffer in
                    duck_core_new(
                        scenePtr, policyPtr,
                        buffer.baseAddress, Int32(buffer.count)
                    )
                }
            }
        }
        guard core != nil else {
            errorMessage = error.withUnsafeBufferPointer {
                String(cString: $0.baseAddress!)
            }
            return
        }

        ready = true
        if let core {
            _ = duck_core_start_intent_server(core, 8765)
        }
        let timer = Timer(timeInterval: 0.02, repeats: true) { [weak self] _ in
            self?.tick()
        }
        RunLoop.main.add(timer, forMode: .common)
        self.timer = timer
        startMotion()
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
