import ARKit
import AVFoundation
import CoreVideo
import Foundation
import QuartzCore
import simd

/// What a launch scan found.
struct DepthScanResult {
    /// At most three boxes, in the duck's MuJoCo frame, absolute metres.
    var boxes: [DepthObstacleBox]
    /// The scene-relative floor offset to hand to `POST /floor`.
    ///
    /// The core's route speaks metres **from the scene's default floor** and
    /// moves the plane and the duck together; it explicitly refuses a camera
    /// height. This app has no AR anchor for the duck's MuJoCo origin — the
    /// scan's detected plane is therefore defined as the duck's floor, so
    /// the honest offset is 0. A nonzero offset needs the duck anchored in
    /// the AR world, which is a renderer feature this ticket does not add.
    var floorOffset: Double?
    /// How many depth frames actually contributed.
    var frames: Int
}

/// The one-shot scan's answer. The shell must tell an unavailable sensor
/// apart from a refused permission and a scan that simply saw nothing.
enum DepthScanOutcome {
    /// This device has no LiDAR / no ARKit world tracking (also: simulator).
    case unsupported
    /// The camera permission was not granted, so no depth frame can exist.
    /// A first launch answers this without prompting; Director owns the
    /// prompt and a later launch uses the grant.
    case denied
    /// The session ran but produced no usable floor or no depth frames.
    case failed
    case done(DepthScanResult)
}

/// What the sensors card has to say about the launch scan. Presentation
/// only; the scan itself has already finished (or was honestly unavailable).
enum DepthScanStatus: Equatable {
    case unsupported
    case denied
    case scanning
    /// The floor offset this launch can justify, or nil when no plane was seen.
    case scanned(boxes: Int, floorOffset: Double?)
    case failed
}

/// A one-second LiDAR scan at launch: scene depth in, `DepthScanResult` out.
///
/// The ticket's whole point is that the furniture is real at the moment the
/// core compiles the scene, so this runs before `duck_core_new` and stops
/// its `ARSession` the moment it has its answer. It is not kept alive: the
/// rear camera must be free again for Director mode, and ARKit and
/// `AVCaptureSession` cannot own it at the same time.
///
/// Coordinates: ARKit is right-handed, y up, camera looking down −z. The
/// duck's frame is the one the parallel Android scan writes and the J1 core
/// merges: origin at the phone's ground projection, x forward (the camera's
/// optical axis projected onto the floor), y left, z up from the detected
/// plane. The forward axis is fixed from the first frame, so every box is
/// oriented like the duck the person saw.
///
/// "Below the desktop" is the depth map's wording: an object resting on the
/// desk is *nearer to the camera* (smaller depth) than the desk surface
/// behind it, which in gravity-aligned coordinates means standing **above**
/// the plane. The scan classifies those points (0.06…1.5 m above the plane),
/// exactly the way Android's `DepthGeometry.scene` does; points behind the
/// plane are the room, not something the duck can walk into.
///
/// The floor it reports is the core's scene-relative offset: the detected
/// plane is defined as the duck's floor, so this launch reports 0. The core
/// explicitly refuses a camera height, and ARKit's per-launch camera origin
/// gives no anchor from which a nonzero offset could honestly be derived;
/// that needs the duck placed in the AR world, which this shell does not do.
final class DepthScanner: NSObject {
    /// One cell's accumulated hits, valid samples and the world extent of
    /// its qualifying points.
    private struct CellStats {
        var hits = 0
        var samples = 0
        var minPoint = SIMD3<Double>(repeating: .greatestFiniteMagnitude)
        var maxPoint = SIMD3<Double>(repeating: -.greatestFiniteMagnitude)
        var hasBounds = false
    }

    /// LiDAR is a hardware capability. The simulator reports no scene-depth
    /// frame semantics, and neither does a phone without a depth camera;
    /// both must stay honest rather than invent a grid.
    static let isSupported: Bool = {
        #if targetEnvironment(simulator)
        return false
        #else
        return ARWorldTrackingConfiguration.isSupported
            && ARWorldTrackingConfiguration.supportsFrameSemantics(.sceneDepth)
        #endif
    }()

    /// The scan resolves on the main thread exactly once. Main-thread state.
    private var completion: ((DepthScanOutcome) -> Void)?
    private var scanning = false

    private let session = ARSession()
    /// ARSession delegate callbacks and all scan bookkeeping run here.
    private let scanQueue = DispatchQueue(label: "duck.depth-scan")

    // Scan state: scanQueue only.
    private var planes: [UUID: ARPlaneAnchor] = [:]
    private var stats = Array(repeating: CellStats(), count: DepthObstaclePlanner.rows * DepthObstaclePlanner.columns)
    private var startTime: CFTimeInterval?
    private var startCameraY: Float = 0
    /// The duck's horizontal axes, fixed at the first frame: x forward is
    /// the camera's optical axis projected onto the floor, y left is its
    /// right-hand neighbour. Android constructs the same frame.
    private var startForward = SIMD2<Float>(0, -1)
    private var startRight = SIMD2<Float>(1, 0)
    /// The one plane the whole scan uses. Android's fit runs on one frame;
    /// fixing the first valid plane here keeps every box's height and the
    /// `/floor` number measured from the same surface even if another plane
    /// anchor appears mid-second.
    private var scanPlaneY: Float?
    private var frames = 0
    private var sawDepth = false
    private var finished = false

    /// Scan for one second of frames (ARScene depth arrives at display rate;
    /// 1 s is ~60 frames). If the phone is interrupted or the scan yields no
    /// frame at all, a three-second watchdog answers `.failed` instead of
    /// leaving the core waiting forever.
    ///
    /// The camera prompt belongs to Director, not to this launch path
    /// (Android's Camera2 scan makes the same rule: no CAMERA grant, no
    /// scan). A first launch therefore answers `.denied` honestly, and the
    /// first Director visit grants the camera for the next launch.
    func scan(completion: @escaping (DepthScanOutcome) -> Void) {
        guard Self.isSupported else {
            completion(.unsupported)
            return
        }
        guard !scanning else { return }
        switch AVCaptureDevice.authorizationStatus(for: .video) {
        case .authorized:
            begin(completion)
        case .denied, .restricted, .notDetermined:
            completion(.denied)
        @unknown default:
            completion(.denied)
        }
    }

    /// Stop the session without answering. Used when the launch watchdog
    /// has already decided to build the core without a scan.
    func cancel() {
        completion = nil
        scanning = false
        scanQueue.async { [weak self] in
            self?.finished = true
        }
        // Stop the frames themselves: a paused session with a live delegate
        // could still deliver one more frame and run a full grid pass after
        // the watchdog has already built the core.
        session.delegate = nil
        session.pause()
    }

    private func begin(_ completion: @escaping (DepthScanOutcome) -> Void) {
        scanning = true
        self.completion = completion
        scanQueue.async { [weak self] in
            guard let self else { return }
            self.planes.removeAll()
            self.stats = Array(
                repeating: CellStats(),
                count: DepthObstaclePlanner.rows * DepthObstaclePlanner.columns
            )
            self.startTime = nil
            self.startCameraY = 0
            self.startForward = SIMD2<Float>(0, -1)
            self.startRight = SIMD2<Float>(1, 0)
            self.scanPlaneY = nil
            self.frames = 0
            self.sawDepth = false
            self.finished = false
            self.scanQueue.asyncAfter(deadline: .now() + 3.0) { [weak self] in
                self?.watchdogFired()
            }
        }
        let configuration = ARWorldTrackingConfiguration()
        configuration.worldAlignment = .gravity
        configuration.planeDetection = [.horizontal]
        configuration.isLightEstimationEnabled = false
        configuration.frameSemantics.insert(.sceneDepth)
        session.delegate = self
        session.delegateQueue = scanQueue
        session.run(configuration, options: [.resetTracking, .removeExistingAnchors])
    }

    /// ScanQueue.
    private func watchdogFired() {
        guard !finished else { return }
        if sawDepth, let outcome = makeOutcome() {
            finish(outcome)
        } else {
            finish(.failed)
        }
    }

    /// ScanQueue. A complete scan has a floor plane; without one the obstacle
    /// filter has no meaning and nothing is invented.
    private func makeOutcome() -> DepthScanOutcome? {
        guard sawDepth, frames > 3 else { return nil }
        guard scanPlaneY != nil else { return nil }
        // The core's `/floor` is a scene-relative offset (0 = unchanged),
        // not the camera height; ARKit's per-launch camera origin gives no
        // anchor that could make it nonzero. The detected plane is the
        // duck's floor, so this launch reports the honest 0 rather than a
        // measured height the core would refuse.
        let floorOffset = 0.0
        var hits = Array(repeating: Array(repeating: 0, count: DepthObstaclePlanner.columns),
                         count: DepthObstaclePlanner.rows)
        var samples = hits
        var bounds = Array(repeating: Array(repeating: Optional<DepthCellBounds>.none,
                                            count: DepthObstaclePlanner.columns),
                           count: DepthObstaclePlanner.rows)
        for row in 0..<DepthObstaclePlanner.rows {
            for col in 0..<DepthObstaclePlanner.columns {
                let cell = stats[row * DepthObstaclePlanner.columns + col]
                hits[row][col] = cell.hits
                samples[row][col] = cell.samples
                if cell.hasBounds {
                    bounds[row][col] = DepthCellBounds(min: cell.minPoint, max: cell.maxPoint)
                }
            }
        }
        let grid = DepthObstaclePlanner.occupancy(hits: hits, samples: samples)
        let boxes = DepthObstaclePlanner.boxes(grid: grid, bounds: bounds)
        return .done(DepthScanResult(boxes: boxes, floorOffset: floorOffset, frames: frames))
    }

    /// ScanQueue. Stop the camera, answer once, on the main thread.
    private func finish(_ outcome: DepthScanOutcome) {
        guard !finished else { return }
        finished = true
        DispatchQueue.main.async { [weak self] in
            guard let self else { return }
            self.session.delegate = nil
            self.session.pause()
            self.scanning = false
            let completion = self.completion
            self.completion = nil
            completion?(outcome)
        }
    }

    // MARK: - Per-frame classification

    /// ScanQueue. The plane the duck walks on: the largest horizontal surface
    /// under the phone, between 5 cm and 2 m below it. That is Android's
    /// dominant-plane rule seen through ARKit's anchors (the desk when the
    /// desk is what the phone sees, the room floor when it is not).
    private func referencePlaneY() -> Float? {
        let candidates = planes.values.filter { plane in
            guard plane.alignment == .horizontal,
                  plane.transform.columns.1.y >= 0.9
            else { return false }
            let height = startCameraY - plane.transform.columns.3.y
            return height >= 0.05 && height <= 2.0
        }
        return candidates.max { first, second in
            let firstArea = first.planeExtent.width * first.planeExtent.height
            let secondArea = second.planeExtent.width * second.planeExtent.height
            if firstArea != secondArea { return firstArea < secondArea }
            return first.transform.columns.3.y < second.transform.columns.3.y
        }?.transform.columns.3.y
    }

    /// ScanQueue. Walk the depth map in 16 × 12 cells. A stride of two pixels
    /// keeps a second of LiDAR frames cheap on the scan queue without
    /// changing which cell a point belongs to. The one-second vote is the
    /// ticket's median map's robust core: a cell must be filled in most of
    /// its valid samples, never just in one noisy frame.
    private func accumulate(_ frame: ARFrame, referenceY: Float) {
        guard let sceneDepth = frame.sceneDepth else { return }
        let depthMap = sceneDepth.depthMap
        guard CVPixelBufferLockBaseAddress(depthMap, .readOnly) == kCVReturnSuccess else { return }
        defer { CVPixelBufferUnlockBaseAddress(depthMap, .readOnly) }

        let width = CVPixelBufferGetWidth(depthMap)
        let height = CVPixelBufferGetHeight(depthMap)
        guard width > 0, height > 0,
              let base = CVPixelBufferGetBaseAddress(depthMap)
        else { return }
        let rowBytes = CVPixelBufferGetBytesPerRow(depthMap)
        let format = CVPixelBufferGetPixelFormatType(depthMap)
        guard format == kCVPixelFormatType_DepthFloat32 || format == kCVPixelFormatType_DepthFloat16
        else { return }

        // The intrinsics describe the full camera image; the depth map is a
        // scaled copy of it, so scale the focal lengths and principal point.
        let image = frame.camera.imageResolution
        guard image.width > 0, image.height > 0 else { return }
        let scaleX = Float(width) / Float(image.width)
        let scaleY = Float(height) / Float(image.height)
        let intrinsics = frame.camera.intrinsics
        let fx = intrinsics[0][0] * scaleX
        let fy = intrinsics[1][1] * scaleY
        let cx = intrinsics[2][0] * scaleX
        let cy = intrinsics[2][1] * scaleY
        guard fx > 0, fy > 0 else { return }
        let transform = frame.camera.transform

        for row in 0..<DepthObstaclePlanner.rows {
            let rowStart = row * height / DepthObstaclePlanner.rows
            let rowEnd = (row + 1) * height / DepthObstaclePlanner.rows
            for col in 0..<DepthObstaclePlanner.columns {
                let colStart = col * width / DepthObstaclePlanner.columns
                let colEnd = (col + 1) * width / DepthObstaclePlanner.columns
                var cell = stats[row * DepthObstaclePlanner.columns + col]
                var pixelRow = rowStart
                while pixelRow < rowEnd {
                    let line = base.advanced(by: pixelRow * rowBytes)
                    var pixelCol = colStart
                    while pixelCol < colEnd {
                        let depth: Float
                        switch format {
                        case kCVPixelFormatType_DepthFloat32:
                            depth = line.assumingMemoryBound(to: Float32.self)[pixelCol]
                        default:
                            let bits = line.assumingMemoryBound(to: UInt16.self)[pixelCol]
                            depth = Float(Float16(bitPattern: bits))
                        }
                        // 10 cm rejects LiDAR's near-field ghosts; 4 m is the
                        // ticket's horizontal bound and the honest reach here.
                        if depth.isFinite, depth > 0.1, depth < 4.0 {
                            cell.samples += 1
                            let x = (Float(pixelCol) + 0.5 - cx) / fx * depth
                            let y = (Float(pixelRow) + 0.5 - cy) / fy * depth
                            // ARKit's camera looks down -z.
                            let world = transform * SIMD4<Float>(x, y, -depth, 1)
                            // The camera's optical axis projected onto the floor becomes
                            // the duck's forward; y left is -right. Height is
                            // measured from the detected plane, so a box that
                            // stands on it starts at z ≈ 0, like Android's.
                            let dx = world.x
                            let dz = world.z
                            let forward = dx * startForward.x + dz * startForward.y
                            let right = dx * startRight.x + dz * startRight.y
                            let height = world.y - referenceY
                            let horizontal = forward * forward + right * right
                            // Above the plane (nearer than the desk in the
                            // depth map), inside the ticket's 4 m horizontal
                            // circle and Android's 0.06...1.5 m height band.
                            if height > 0.06, height < 1.5,
                               forward > 0.1, forward < 4.0,
                               abs(right) < 4.0, horizontal < 16.0 {
                                let point = SIMD3<Double>(
                                    Double(forward),
                                    Double(-right),
                                    Double(height)
                                )
                                cell.hits += 1
                                cell.minPoint = SIMD3<Double>(
                                    Swift.min(cell.minPoint.x, point.x),
                                    Swift.min(cell.minPoint.y, point.y),
                                    Swift.min(cell.minPoint.z, point.z)
                                )
                                cell.maxPoint = SIMD3<Double>(
                                    Swift.max(cell.maxPoint.x, point.x),
                                    Swift.max(cell.maxPoint.y, point.y),
                                    Swift.max(cell.maxPoint.z, point.z)
                                )
                                cell.hasBounds = true
                            }
                        }
                        pixelCol += 2
                    }
                    pixelRow += 2
                }
                stats[row * DepthObstaclePlanner.columns + col] = cell
            }
        }
    }
}

extension DepthScanner: ARSessionDelegate {
    func session(_ session: ARSession, didUpdate frame: ARFrame) {
        // ScanQueue.
        guard !finished else { return }
        if startTime == nil {
            startTime = CACurrentMediaTime()
            let transform = frame.camera.transform
            startCameraY = transform.columns.3.y
            // x forward: the camera looks down its own -z; project that
            // direction onto the gravity-aligned horizontal plane. A phone
            // pointed almost straight down has no meaningful heading, so
            // the camera's up axis is the fallback, then AR world -z.
            var forward = SIMD2<Float>(-transform.columns.2.x, -transform.columns.2.z)
            if simd_length(forward) < 0.1 {
                forward = SIMD2<Float>(transform.columns.1.x, transform.columns.1.z)
            }
            if simd_length(forward) < 0.1 {
                forward = SIMD2<Float>(0, -1)
            }
            forward = simd_normalize(forward)
            startForward = forward
            // right = forward × up; in the x/z plane that is (-fz, fx).
            startRight = SIMD2<Float>(-forward.y, forward.x)
        }
        frames += 1
        if frame.sceneDepth != nil { sawDepth = true }
        if scanPlaneY == nil { scanPlaneY = referencePlaneY() }
        if let reference = scanPlaneY {
            accumulate(frame, referenceY: reference)
        }
        guard let start = startTime else { return }
        if CACurrentMediaTime() - start >= 1.0 {
            finish(makeOutcome() ?? .failed)
        }
    }

    func session(_ session: ARSession, didAdd anchors: [ARAnchor]) {
        record(anchors)
    }

    func session(_ session: ARSession, didUpdate anchors: [ARAnchor]) {
        record(anchors)
    }

    func session(_ session: ARSession, didRemove anchors: [ARAnchor]) {
        for anchor in anchors {
            planes.removeValue(forKey: anchor.identifier)
        }
    }

    func session(_ session: ARSession, didFailWithError error: Error) {
        NSLog("duck: AR depth scan failed: %@", error.localizedDescription)
        finish(.failed)
    }

    /// ScanQueue.
    private func record(_ anchors: [ARAnchor]) {
        for anchor in anchors {
            if let plane = anchor as? ARPlaneAnchor {
                planes[plane.identifier] = plane
            }
        }
    }
}