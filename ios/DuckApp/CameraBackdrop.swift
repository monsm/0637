import AVFoundation
import SwiftUI

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
final class CameraFeed: ObservableObject {
    /// A camera was found and started. False in the simulator, on a device
    /// without a rear camera, or when permission is denied.
    @Published private(set) var available = false
    /// The user denied the camera; do not ask again from this screen.
    @Published private(set) var denied = false

    let session = AVCaptureSession()
    private let queue = DispatchQueue(label: "duck.camera")
    private var configured = false
    private var metadataTimer: Timer?

    func start() {
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

/// A UIView whose layer *is* the capture preview layer.
final class CameraPreviewView: UIView {
    override class var layerClass: AnyClass { AVCaptureVideoPreviewLayer.self }

    var previewLayer: AVCaptureVideoPreviewLayer {
        // `layerClass` guarantees the type.
        layer as! AVCaptureVideoPreviewLayer
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
        // The app is landscape-only; rotate the image to match.
        if let connection = view.previewLayer.connection,
           connection.isVideoRotationAngleSupported(90) {
            connection.videoRotationAngle = 90
        }
        return view
    }

    func updateUIView(_ uiView: CameraPreviewView, context: Context) {
        if uiView.previewLayer.session !== session {
            uiView.previewLayer.session = session
        }
    }
}
