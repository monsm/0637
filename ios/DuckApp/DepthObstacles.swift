import Foundation
import simd

/// A metre-space AABB in the duck's MuJoCo frame (z up, origin at the duck).
///
/// The ARKit scan works in AR world coordinates and converts every point
/// through one explicit transform before it reaches these boxes, so the
/// numbers written into `obstacles.xml` are already the frame MuJoCo loads.
struct DepthObstacleBox: Equatable {
    var min: SIMD3<Double>
    var max: SIMD3<Double>

    var center: SIMD3<Double> { (min + max) * 0.5 }
    var halfExtents: SIMD3<Double> {
        let half = (max - min) * 0.5
        // MuJoCo rejects a zero-thickness box; a one-centimetre minimum is
        // still far below anything the scan can resolve.
        return SIMD3<Double>(
            Swift.max(half.x, 0.01),
            Swift.max(half.y, 0.01),
            Swift.max(half.z, 0.01)
        )
    }
}

/// The world-space extent of the qualifying points that landed in one grid
/// cell, in the duck's MuJoCo frame.
struct DepthCellBounds: Equatable {
    var min: SIMD3<Double>
    var max: SIMD3<Double>
}

/// A rectangle of cells in the 12-row × 16-column scan grid. Row 0 is the
/// top of the depth image; adjacency is four-connected.
struct DepthCellRect: Equatable {
    var minRow: Int
    var maxRow: Int
    var minCol: Int
    var maxCol: Int

    var cellCount: Int {
        (maxRow - minRow + 1) * (maxCol - minCol + 1)
    }

    func union(_ other: DepthCellRect) -> DepthCellRect {
        DepthCellRect(
            minRow: Swift.min(minRow, other.minRow),
            maxRow: Swift.max(maxRow, other.maxRow),
            minCol: Swift.min(minCol, other.minCol),
            maxCol: Swift.max(maxCol, other.maxCol)
        )
    }
}

/// The ARKit depth scan's pure half: a 16 × 12 occupancy grid in, at most
/// three metre-space AABBs and one MJCF document out.
///
/// Deliberately no ARKit import: the simulator has no LiDAR, so everything
/// that can be tested runs there. The ticket's algorithm is written down
/// here rather than inferred: a cell is filled when more than 40 % of its
/// depth samples qualify; four-connected filled cells merge; the merged
/// rectangles are reduced to three by conservative unions (a union only
/// ever grows a box, so a small obstacle is never silently dropped).
enum DepthObstaclePlanner {
    static let columns = 16
    static let rows = 12
    /// The ticket's "填充率>40%": strictly more than 40 % of a cell's valid
    /// depth samples must qualify as obstacle points.
    static let fillThreshold = 0.4
    static let maxBoxes = 3
    static let obstacleXMLFileName = "obstacles.xml"

    /// One cell is filled when its qualifying samples exceed the threshold.
    /// An empty cell is never filled, and a mismatch of dimensions is an
    /// empty grid rather than a crash.
    static func occupancy(hits: [[Int]], samples: [[Int]]) -> [[Bool]] {
        guard hits.count == rows, samples.count == rows else {
            return Array(repeating: Array(repeating: false, count: columns), count: rows)
        }
        var grid = Array(repeating: Array(repeating: false, count: columns), count: rows)
        for row in 0..<rows where hits[row].count == columns && samples[row].count == columns {
            for col in 0..<columns {
                let seen = samples[row][col]
                guard seen > 0 else { continue }
                grid[row][col] = Double(hits[row][col]) / Double(seen) > fillThreshold
            }
        }
        return grid
    }

    /// Four-connected components of filled cells, in row-major discovery
    /// order. Diagonal neighbours are different components.
    static func connectedComponents(_ grid: [[Bool]]) -> [DepthCellRect] {
        guard grid.count == rows, grid.allSatisfy({ $0.count == columns }) else { return [] }
        var visited = Array(repeating: Array(repeating: false, count: columns), count: rows)
        var rects: [DepthCellRect] = []
        var stack: [(Int, Int)] = []
        for startRow in 0..<rows {
            for startCol in 0..<columns {
                guard grid[startRow][startCol], !visited[startRow][startCol] else { continue }
                var rect = DepthCellRect(
                    minRow: startRow, maxRow: startRow,
                    minCol: startCol, maxCol: startCol
                )
                visited[startRow][startCol] = true
                stack.append((startRow, startCol))
                while let (row, col) = stack.popLast() {
                    rect = rect.union(DepthCellRect(minRow: row, maxRow: row, minCol: col, maxCol: col))
                    let neighbours = [(row - 1, col), (row + 1, col), (row, col - 1), (row, col + 1)]
                    for (nextRow, nextCol) in neighbours {
                        guard nextRow >= 0, nextRow < rows, nextCol >= 0, nextCol < columns,
                              grid[nextRow][nextCol], !visited[nextRow][nextCol]
                        else { continue }
                        visited[nextRow][nextCol] = true
                        stack.append((nextRow, nextCol))
                    }
                }
                rects.append(rect)
            }
        }
        return rects
    }

    /// The ticket allows at most three boxes. Rather than dropping the
    /// smaller regions, the closest pair is merged until the cap holds: a
    /// union box covers more than the two parts, never less.
    static func reduce(_ rects: [DepthCellRect], to limit: Int = maxBoxes) -> [DepthCellRect] {
        guard limit > 0 else { return [] }
        var result = rects
        while result.count > limit {
            var best: (first: Int, second: Int, cells: Int)?
            for first in 0..<result.count {
                for second in (first + 1)..<result.count {
                    let cells = result[first].union(result[second]).cellCount
                    if best == nil || cells < best!.cells {
                        best = (first, second, cells)
                    }
                }
            }
            guard let pair = best else { break }
            result[pair.first] = result[pair.first].union(result[pair.second])
            result.remove(at: pair.second)
        }
        return result
    }

    /// Turn filled cells into metre-space boxes. Each component's box is
    /// the union of its cells' observed point extents, padded a little:
    /// growing a box is the conservative direction, shrinking is not.
    static func boxes(grid: [[Bool]], bounds: [[DepthCellBounds?]]) -> [DepthObstacleBox] {
        guard grid.count == rows, bounds.count == rows else { return [] }
        let components = reduce(connectedComponents(grid))
        var boxes: [DepthObstacleBox] = []
        for component in components {
            var low = SIMD3<Double>(repeating: .greatestFiniteMagnitude)
            var high = SIMD3<Double>(repeating: -.greatestFiniteMagnitude)
            var found = false
            for row in component.minRow...component.maxRow {
                for col in component.minCol...component.maxCol {
                    guard row < grid.count, col < grid[row].count, grid[row][col],
                          col < bounds[row].count,
                          let cell = bounds[row][col]
                    else { continue }
                    low = SIMD3<Double>(Swift.min(low.x, cell.min.x),
                                        Swift.min(low.y, cell.min.y),
                                        Swift.min(low.z, cell.min.z))
                    high = SIMD3<Double>(Swift.max(high.x, cell.max.x),
                                         Swift.max(high.y, cell.max.y),
                                         Swift.max(high.z, cell.max.z))
                    found = true
                }
            }
            guard found else { continue }
            // 5 cm of horizontal and 1 cm of vertical slack absorbs pose
            // jitter between the scan's frames without inventing furniture.
            low = low - SIMD3<Double>(0.05, 0.05, 0.01)
            high = high + SIMD3<Double>(0.05, 0.05, 0.01)
            // MuJoCo needs a solid box; a degenerate axis gets 4 cm.
            for axis in 0..<3 where high[axis] - low[axis] < 0.04 {
                let middle = (high[axis] + low[axis]) * 0.5
                low[axis] = middle - 0.02
                high[axis] = middle + 0.02
            }
            boxes.append(DepthObstacleBox(min: low, max: high))
        }
        return boxes
    }

    /// The official MJCF include shape: a full `<mujoco>` document whose
    /// `<worldbody>` is the fragment the core merges. The staged scene files
    /// use the same shape (`<include file="robot_…xml"/>` points at a
    /// `<mujoco>` root), so the core can treat this file as one more include.
    ///
    /// Empty boxes produce an empty string: the shell removes the file
    /// instead of claiming an empty scan was an injection.
    static func makeXML(boxes: [DepthObstacleBox]) -> String {
        guard !boxes.isEmpty else { return "" }
        let geoms = boxes.enumerated().map { index, box -> String in
            let center = box.center
            let half = box.halfExtents
            func number(_ value: Double) -> String { String(format: "%.4f", value) }
            return "<geom name=\"depth_obstacle_\(index)\" type=\"box\" "
                + "size=\"\(number(half.x)) \(number(half.y)) \(number(half.z))\" "
                + "pos=\"\(number(center.x)) \(number(center.y)) \(number(center.z))\" "
                + "rgba=\"0.55 0.60 0.62 0.35\" contype=\"1\" conaffinity=\"1\"/>"
        }.joined(separator: "\n")
        return """
        <?xml version="1.0"?>
        <mujoco model="duck-depth-obstacles">
            <!-- Generated at launch from a one-second ARKit LiDAR scan.
                 Absolute metres in the duck's MuJoCo frame; the furniture
                 does not follow the room after launch. -->
            <worldbody>
        \(geoms)
            </worldbody>
        </mujoco>
        """
    }
}

/// Where the scene actually handed to `duck_core_new` lives.
///
/// The app bundle is read-only on a phone, so "obstacles.xml beside the
/// scene" cannot mean the bundled file. The shell mirrors the whole scene
/// directory (XMLs and `assets/`: the robot XML's `meshdir="assets"` is
/// resolved relative to the scene file) into Application Support, writes
/// `obstacles.xml` there, and passes that copy's path to the core. The copy
/// is cached per bundle version + source size; only `obstacles.xml` is
/// rewritten each launch.
enum DuckSceneStaging {
    struct Result {
        /// The scene path to hand to `duck_core_new`.
        let sceneURL: URL
        /// False when the bundle scene had to be used and no obstacle file
        /// could be placed beside it.
        let wroteObstacles: Bool
    }

    static func prepare(resourceURL: URL, obstacleXML: String) -> Result {
        let bundleScene = resourceURL
            .appendingPathComponent("DuckResources/robot/microduck/scene_backlash.xml")
        // No obstacles this launch: the bundled scene is enough. A stale
        // writable copy is never passed to the core, so it cannot inject
        // last launch's furniture.
        guard !obstacleXML.isEmpty else {
            return Result(sceneURL: bundleScene, wroteObstacles: false)
        }
        guard let directory = ensureWritableScene(resourceURL: resourceURL) else {
            return Result(sceneURL: bundleScene, wroteObstacles: false)
        }
        let scene = directory.appendingPathComponent("scene_backlash.xml")
        let target = directory.appendingPathComponent(DepthObstaclePlanner.obstacleXMLFileName)
        do {
            try obstacleXML.write(to: target, atomically: true, encoding: .utf8)
            return Result(sceneURL: scene, wroteObstacles: true)
        } catch {
            // A half-written or old obstacle file must never be compiled by
            // accident; without a reliable write, fall back to the bundle.
            try? FileManager.default.removeItem(at: target)
            NSLog("duck: obstacles.xml could not be written: %@", error.localizedDescription)
            return Result(sceneURL: bundleScene, wroteObstacles: false)
        }
    }

    /// The writable mirror of `DuckResources/robot/microduck`, or nil when
    /// the copy cannot be made.
    private static func ensureWritableScene(resourceURL: URL) -> URL? {
        let fileManager = FileManager.default
        let source = resourceURL.appendingPathComponent("DuckResources/robot/microduck")
        guard fileManager.fileExists(atPath: source.appendingPathComponent("scene_backlash.xml").path),
              fileManager.fileExists(atPath: source.appendingPathComponent("assets").path)
        else { return nil }

        guard let support = fileManager.urls(for: .applicationSupportDirectory,
                                             in: .userDomainMask).first
        else { return nil }
        let version = (Bundle.main.object(forInfoDictionaryKey: "CFBundleVersion") as? String) ?? "0"
        let base = support.appendingPathComponent("DuckScene", isDirectory: true)
        let root = base.appendingPathComponent("v\(version)", isDirectory: true)
        let directory = root.appendingPathComponent("robot/microduck", isDirectory: true)
        let marker = root.appendingPathComponent(".staged")
        let scene = directory.appendingPathComponent("scene_backlash.xml")

        let stamp = "\(version)-\(fingerprint(of: source))-\(sceneStamp(of: source.appendingPathComponent("scene_backlash.xml")))"
        let cached = (try? String(contentsOf: marker, encoding: .utf8)) == stamp
        if !(cached && fileManager.fileExists(atPath: scene.path)) {
            do {
                try fileManager.createDirectory(at: root, withIntermediateDirectories: true)
                if fileManager.fileExists(atPath: directory.path) {
                    try fileManager.removeItem(at: directory)
                }
                try fileManager.copyItem(at: source, to: directory)
                try stamp.write(to: marker, atomically: true, encoding: .utf8)
            } catch {
                NSLog("duck: scene mirror failed: %@", error.localizedDescription)
                return nil
            }
        }

        // The core's model resolver checks `<scene-root>/models/
        // pet_detect.onnx` before it falls back to the policy set's parent.
        // Keep the model beside the mirrored scene too, so the mirror is
        // self-contained if the policies path ever moves; 20 KB copied each
        // prepare cannot go stale behind the scene cache.
        let model = resourceURL.appendingPathComponent("DuckResources/models/pet_detect.onnx")
        if fileManager.fileExists(atPath: model.path) {
            let models = root.appendingPathComponent("models", isDirectory: true)
            try? fileManager.createDirectory(at: models, withIntermediateDirectories: true)
            let target = models.appendingPathComponent("pet_detect.onnx")
            try? fileManager.removeItem(at: target)
            try? fileManager.copyItem(at: model, to: target)
        }

        // Old app versions leave their own 23 MB mirrors behind; this app
        // only ever compiles the current one.
        if let siblings = try? fileManager.contentsOfDirectory(at: base, includingPropertiesForKeys: nil) {
            for sibling in siblings
            where sibling.lastPathComponent.hasPrefix("v") && sibling != root {
                try? fileManager.removeItem(at: sibling)
            }
        }
        return fileManager.fileExists(atPath: scene.path) ? directory : nil
    }

    /// Byte size plus file count of the source tree. The bundle is fixed per
    /// install, so this is a cheap guard against a dev build whose version
    /// number did not change with its assets.
    private static func fingerprint(of directory: URL) -> Int {
        var total = 0
        var count = 0
        if let walker = FileManager.default.enumerator(
            at: directory,
            includingPropertiesForKeys: [.fileSizeKey],
            options: [.skipsHiddenFiles]
        ) {
            for case let url as URL in walker {
                count += 1
                total += (try? url.resourceValues(forKeys: [.fileSizeKey]).fileSize) ?? 0
            }
        }
        return total &* 31 &+ count
    }

    /// The scene's modification time: a dev rebuild can keep the version and
    /// the byte count while changing the XML, and a stale mirror would then
    /// feed MuJoCo yesterday's room.
    private static func sceneStamp(of scene: URL) -> Int {
        guard let attributes = try? FileManager.default.attributesOfItem(atPath: scene.path),
              let modified = attributes[.modificationDate] as? Date
        else { return 0 }
        return Int(modified.timeIntervalSince1970)
    }
}