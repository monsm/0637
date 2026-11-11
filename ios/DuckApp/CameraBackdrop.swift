import AVFoundation
import SwiftUI
import UIKit

/// The rear camera as the backdrop of Director mode.
///
/// The 3D duck is composited over the live image — the room the duck stands in
/// is the real one. This is a *backdrop*, not a sense: the camera never feeds
/// the 61-D observation, and it is not read per frame by the app. The OS owns
/// the preview layer and its own capture pipeline, so the 50 Hz control loop
/// sees no extra callback on the main thread.
///
/// Only metadata (size, frame rate, exposure, field of view) is posted to
/// `/sensor`, at 1 Hz, so a client can tell what the environment was.
final class CameraFeed: NSObject, ObservableObject {
    /// A camera was found and started. False in the simulator, on a device
    /// without a rear camera, or when permission is denied.
    @Published private(set) var available = false
    /// The user denied the camera; do not ask again from this screen.
    @Published private(set) var denied = false

    let session = AVCaptureSession()
    private let queue = DispatchQueue(label: "duck.camera")
    private var configured = false
    private var metadataTimer: Timer?
    /// Director's still capture. The preview alone has no frames to read, and
    /// asking the OS for a photo is the only path with the full-resolution
    /// image the user saw behind the duck.
    private let photoOutput = AVCapturePhotoOutput()
    private var photoCompletion: ((UIImage?) -> Void)?

    override init() {
        super.init()
    }

    func start() {
        // The still capture needs the device's orientation to rotate the room
        // with the screen; without generating notifications the orientation
        // reads unknown and every landscape photo comes out sideways.
        UIDevice.current.beginGeneratingDeviceOrientationNotifications()
        let status = AVCaptureDevice.authorizationStatus(for: .video)
        switch status {
        case .authorized:
            activate()
        case .notDetermined:
            // Asked only when Director is opened, never at launch.
            AVCaptureDevice.requestAccess(for: .video) { [weak self] granted in
                DispatchQueue.main.async {
                    if granted {
                        self?.activate()
                    } else {
                        self?.denied = true
                    }
                }
            }
        default:
            denied = true
        }
    }

    func stop() {
        metadataTimer?.invalidate()
        metadataTimer = nil
        queue.async { [session] in
            if session.isRunning {
                session.stopRunning()
            }
        }
        UIDevice.current.endGeneratingDeviceOrientationNotifications()
        available = false
    }

    private func activate() {
        queue.async { [weak self] in
            guard let self else { return }
            if !self.configured {
                self.configure()
            }
            guard self.configured else { return }
            if !self.session.isRunning {
                // startRunning blocks for tens of milliseconds and must not
                // run on the main thread: the 20 ms control timer lives there.
                self.session.startRunning()
            }
            DispatchQueue.main.async {
                self.available = true
                self.startMetadata()
            }
        }
    }

    /// Runs on `queue`.
    private func configure() {
        guard let device = AVCaptureDevice.default(
            .builtInWideAngleCamera,
            for: .video,
            position: .back
        ) else {
            NSLog("duck: camera unavailable (no rear camera on this device)")
            return
        }
        guard let input = try? AVCaptureDeviceInput(device: device) else {
            NSLog("duck: camera input refused")
            return
        }
        session.beginConfiguration()
        // 720p is the composite's source, not a recording: higher presets cost
        // power and GPU for pixels the duck covers anyway.
        session.sessionPreset = .hd1280x720
        if session.canAddInput(input) {
            session.addInput(input)
        } else {
            session.commitConfiguration()
            NSLog("duck: camera input could not be added")
            return
        }
        if session.canAddOutput(photoOutput) {
            session.addOutput(photoOutput)
        }
        session.commitConfiguration()
        configured = true
        NSLog(
            "duck: camera ready %dx%d %.0f fps fov %.0f deg",
            Int(device.activeFormat.formatDescription.dimensions.width),
            Int(device.activeFormat.formatDescription.dimensions.height),
            device.activeFormat.videoSupportedFrameRateRanges.first?.maxFrameRate ?? 0,
            device.activeFormat.videoFieldOfView
        )
    }

    /// Ask the camera for one still. Completion always runs on the main
    /// thread; nil means no camera, not yet running, or the capture failed.
    func capture(completion: @escaping (UIImage?) -> Void) {
        let angle: CGFloat
        switch UIDevice.current.orientation {
        case .landscapeLeft: angle = 0
        case .landscapeRight: angle = 180
        case .portraitUpsideDown: angle = 270
        default: angle = 90
        }
        queue.async { [weak self] in
            guard let self, self.configured, self.session.isRunning else {
                DispatchQueue.main.async { completion(nil) }
                return
            }
            if let connection = self.photoOutput.connection(with: .video),
               connection.isVideoRotationAngleSupported(angle) {
                connection.videoRotationAngle = angle
            }
            self.photoCompletion = completion
            let settings = AVCapturePhotoSettings()
            self.photoOutput.capturePhoto(with: settings, delegate: self)
        }
    }

    private func startMetadata() {
        metadataTimer?.invalidate()
        metadataTimer = Timer.scheduledTimer(withTimeInterval: 1.0, repeats: true) { [weak self] _ in
            self?.postMetadata()
        }
    }

    private func postMetadata() {
        guard let device = AVCaptureDevice.default(for: .video) else { return }
        let dimensions = CMVideoFormatDescriptionGetDimensions(
            device.activeFormat.formatDescription
        )
        let body: [String: Any] = [
            "sensor": "camera",
            "mode": "ar",
            "value": [
                "width": Int(dimensions.width),
                "height": Int(dimensions.height),
                "fps": device.activeFormat.videoSupportedFrameRateRanges.first?.maxFrameRate ?? 0,
                "exposure_s": device.exposureDuration.seconds,
                "fov_deg": device.activeFormat.videoFieldOfView,
                "position": "back",
            ],
        ]
        guard let data = try? JSONSerialization.data(withJSONObject: body),
              let url = URL(string: "http://127.0.0.1:8765/sensor")
        else { return }
        var request = URLRequest(url: url)
        request.httpMethod = "POST"
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.httpBody = data
        request.timeoutInterval = 2
        URLSession.shared.dataTask(with: request).resume()
    }
}

extension CameraFeed: AVCapturePhotoCaptureDelegate {
    func photoOutput(
        _ output: AVCapturePhotoOutput,
        didFinishProcessingPhoto photo: AVCapturePhoto,
        error: Error?
    ) {
        let completion = photoCompletion
        photoCompletion = nil
        let image = error == nil
            ? photo.fileDataRepresentation().flatMap(UIImage.init(data:))
            : nil
        DispatchQueue.main.async { completion?(image) }
    }
}

/// A UIView whose layer *is* the capture preview layer.
final class CameraPreviewView: UIView {
    override class var layerClass: AnyClass { AVCaptureVideoPreviewLayer.self }

    var previewLayer: AVCaptureVideoPreviewLayer {
        // `layerClass` guarantees the type.
        layer as! AVCaptureVideoPreviewLayer
    }

    /// The room must stay upright in both orientations. `layoutSubviews`
    /// runs on every bounds/rotation change, which is exactly when the
    /// interface orientation (and therefore the needed angle) moves.
    override func layoutSubviews() {
        super.layoutSubviews()
        updateRotation()
    }

    func updateRotation() {
        guard let connection = previewLayer.connection else { return }
        let angle: CGFloat
        switch window?.windowScene?.interfaceOrientation {
        case .portrait: angle = 90
        case .portraitUpsideDown: angle = 270
        case .landscapeLeft: angle = 0
        case .landscapeRight: angle = 180
        default: angle = 90
        }
        if connection.isVideoRotationAngleSupported(angle) {
            connection.videoRotationAngle = angle
        }
    }
}

/// The SwiftUI wrapper. It takes no touches: the duck's gestures are above it.
struct CameraBackdrop: UIViewRepresentable {
    let session: AVCaptureSession

    func makeUIView(context: Context) -> CameraPreviewView {
        let view = CameraPreviewView()
        view.isUserInteractionEnabled = false
        view.backgroundColor = .black
        view.previewLayer.session = session
        view.previewLayer.videoGravity = .resizeAspectFill
        // The angle itself depends on the interface orientation and is set in
        // `layoutSubviews`, which runs as soon as the view has a window.
        view.updateRotation()
        return view
    }

    func updateUIView(_ uiView: CameraPreviewView, context: Context) {
        if uiView.previewLayer.session !== session {
            uiView.previewLayer.session = session
        }
    }
}
