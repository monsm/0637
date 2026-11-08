import Foundation
import MetalKit
import simd
import SwiftUI

private struct Uniforms {
    var mvp: simd_float4x4
    var model: simd_float4x4
    var color: SIMD4<Float>
    var light: SIMD4<Float>
}

private struct Mesh {
    let buffer: MTLBuffer
    let vertices: Int
}

private struct Instance {
    let mesh: Mesh
    let body: Int
    let local: simd_float4x4
    let color: SIMD4<Float>
}

/// Metal renderer: one draw per visual geom, transforms read from MuJoCo
/// bodies each frame. Physics runs at 50 Hz; rendering at the display rate.
final class Renderer: NSObject, MTKViewDelegate {
    private let device: MTLDevice
    private let queue: MTLCommandQueue
    private let controller: DuckController
    private let pipeline: MTLRenderPipelineState
    private let depthState: MTLDepthStencilState
    private var meshes: [String: Mesh] = [:]
    private var instances: [Instance] = []
    private let floor: Mesh
    private var aspect: Float = 1
    private let yaw: Float = .pi
    private let pitch: Float = 0.32
    private let distance: Float = 0.85

    init?(controller: DuckController, view: MTKView) {
        guard let device = MTLCreateSystemDefaultDevice(),
              let queue = device.makeCommandQueue(),
              let library = Renderer.makeLibrary(device),
              let vertexFunction = library.makeFunction(name: "duck_vertex"),
              let fragmentFunction = library.makeFunction(name: "duck_fragment") else {
            return nil
        }
        self.device = device
        self.queue = queue
        self.controller = controller

        let descriptor = MTLRenderPipelineDescriptor()
        descriptor.vertexFunction = vertexFunction
        descriptor.fragmentFunction = fragmentFunction
        descriptor.colorAttachments[0].pixelFormat = .bgra8Unorm
        descriptor.depthAttachmentPixelFormat = .depth32Float
        guard let pipeline = try? device.makeRenderPipelineState(descriptor: descriptor) else {
            return nil
        }
        self.pipeline = pipeline

        let depthDescriptor = MTLDepthStencilDescriptor()
        depthDescriptor.depthCompareFunction = .less
        depthDescriptor.isDepthWriteEnabled = true
        guard let depthState = device.makeDepthStencilState(descriptor: depthDescriptor) else {
            return nil
        }
        self.depthState = depthState
        self.floor = Renderer.makeFloor(device: device)
        super.init()

        view.device = device
        view.colorPixelFormat = .bgra8Unorm
        view.depthStencilPixelFormat = .depth32Float
        view.clearColor = MTLClearColor(red: 0.055, green: 0.075, blue: 0.10, alpha: 1)
        view.preferredFramesPerSecond = 60
        view.delegate = self
        buildInstances()
    }

    // MARK: - Scene building

    private func buildInstances() {
        guard let core = controller.core else { return }
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
            guard let namePointer = duck_core_mesh_name(core, meshID) else { continue }
            let name = String(cString: namePointer)
            guard let mesh = loadMesh(named: name) else { continue }

            let local = translation(SIMD3<Float>(
                Float(position[0]), Float(position[1]), Float(position[2])
            )) * quaternionMatrix(SIMD4<Float>(
                Float(rotation[0]), Float(rotation[1]),
                Float(rotation[2]), Float(rotation[3])
            ))
            let alpha = color[3] > 0 ? color[3] : 1
            instances.append(Instance(
                mesh: mesh,
                body: Int(body),
                local: local,
                color: SIMD4<Float>(color[0], color[1], color[2], alpha)
            ))
        }
        print("renderer: \(instances.count) visual geoms")
    }

    private func loadMesh(named name: String) -> Mesh? {
        if let cached = meshes[name] { return cached }
        let bundle = Bundle.main
        guard let url = bundle.url(
            forResource: name,
            withExtension: "stl",
            subdirectory: "Resources/robot/microduck/assets"
        ) else {
            return nil
        }
        guard let vertices = STLLoader.load(url: url), !vertices.isEmpty else {
            return nil
        }
        guard let buffer = device.makeBuffer(
            bytes: vertices,
            length: vertices.count * MemoryLayout<Float>.stride,
            options: .storageModeShared
        ) else {
            return nil
        }
        let mesh = Mesh(buffer: buffer, vertices: vertices.count / 6)
        meshes[name] = mesh
        return mesh
    }

    /// Compile Shaders.metal at runtime. Shipping the source as a resource
    /// avoids a root-level default.metallib, which Xcode 26 refuses to code
    /// sign ("code object is not signed at all"). Falls back to the compiled
    /// library when the source is absent.
    private static func makeLibrary(_ device: MTLDevice) -> MTLLibrary? {
        let candidates = [
            Bundle.main.url(forResource: "Shaders", withExtension: "metal", subdirectory: "Resources"),
            Bundle.main.url(forResource: "Shaders", withExtension: "metal"),
        ]
        for case let url? in candidates {
            if let source = try? String(contentsOf: url, encoding: .utf8),
               let library = try? device.makeLibrary(source: source, options: nil) {
                return library
            }
        }
        return device.makeDefaultLibrary()
    }

    private static func makeFloor(device: MTLDevice) -> Mesh {
        let extent: Float = 4
        let z: Float = -0.002
        let vertices: [Float] = [
            -extent, -extent, z, 0, 0, 1,
             extent, -extent, z, 0, 0, 1,
             extent,  extent, z, 0, 0, 1,
            -extent, -extent, z, 0, 0, 1,
             extent,  extent, z, 0, 0, 1,
            -extent,  extent, z, 0, 0, 1,
        ]
        let buffer = device.makeBuffer(
            bytes: vertices,
            length: vertices.count * MemoryLayout<Float>.stride,
            options: .storageModeShared
        )!
        return Mesh(buffer: buffer, vertices: vertices.count / 6)
    }

    // MARK: - MTKViewDelegate

    func mtkView(_ view: MTKView, drawableSizeWillChange size: CGSize) {
        aspect = Float(size.width / max(size.height, 1))
    }

    func draw(in view: MTKView) {
        guard let core = controller.core,
              let drawable = view.currentDrawable,
              let descriptor = view.currentRenderPassDescriptor,
              let buffer = queue.makeCommandBuffer(),
              let encoder = buffer.makeRenderCommandEncoder(descriptor: descriptor) else {
            return
        }

        encoder.setRenderPipelineState(pipeline)
        encoder.setDepthStencilState(depthState)
        encoder.setCullMode(.none)

        let focus = controller.trunk
        let eye = focus + SIMD3<Float>(
            distance * cos(pitch) * sin(yaw),
            distance * cos(pitch) * cos(yaw),
            distance * sin(pitch)
        )
        let viewMatrix = lookAt(eye: eye, center: focus, up: SIMD3<Float>(0, 0, 1))
        let projection = perspective(
            fovY: 45 * .pi / 180,
            aspect: aspect,
            near: 0.01,
            far: 100
        )
        let viewProjection = projection * viewMatrix

        draw(mesh: floor, model: matrix_identity_float4x4,
             color: SIMD4<Float>(0.14, 0.15, 0.18, 1),
             viewProjection: viewProjection, encoder: encoder)

        var poses: [Int: (SIMD3<Float>, SIMD4<Float>)] = [:]
        for instance in instances {
            if poses[instance.body] == nil {
                poses[instance.body] = bodyPose(core: core, body: instance.body)
            }
            guard let (position, rotation) = poses[instance.body] else { continue }
            let model = translation(position) * quaternionMatrix(rotation) * instance.local
            draw(mesh: instance.mesh, model: model, color: instance.color,
                 viewProjection: viewProjection, encoder: encoder)
        }

        encoder.endEncoding()
        buffer.present(drawable)
        buffer.commit()
    }

    private func bodyPose(core: OpaquePointer, body: Int) -> (SIMD3<Float>, SIMD4<Float>) {
        var position = [Double](repeating: 0, count: 3)
        var rotation = [Double](repeating: 0, count: 4)
        position.withUnsafeMutableBufferPointer { pos in
            rotation.withUnsafeMutableBufferPointer { quat in
                _ = duck_core_body_pose(core, Int32(body), pos.baseAddress, quat.baseAddress)
            }
        }
        return (
            SIMD3<Float>(Float(position[0]), Float(position[1]), Float(position[2])),
            SIMD4<Float>(Float(rotation[0]), Float(rotation[1]),
                         Float(rotation[2]), Float(rotation[3]))
        )
    }

    private func draw(
        mesh: Mesh,
        model: simd_float4x4,
        color: SIMD4<Float>,
        viewProjection: simd_float4x4,
        encoder: MTLRenderCommandEncoder
    ) {
        var uniforms = Uniforms(
            mvp: viewProjection * model,
            model: model,
            color: color,
            light: SIMD4<Float>(-0.45, -0.65, 1.0, 0)
        )
        encoder.setVertexBuffer(mesh.buffer, offset: 0, index: 0)
        encoder.setVertexBytes(&uniforms, length: MemoryLayout<Uniforms>.stride, index: 1)
        encoder.setFragmentBytes(&uniforms, length: MemoryLayout<Uniforms>.stride, index: 0)
        encoder.drawPrimitives(type: .triangle, vertexStart: 0, vertexCount: mesh.vertices)
    }
}

// MARK: - Math

private func translation(_ value: SIMD3<Float>) -> simd_float4x4 {
    var matrix = matrix_identity_float4x4
    matrix.columns.3 = SIMD4<Float>(value.x, value.y, value.z, 1)
    return matrix
}

private func quaternionMatrix(_ value: SIMD4<Float>) -> simd_float4x4 {
    // MuJoCo quaternions are [w, x, y, z].
    let quaternion = simd_quatf(ix: value.y, iy: value.z, iz: value.w, r: value.x)
    return simd_float4x4(quaternion)
}

private func perspective(fovY: Float, aspect: Float, near: Float, far: Float) -> simd_float4x4 {
    let y = 1 / tan(fovY * 0.5)
    let x = y / max(aspect, 0.001)
    let z = far / (near - far)
    return simd_float4x4(
        SIMD4<Float>(x, 0, 0, 0),
        SIMD4<Float>(0, y, 0, 0),
        SIMD4<Float>(0, 0, z, -1),
        SIMD4<Float>(0, 0, z * near, 0)
    )
}

private func lookAt(eye: SIMD3<Float>, center: SIMD3<Float>, up: SIMD3<Float>) -> simd_float4x4 {
    let forward = simd_normalize(center - eye)
    let side = simd_normalize(simd_cross(forward, up))
    let trueUp = simd_cross(side, forward)
    return simd_float4x4(
        SIMD4<Float>(side.x, trueUp.x, -forward.x, 0),
        SIMD4<Float>(side.y, trueUp.y, -forward.y, 0),
        SIMD4<Float>(side.z, trueUp.z, -forward.z, 0),
        SIMD4<Float>(-simd_dot(side, eye), -simd_dot(trueUp, eye), simd_dot(forward, eye), 1)
    )
}

// MARK: - STL

private enum STLLoader {
    /// Flat array of (px, py, pz, nx, ny, nz) per vertex, three per triangle.
    static func load(url: URL) -> [Float]? {
        guard let data = try? Data(contentsOf: url) else { return nil }
        if data.count >= 84 {
            let triangles = data.withUnsafeBytes { raw -> UInt32 in
                guard let base = raw.baseAddress else { return 0 }
                return base.advanced(by: 80).loadUnaligned(as: UInt32.self)
            }
            let expected = 84 + Int(triangles) * 50
            if triangles > 0, expected == data.count {
                return binary(data: data, triangles: Int(triangles))
            }
        }
        return ascii(data: data)
    }

    private static func binary(data: Data, triangles: Int) -> [Float] {
        var output = [Float]()
        output.reserveCapacity(triangles * 18)
        data.withUnsafeBytes { raw in
            guard let base = raw.baseAddress else { return }
            for triangle in 0..<triangles {
                let start = 84 + triangle * 50 + 12
                var points = [SIMD3<Float>]()
                points.reserveCapacity(3)
                for vertex in 0..<3 {
                    let offset = start + vertex * 12
                    let x = base.advanced(by: offset).loadUnaligned(as: Float.self)
                    let y = base.advanced(by: offset + 4).loadUnaligned(as: Float.self)
                    let z = base.advanced(by: offset + 8).loadUnaligned(as: Float.self)
                    points.append(SIMD3<Float>(x, y, z))
                }
                appendTriangle(points, to: &output)
            }
        }
        return output
    }

    private static func ascii(data: Data) -> [Float]? {
        guard let text = String(data: data, encoding: .utf8) else { return nil }
        var points = [SIMD3<Float>]()
        var output = [Float]()
        for line in text.split(separator: "\n") {
            let parts = line.split(separator: " ")
            guard parts.count >= 4, parts[0] == "vertex" else { continue }
            guard let x = Float(parts[1]), let y = Float(parts[2]), let z = Float(parts[3]) else {
                continue
            }
            points.append(SIMD3<Float>(x, y, z))
            if points.count == 3 {
                appendTriangle(points, to: &output)
                points.removeAll(keepingCapacity: true)
            }
        }
        return output.isEmpty ? nil : output
    }

    private static func appendTriangle(_ points: [SIMD3<Float>], to output: inout [Float]) {
        let cross = simd_cross(points[1] - points[0], points[2] - points[0])
        let length = simd_length(cross)
        let normal = length > 0 ? cross / length : SIMD3<Float>(0, 0, 1)
        for point in points {
            output.append(contentsOf: [point.x, point.y, point.z, normal.x, normal.y, normal.z])
        }
    }
}

// MARK: - SwiftUI bridge

struct MetalView: UIViewRepresentable {
    let controller: DuckController

    func makeUIView(context: Context) -> MTKView {
        let view = MTKView()
        context.coordinator.renderer = Renderer(controller: controller, view: view)
        return view
    }

    func updateUIView(_ uiView: MTKView, context: Context) {}

    func makeCoordinator() -> Coordinator {
        Coordinator()
    }

    final class Coordinator {
        var renderer: Renderer?
    }
}
