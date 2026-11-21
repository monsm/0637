import XCTest
@testable import Duck

/// The depth scan's testable half. The ARKit session itself needs a LiDAR
/// phone (the simulator reports no scene depth), so these tests pin the
/// ticket's algorithm: 16 × 12 cells, >40 % fill, four-connected merge,
/// at most three boxes, and the MJCF shape the core includes.
final class DepthObstacleTests: XCTestCase {
    private let columns = DepthObstaclePlanner.columns
    private let rows = DepthObstaclePlanner.rows

    private func emptyGrid() -> [[Bool]] {
        Array(repeating: Array(repeating: false, count: columns), count: rows)
    }

    private func grid(filled cells: [(row: Int, col: Int)]) -> [[Bool]] {
        var grid = emptyGrid()
        for cell in cells {
            grid[cell.row][cell.col] = true
        }
        return grid
    }

    private func emptyBounds() -> [[DepthCellBounds?]] {
        Array(repeating: Array(repeating: Optional<DepthCellBounds>.none, count: columns),
              count: rows)
    }

    func testTheGridIsTheTicketShape() {
        XCTAssertEqual(columns, 16)
        XCTAssertEqual(rows, 12)
        XCTAssertEqual(DepthObstaclePlanner.maxBoxes, 3)
        XCTAssertEqual(DepthObstaclePlanner.fillThreshold, 0.4, accuracy: 1e-12)
    }

    /// Exactly 40 % is not "more than 40 %"; a single sample above it is.
    func testFillThresholdIsStrictlyMoreThanFortyPercent() {
        var hits = Array(repeating: Array(repeating: 0, count: columns), count: rows)
        var samples = Array(repeating: Array(repeating: 0, count: columns), count: rows)
        samples[0][0] = 10
        hits[0][0] = 4
        samples[0][1] = 10
        hits[0][1] = 5
        let grid = DepthObstaclePlanner.occupancy(hits: hits, samples: samples)
        XCTAssertFalse(grid[0][0], "40 % exactly is not a filled cell")
        XCTAssertTrue(grid[0][1], "50 % is")
        XCTAssertFalse(grid[1][1], "a cell with no samples is never filled")
    }

    /// One filled cell becomes one box that still contains the observed
    /// points after the conservative padding.
    func testSingleFilledCellBecomesOneBox() {
        let grid = grid(filled: [(5, 7)])
        let cell = DepthCellBounds(
            min: SIMD3<Double>(1.0, 2.0, 0.5),
            max: SIMD3<Double>(1.2, 2.2, 0.8)
        )
        var bounds = emptyBounds()
        bounds[5][7] = cell
        let boxes = DepthObstaclePlanner.boxes(grid: grid, bounds: bounds)
        XCTAssertEqual(boxes.count, 1)
        guard let box = boxes.first else { return }
        XCTAssertLessThanOrEqual(box.min.x, cell.min.x)
        XCTAssertLessThanOrEqual(box.min.y, cell.min.y)
        XCTAssertLessThanOrEqual(box.min.z, cell.min.z)
        XCTAssertGreaterThanOrEqual(box.max.x, cell.max.x)
        XCTAssertGreaterThanOrEqual(box.max.y, cell.max.y)
        XCTAssertGreaterThanOrEqual(box.max.z, cell.max.z)
        XCTAssertGreaterThan(box.halfExtents.x, 0)
        XCTAssertGreaterThan(box.halfExtents.y, 0)
        XCTAssertGreaterThan(box.halfExtents.z, 0)
    }

    /// Four-connected (not eight): a horizontal run and a cell below it
    /// merge into one component and therefore one box.
    func testFourConnectedCellsMergeIntoOneBox() {
        let grid = grid(filled: [(5, 7), (5, 8), (6, 8)])
        var bounds = emptyBounds()
        bounds[5][7] = DepthCellBounds(min: SIMD3<Double>(0, 0, 0),
                                       max: SIMD3<Double>(0.2, 0.2, 0.2))
        bounds[5][8] = DepthCellBounds(min: SIMD3<Double>(0.3, 0, 0),
                                       max: SIMD3<Double>(0.4, 0.2, 0.2))
        bounds[6][8] = DepthCellBounds(min: SIMD3<Double>(0.3, 0.3, 0),
                                       max: SIMD3<Double>(0.4, 0.4, 0.2))
        let boxes = DepthObstaclePlanner.boxes(grid: grid, bounds: bounds)
        XCTAssertEqual(boxes.count, 1)
        XCTAssertEqual(DepthObstaclePlanner.connectedComponents(grid).count, 1)
    }

    /// Diagonal neighbours are separate components, so two boxes survive
    /// (the cap is three, nothing needs merging).
    func testDiagonalCellsAreNotMerged() {
        let grid = grid(filled: [(5, 7), (6, 8)])
        var bounds = emptyBounds()
        bounds[5][7] = DepthCellBounds(min: SIMD3<Double>(0, 0, 0),
                                       max: SIMD3<Double>(0.1, 0.1, 0.1))
        bounds[6][8] = DepthCellBounds(min: SIMD3<Double>(2, 2, 0),
                                       max: SIMD3<Double>(2.1, 2.1, 0.1))
        XCTAssertEqual(DepthObstaclePlanner.connectedComponents(grid).count, 2)
        XCTAssertEqual(DepthObstaclePlanner.boxes(grid: grid, bounds: bounds).count, 2)
    }

    /// Nothing seen means no file, not an empty file the core would still
    /// try to include.
    func testEmptyInputMakesNoBoxesAndNoXML() {
        XCTAssertEqual(DepthObstaclePlanner.connectedComponents(emptyGrid()).count, 0)
        XCTAssertEqual(
            DepthObstaclePlanner.boxes(grid: emptyGrid(), bounds: emptyBounds()).count,
            0
        )
        XCTAssertEqual(DepthObstaclePlanner.makeXML(boxes: []), "")
    }

    /// More than three regions are merged, never dropped: every original
    /// point still lies inside some box.
    func testMoreThanThreeRegionsReduceConservatively() {
        let cells = [(0, 0), (0, 15), (11, 0), (11, 15)]
        let grid = grid(filled: cells)
        var bounds = emptyBounds()
        var corners: [SIMD3<Double>] = []
        for (index, cell) in cells.enumerated() {
            let origin = Double(index) * 10.0
            let low = SIMD3<Double>(origin, origin, 0.0)
            let high = SIMD3<Double>(origin + 0.2, origin + 0.2, 0.3)
            bounds[cell.0][cell.1] = DepthCellBounds(min: low, max: high)
            corners.append((low + high) * 0.5)
        }
        let boxes = DepthObstaclePlanner.boxes(grid: grid, bounds: bounds)
        XCTAssertEqual(boxes.count, 3, "the cap is three")
        for corner in corners {
            let covered = boxes.contains { box in
                box.min.x <= corner.x && corner.x <= box.max.x
                    && box.min.y <= corner.y && corner.y <= box.max.y
                    && box.min.z <= corner.z && corner.z <= box.max.z
            }
            XCTAssertTrue(covered, "a conservative merge must not drop a region")
        }
    }

    /// The writer's shape is the official MJCF include shape: a `<mujoco>`
    /// document with a `<worldbody>` fragment, so the core can include it
    /// exactly like it includes `robot_…xml`.
    func testObstacleXMLHasTheOfficialMJCFShape() {
        let boxes = [
            DepthObstacleBox(min: SIMD3<Double>(-1, -2, -0.5),
                             max: SIMD3<Double>(1, 2, -0.3)),
            DepthObstacleBox(min: SIMD3<Double>(0.5, 0.5, -0.4),
                             max: SIMD3<Double>(0.7, 0.9, -0.1)),
        ]
        let xml = DepthObstaclePlanner.makeXML(boxes: boxes)
        XCTAssertTrue(xml.contains("<?xml version=\"1.0\"?>"))
        XCTAssertTrue(xml.contains("<mujoco model=\"duck-depth-obstacles\">"))
        XCTAssertTrue(xml.contains("<worldbody>"))
        XCTAssertTrue(xml.contains("</worldbody>"))
        XCTAssertTrue(xml.contains("</mujoco>"))
        XCTAssertEqual(xml.components(separatedBy: "<geom ").count - 1, boxes.count)
        XCTAssertTrue(xml.contains("name=\"depth_obstacle_0\""))
        XCTAssertTrue(xml.contains("type=\"box\""))
        XCTAssertTrue(xml.contains("contype=\"1\""))
        XCTAssertTrue(xml.contains("conaffinity=\"1\""))
        XCTAssertTrue(xml.contains("size=\"1.0000 2.0000 0.1000\""))
        XCTAssertTrue(xml.contains("pos=\"0.0000 0.0000 -0.4000\""))
    }
}