import Foundation
import MetalKit
import UIKit
import simd
import SwiftUI

private struct Uniforms {
    var mvp: simd_float4x4
    var model: simd_float4x4
    var color: SIMD4<Float>
    var light: SIMD4<Float>
    /// x: metalness, y: roughness, z: exposure.
    var material: SIMD4<Float>
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

/// Why the renderer could not start. Every one of these used to end in a
/// silent `nil`: the view was created, no delegate was attached, and the
/// screen stayed black with nothing in the log. The app now reports the
/// reason on screen instead.
enum RendererError: Error, CustomStringConvertible {
    case noDevice
    case noQueue
    case noLibrary(String)
    case noFunction(String)
    case noPipeline
    case noDepthState

    var description: String {
        switch self {
        case .noDevice:
            return "Metal device unavailable"
        case .noQueue:
            return "Metal command queue unavailable"
        case .noLibrary(let detail):
            return "Metal library unavailable: \(detail)"
        case .noFunction(let name):
            return "Metal function '\(name)' missing"
        case .noPipeline:
            return "Metal pipeline creation failed"
        case .noDepthState:
            return "Metal depth stencil state failed"
        }
    }
}

/// Metal renderer: one draw per visual geom, transforms read from MuJoCo
/// bodies each frame. Physics runs at 50 Hz; rendering at the display rate.
final class Renderer: NSObject, MTKViewDelegate {
    private let device: MTLDevice
    private let queue: MTLCommandQueue
    private let controller: DuckController
    private let pipeline: MTLRenderPipelineState
    private let depthState: MTLDepthStencilState
    private var meshes: [Int32: Mesh] = [:]
    private var instances: [Instance] = []
    private let floor: Mesh
    private var aspect: Float = 1
    /// The body model this renderer's meshes and instances were built for.
    /// A mode switch loads a different MJCF, so everything cached has to go.
    private var builtGeneration: UInt64 = 0
    /// After a rebuild, ask the core for meshes: the controller's off-thread
    /// parse describes the model the app started with.
    private var meshesFromCore = false
    private let yaw: Float = .pi
    private let pitch: Float = 0.32
    private let distance: Float = 0.85

    /// Why the last `init?` failed. A renderer that could not start used to
    /// leave a black screen with nothing to look at.
    fileprivate static var lastError: String?

    init?(controller: DuckController, view: MTKView) {
        Renderer.lastError = nil
        guard let device = MTLCreateSystemDefaultDevice() else {
            Renderer.lastError = RendererError.noDevice.description
            return nil
        }
        guard let queue = device.makeCommandQueue() else {
            Renderer.lastError = RendererError.noQueue.description
            return nil
        }
        let library: MTLLibrary
        do {
            library = try Renderer.makeLibrary(device)
        } catch {
            Renderer.lastError = (error as? RendererError)?.description ?? "\(error)"
            return nil
        }
        guard let vertexFunction = library.makeFunction(name: "duck_vertex") else {
            Renderer.lastError = RendererError.noFunction("duck_vertex").description
            return nil
        }
        guard let fragmentFunction = library.makeFunction(name: "duck_fragment") else {
            Renderer.lastError = RendererError.noFunction("duck_fragment").description
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
            Renderer.lastError = RendererError.noPipeline.description
            return nil
        }
        self.pipeline = pipeline

        let depthDescriptor = MTLDepthStencilDescriptor()
        depthDescriptor.depthCompareFunction = .less
        depthDescriptor.isDepthWriteEnabled = true
        guard let depthState = device.makeDepthStencilState(descriptor: depthDescriptor) else {
            Renderer.lastError = RendererError.noDepthState.description
            return nil
        }
        self.depthState = depthState
        self.floor = Renderer.makeFloor(device: device)
        super.init()

        view.device = device
        view.colorPixelFormat = .bgra8Unorm
        view.depthStencilPixelFormat = .depth32Float
        view.clearColor = MTLClearColor(red: 0.055, green: 0.075, blue: 0.10, alpha: 1)
        // **Clear, not the studio colour.** The view must be able to composite
        // over the camera in Director mode; an opaque background colour would
        // paint the studio over the room even with a transparent clear colour.
        // The studio look comes from `clearColor`, which is drawn every frame.
        view.backgroundColor = .clear
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
            guard let mesh = loadMesh(meshID: meshID) else { continue }

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
        // NSLog, not print: the CI session reads this from the unified log,
        // and print() to a pipe can sit in a buffer until the app exits.
        NSLog("duck: renderer: %d visual geoms", instances.count)
    }

    private func loadMesh(meshID: Int32) -> Mesh? {
        if let cached = meshes[meshID] { return cached }
        // The controller builds every soup off the main thread while the core
        // is created; fall back to the core if one is missing.
        if !meshesFromCore,
           let vertices = controller.meshVertices[meshID], !vertices.isEmpty {
            return storeMesh(id: meshID, vertices: vertices)
        }
        guard let core = controller.core else { return nil }
        let floats = Int(duck_core_mesh_soup(core, meshID, nil, 0))
        guard floats > 0 else { return nil }
        var soup = [Float](repeating: 0, count: floats)
        let written = soup.withUnsafeMutableBufferPointer { buffer in
            duck_core_mesh_soup(core, meshID, buffer.baseAddress, Int32(buffer.count))
        }
        guard Int(written) == floats else { return nil }
        return storeMesh(id: meshID, vertices: soup)
    }

    private func storeMesh(id: Int32, vertices: [Float]) -> Mesh? {
        guard let buffer = device.makeBuffer(
            bytes: vertices,
            length: vertices.count * MemoryLayout<Float>.stride,
            options: .storageModeShared
        ) else {
            return nil
        }
        let mesh = Mesh(buffer: buffer, vertices: vertices.count / 6)
        meshes[id] = mesh
        return mesh
    }

    /// Compile Shaders.metal at runtime. Shipping the source as a resource
    /// avoids a root-level default.metallib, which Xcode 26 refuses to code
    /// sign ("code object is not signed at all"). Falls back to the compiled
    /// library when the source is absent.
    private static func makeLibrary(_ device: MTLDevice) throws -> MTLLibrary {
        let candidates = [
            Bundle.main.url(forResource: "Shaders", withExtension: "metal", subdirectory: "DuckResources"),
            Bundle.main.url(forResource: "Shaders", withExtension: "metal"),
        ]
        var sourceProblem = "Shaders.metal not found in the bundle"
        for case let url? in candidates {
            guard let source = try? String(contentsOf: url, encoding: .utf8) else {
                sourceProblem = "\(url.lastPathComponent) is not readable"
                continue
            }
            do {
                return try device.makeLibrary(source: source, options: nil)
            } catch {
                sourceProblem = "Shaders.metal failed to compile: \(error.localizedDescription)"
            }
        }
        // The compiled library is the fallback. `makeDefaultLibrary()` has
        // changed shape across SDKs (optional or not), so widen it first.
        let fallback: MTLLibrary? = device.makeDefaultLibrary()
        if let library = fallback {
            return library
        }
        throw RendererError.noLibrary(sourceProblem)
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
        controller.noteFrame()
        // Director mode composites the duck over the live camera. The clear
        // colour must be transparent *and* black: CAMetalLayer's contents are
        // premultiplied, so a dark-blue clear would veil the image.
        let camera = controller.cameraActive
        view.clearColor = camera
            ? MTLClearColor(red: 0, green: 0, blue: 0, alpha: 0)
            : MTLClearColor(red: 0.055, green: 0.075, blue: 0.10, alpha: 1)
        guard let core = controller.core,
              let drawable = view.currentDrawable,
              let descriptor = view.currentRenderPassDescriptor,
              let buffer = queue.makeCommandBuffer(),
              let encoder = buffer.makeRenderCommandEncoder(descriptor: descriptor) else {
            return
        }

        // A roller duck has different feet and meshes. Rebuild before drawing
        // a frame against the old model.
        let generation = duck_core_model_generation(core)
        if generation != builtGeneration {
            // A different model: nothing cached describes it. (The first
            // build happened in `init` at generation 0, so any difference
            // here is a mode switch.)
            instances.removeAll()
            meshes.removeAll()
            meshesFromCore = true
            buildInstances()
            builtGeneration = generation
        }

        encoder.setRenderPipelineState(pipeline)
        encoder.setDepthStencilState(depthState)
        encoder.setCullMode(.none)

        // The camera follows the interpolated trunk, not the 10 Hz status
        // sample: a camera locked to a slower clock than the duck makes the
        // whole room judder even when the duck itself is smooth.
        let alpha = controller.interpolationAlpha()
        let trunkPose = controller.interpolatedPose(body: 1, alpha: alpha)
        let focus = trunkPose?.position ?? controller.trunk
        // Portrait narrows the horizontal field to a third of landscape's.
        // Backing off slightly keeps a side-stepping or carried duck in
        // frame without shrinking it vertically (fovY is unchanged).
        let eyeDistance = aspect < 1 ? distance * 1.15 : distance
        let eye = focus + SIMD3<Float>(
            eyeDistance * cos(pitch) * sin(yaw),
            eyeDistance * cos(pitch) * cos(yaw),
            eyeDistance * sin(pitch)
        )
        let viewMatrix = lookAt(eye: eye, center: focus, up: SIMD3<Float>(0, 0, 1))
        let projection = perspective(
            fovY: 45 * .pi / 180,
            aspect: aspect,
            near: 0.01,
            far: 100
        )
        let viewProjection = projection * viewMatrix

        // A headlight, like MuJoCo's own viewer: the light travels along the
        // camera's forward axis, so whatever the camera sees is lit. A fixed
        // world light left the front of the duck almost black.
        let forward = simd_normalize(focus - eye)

        // The studio floor is the one thing that must not be drawn over a
        // camera image: a 4 m opaque plane at z=-0.002 hides the room and the
        // duck's feet. Physics ground stays at z=0 either way.
        if !camera {
            draw(mesh: floor, model: matrix_identity_float4x4,
                 color: SIMD4<Float>(0.14, 0.15, 0.18, 1),
                 light: forward, checker: true, viewProjection: viewProjection, encoder: encoder)
        }

        var poses: [Int: (SIMD3<Float>, SIMD4<Float>)] = [:]
        for instance in instances {
            if poses[instance.body] == nil {
                if let sample = controller.interpolatedPose(body: instance.body, alpha: alpha) {
                    poses[instance.body] = (sample.position, sample.rotation)
                } else {
                    poses[instance.body] = bodyPose(core: core, body: instance.body)
                }
            }
            guard let (position, rotation) = poses[instance.body] else { continue }
            let model = translation(position) * quaternionMatrix(rotation) * instance.local
            draw(mesh: instance.mesh, model: model, color: instance.color,
                 light: forward, checker: false, viewProjection: viewProjection, encoder: encoder)
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
        light: SIMD3<Float>,
        checker: Bool,
        viewProjection: simd_float4x4,
        encoder: MTLRenderCommandEncoder
    ) {
        var uniforms = Uniforms(
            mvp: viewProjection * model,
            model: model,
            color: color,
            // The shader negates this and dots it with the normal, so the
            // vector points the way the light travels; w carries the floor's
            // checker flag.
            light: SIMD4<Float>(light.x, light.y, light.z, checker ? 1 : 0),
            // The duck is painted plastic over a few metal parts; the floor is
            // matte. Roughness is what keeps the shells from looking like chrome.
            material: checker
                ? SIMD4<Float>(0.0, 0.85, 1.0, 0)
                : SIMD4<Float>(0.05, 0.42, 1.0, 0)
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

// MARK: - SwiftUI bridge

struct MetalView: UIViewRepresentable {
    let controller: DuckController

    func makeUIView(context: Context) -> MTKView {
        let view = MTKView()
        view.backgroundColor = UIColor(red: 0.055, green: 0.075, blue: 0.10, alpha: 1)
        // Able to composite over a camera preview: an opaque layer would
        // paint the studio colour over the room in Director mode.
        view.isOpaque = false
        view.layer.isOpaque = false
        // A UIView inside SwiftUI keeps its own hit-testing; without this the
        // MTKView swallows the SwiftUI gestures layered over it.
        view.isUserInteractionEnabled = false
        context.coordinator.renderer = Renderer(controller: controller, view: view)
        if context.coordinator.renderer == nil {
            // A renderer that failed to start used to leave a black screen
            // with nothing to look at. Report it instead.
            let message = Renderer.lastError ?? "the renderer could not start"
            NSLog("duck: renderer failed: %@", message)
            DispatchQueue.main.async {
                controller.renderError = message
            }
        }
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
