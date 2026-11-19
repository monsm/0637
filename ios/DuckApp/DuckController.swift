import Foundation
import SwiftUI
import Combine
import AVFoundation
import CoreMotion
import simd

/// The walk slot's shell-side memory.
///
/// The core's `POST /config {"walk_policy": name}` is a process-global
/// override: a relaunch starts from the set's own slot again. Official
/// `robotd` writes the name into `robotd.toml` and resolves it at boot
/// (`upstream/microduck/robotd/src/main.rs:5276-5292`); a phone has no
/// config file, so the shell keeps it in UserDefaults and hands the same
/// string back through `/config` once the core exists. The value is
/// replayed verbatim — `"none"` included — because what a name means, and
/// whether it is legal at all, is the core's ruling and not the shell's.
struct WalkPolicyStore {
    /// Beside the rules/quests keys, and deliberately not a Copy key:
    /// nothing visible is named by it.
    static let defaultsKey = "duck_walk_policy_v1"
    private let defaults: UserDefaults

    init(defaults: UserDefaults = .standard) {
        self.defaults = defaults
    }

    /// The last name handed over, exactly as it was written. Empty or
    /// blank is not a name: Android's store treats it as "forget", and an
    /// empty `/config` value is the core's clear request, so there is
    /// nothing to replay.
    var name: String? {
        guard let stored = defaults.string(forKey: Self.defaultsKey),
              !stored.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
        else { return nil }
        return stored
    }

    /// Remember a name for the next launch. Blank forgets: there is no
    /// file to name. `"none"` is not blank — it is stored and replayed
    /// verbatim, and the core rules on the sentinel.
    func remember(_ name: String) {
        if name.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            defaults.removeObject(forKey: Self.defaultsKey)
        } else {
            defaults.set(name, forKey: Self.defaultsKey)
        }
    }

    func forget() {
        defaults.removeObject(forKey: Self.defaultsKey)
    }

    /// Hand the stored name to the core without editing it. The closure
    /// answers whether the core took it; false when nothing usable was
    /// stored, so the caller can tell "no request" from "refused". Pure
    /// apart from the injected store, so a test can record the exact
    /// replayed string.
    @discardableResult
    func replay(apply: (String) -> Bool) -> Bool {
        guard let stored = name else { return false }
        return apply(stored)
    }
}

/// Owns the Rust core and ticks it at 50 Hz on the main run loop.
///
/// The renderer and the core share the main thread, so Metal never reads
/// MuJoCo state while it is being written.
final class DuckController: ObservableObject {
    /// Dance Studio is owned by the controller, so every action button can
    /// record what it just did without a second path through the core.
    let dance = DanceStudio()
    /// The morning postcard shelf.
    let postcards = PostcardShelf()

    /// 成长系统的散步计：life 里的 metres 是核心累计值，这里只记**本次观察到的**
    /// 增量，并按「连续两个 poll 不再前进」切成一次散步——「一次散步 30 米」
    /// 的成就才有语义。首次读到只立基线，不记数。
    private var walkBaselineMetres: Double?
    private var walkSessionMetres = 0
    /// 翻滚同理：只记第一次读到之后的新增，首读立基线。
    private var lastRecordedRolls: Int?
    /// The live renderer, for the Director shutter's frame read-back. Weak:
    /// the view owns the renderer, not the controller.
    weak var renderer: Renderer?

    /// A finished Director photo, waiting for the preview sheet. The file is
    /// already on disk; the sheet decides save-to-Photos or share.
    struct CapturedPhoto: Identifiable {
        let id = UUID()
        let image: UIImage
        let url: URL?
    }
    @Published var capturedPhoto: CapturedPhoto?

    /// The child's block program. The core executes it; the shell edits it and
    /// keeps a copy in UserDefaults so it survives a launch.
    struct DuckRule: Codable, Equatable {
        var trigger: String
        var action: String
        var value: String
        /// Optional ordered follow-ups: a chain, for a longer little program.
        var then: [DuckRuleStep] = []
        /// A timer rule's period in whole simulated seconds. Separate from
        /// the action's word: the period is the WHEN.
        var periodS: Int?

        enum CodingKeys: String, CodingKey { case trigger, action, value, then, periodS = "period_s" }

        init(
            trigger: String,
            action: String,
            value: String,
            then: [DuckRuleStep] = [],
            periodS: Int? = nil
        ) {
            self.trigger = trigger
            self.action = action
            self.value = value
            self.then = then
            self.periodS = periodS
        }

        // Programs saved before chains existed have no `then` key; they must
        // still load rather than being dropped as corrupt. The same holds
        // for `period_s`: a program saved before timer rules carried no
        // period. The synthesized encoder writes `period_s`; without this
        // decode the value was silently lost on every round trip.
        init(from decoder: Decoder) throws {
            let container = try decoder.container(keyedBy: CodingKeys.self)
            trigger = try container.decode(String.self, forKey: .trigger)
            action = try container.decode(String.self, forKey: .action)
            value = try container.decode(String.self, forKey: .value)
            then = try container.decodeIfPresent(
                [DuckRuleStep].self,
                forKey: .then
            ) ?? []
            periodS = try container.decodeIfPresent(Int.self, forKey: .periodS)
        }
    }
    struct DuckRuleStep: Codable, Equatable {
        var action: String
        var value: String
    }
    @Published var rules: [DuckRule] = []
    /// Why the core refused the last program, if it did. Empty otherwise.
    @Published var rulesError = ""
    /// Today's three small goals. The core measures them from its own vet
    /// book; the shell only shows the board and hands it back across launches.
    struct QuestCard: Identifiable {
        let id: String
        let title: String
        let progress: Double
        let done: Bool
    }
    @Published var quests: [QuestCard] = []
    @Published var questStreak = 0
    @Published var questDay = ""
    /// The calendar day the core was last told about, so a midnight that
    /// happens while the app is open still rolls the board.
    private var lastQuestDayPosted = ""
    /// The local first-seen mood dex behind DexSheet. The collection page
    /// now reads the core's own `/collection`; this stays for the legacy
    /// page until the team retires it.
    struct DexEntry: Identifiable {
        let id: String
        let count: Int
        let firstSeen: String?
        let isEmotion: Bool
    }
    @Published var dex: [DexEntry] = []
    private static let dexKey = "duck_dex_first_v1"
    private static let rulesKey = "duck_rules_v1"
    private static let questKey = "duck_quests_v1"
    /// The core's own trigger vocabulary (`core/duck-mobile-core/src/rules.rs:16`).
    /// "pet" is the 摸摸 trigger; "timer" is the clock the editor offers too.
    static let ruleTriggers: Set<String> = ["clap", "whistle", "pet", "timer"]
    /// The editor's three actions. The core also accepts "light"
    /// (`rules.rs:19`), but neither phone's editor offers it, so a shared
    /// program stays inside the vocabulary both editors can show.
    static let ruleActions: Set<String> = ["skill", "emotion", "sound"]
    /// A timer's period in whole simulated seconds. The core refuses a timer
    /// outside this window (`rules.rs:72-73,113-120`); the shell keeps the
    /// same bounds so a pasted program is never one the core rejects whole.
    static let rulePeriodBounds = 5...3600

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
    /// A hand is holding the duck: drags move the body, not the gait.
    @Published var carrying = false
    /// The pickup classifier says a hand has it.
    @Published var pickedUp = false
    /// The orientation filter has converged; before that the duck holds still.
    @Published var imuReady = false
    @Published var mouth: Double = 0
    /// The scene has a mouth joint. The official walking scene does not, so
    /// the sheet does not claim a beak the duck cannot move.
    @Published var hasMouth = false
    /// Main-thread only: the lower beak's opening, 0..1, for the renderer.
    /// The shared MJCF has no jaw joint, so MuJoCo cannot move the mouth even
    /// though the official robot's mouth is a servo. The shell animates the
    /// beak itself while a sound is sounding — visual only; the 61-D
    /// observation and the safety layer never see it.
    private(set) var visualMouth: Double = 0
    /// "walk" or "roller", from /state.
    @Published var driveMode = "walk"
    /// The last skill the ring actually started. `duck_core_trigger_skill`
    /// returns 1 when the core accepted it; the moving label is overwritten
    /// by the next 20 ms tick, so the shell keeps this durable answer.
    @Published var lastSkill = ""
    @Published var volts: Double = 0
    /// The platform's own measurements, once a second.
    @Published var fps: Double = 0
    @Published var ticksPerSecond: Double = 0
    /// The voice's own state, published only when `-duck.voiceDiagnostics`
    /// asks for it: category and options, engine/player, and how many sounds
    /// really reached the audio engine. The regression test reads this; the
    /// product UI never shows it.
    @Published var voiceLine = ""
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
    /// The token a tool on another machine has to send to the duck's
    /// API. The apps never need it — they reach the duck over the
    /// loopback address — but a parent pointing an AI client at this
    /// phone does.
    @Published var intentToken = ""
    /// The latest phone-sensor readings, already formatted for display.
    /// Denied permissions replace the summary (see `pollSensors`); depth keeps
    /// "—" on devices with no LiDAR (see `hasLiDAR`).
    @Published var sensorMode = "twin"
    @Published var cameraSummary = "—"
    @Published var microphoneSummary = "—"
    /// What the duck is feeling and how awake it looks, read from /state.
    @Published var behaviorSummary = "—"
    /// The virtual face LEDs, from the same /state fetch: the verdict robotd's
    /// status LED would show, the four-light battery gauge and the flashlight.
    /// A phone has no I²C bus, so the semantics are the face.
    @Published var faceStatus = "Starting"
    @Published var faceGauge = "Unknown"
    @Published var faceBattery = "—"
    @Published var faceBars = 0
    /// While-you-were-away: the events the duck logged since the app last
    /// went to the background, summed up in one line when it comes back.
    /// A duck that played by itself while nobody watched should say so.
    @Published var awaySummary: String?
    /// The last room sound the duck answered, as one line above the rail.
    @Published var roomSound: String?
    private var lastSoundEventTick = -1
    /// The last programmed block that fired, as "clap → roulade".
    @Published var ruleNotice: String?
    private var lastRuleTick = -1
    /// Other ducks on this Wi-Fi, and the last hello exchanged.
    struct PeerInfo: Identifiable {
        let id: String
        let name: String
        let replied: Bool
        let paired: Bool
        /// "ip:port" as the LAN announced it - the pad sender dials its ip.
        let addr: String
    }
    /// The UI tests' fixture: `-duck.peerFixture YES` pins one paired
    /// neighbour so the peers sheet's paired-peer rows render on a machine
    /// with no LAN duck. Presentation only — the address is TEST-NET, the
    /// pad dials nothing that exists, and no gift is ever recorded.
    private static let peerFixtureOn =
        UserDefaults.standard.string(forKey: "duck.peerFixture") == "YES"
    /// Same family, for the day rows: `-duck.dayFixture YES` pins two sample
    /// rows so the sheet's view is testable without a recorded history.
    private static let dayFixtureOn =
        UserDefaults.standard.string(forKey: "duck.dayFixture") == "YES"
    /// The fixture peer exists from the first frame — the state parse keeps
    /// it pinned every tick, but the sheet must not race the first poll.
    @Published var peers: [PeerInfo] = DuckController.peerFixtureOn
        ? [PeerInfo(id: "fixture", name: "Sunny", replied: false, paired: true, addr: "192.0.2.10:8765")]
        : []
    /// The official UDP pad's gate, read back from /state.pad — the core is
    /// the authority, not the switch the finger just flipped.
    @Published var netPadOn = false
    @Published var peerName = ""
    @Published var peerNotice: String?
    private var lastPeerTick = -1
    /// The latest gift a neighbour sent, as one line over the stage. The
    /// core's tick dedupes repeated publishes; giftTick is a fresh counter
    /// so even the first event (at wire tick 0) fires the pill's onChange.
    @Published var giftTick = 0
    @Published var giftLine: String?
    private var lastGiftTick = -1
    /// The LAN chorus, from /state.chorus: whether it is playing, whether this
    /// duck leads or joined, the moods it will perform and the honest room
    /// size at its start. `chorusTick` is a fresh counter raised on the
    /// inactive -> active edge, so the pill fires once per song rather than
    /// on every 1 Hz publish.
    @Published var chorusActive = false
    @Published var chorusRole = ""
    @Published var chorusMoves: [String] = []
    @Published var chorusPeers = 0
    @Published var chorusTick = 0
    private var chorusWasActive = false
    /// The user's own model. The shell makes the call; the core never talks
    /// to the internet. Config lives in UserDefaults beside the language.
    @Published var aiConfig = AIConfig.load()
    @Published var aiStatus = ""
    @Published var aiDecision = ""
    @Published var aiError: String?
    private let aiBrain = AIBrain()
    private var lastAIThink = Date.distantPast
    private var aiRequestInFlight = false
    /// Bumped by every decision so an old move chain cannot keep steering
    /// after a stop, a look or a newer move.
    private var moveGeneration = 0
    /// One shutter press, one still: a second tap while the camera is still
    /// exposing is ignored rather than overwriting the pending completion.
    private var photoInFlight = false
    private var eventsAtBackground: [String] = []
    private var lastEvents: [String] = []
    /// The companion layer, from /state.care: hunger (0 fed .. 1 starving),
    /// mood (-1 sad .. 1 happy), bond (0..1) and whether it has dozed off.
    /// These are the duck's, not the phone's; the shell only shows them and
    /// hands them back across launches.
    @Published var careHunger: Double = 0
    @Published var careMood: Double = 0
    @Published var careBond: Double = 0
    @Published var careAsleep = false
    /// The core's own heart reading (care.heart_bpm): the same numbers the
    /// three capsules above show, as one bpm for the palm.
    @Published var heartBpm: Double = 0
    /// The parent's daily allowance in minutes, nil when unlimited — the
    /// core's own accounting of `powered_s`, read back on every poll.
    /// The compass: the duck's home keeps one real-world bearing, so the
    /// room turns UNDER the camera when the person turns. Off by default
    /// (a rotating room can confuse a driving child); unavailable when the
    /// device has no attitude - the simulator has none, and the row hides.
    @Published var compassOn = false {
        didSet {
            if compassOn != oldValue {
                UserDefaults.standard.set(compassOn, forKey: "duck_compass_on")
                if !compassOn { compassYaw = 0 }
            }
        }
    }
    /// The heading delta from the bearing the compass started with, in
    /// radians, already negated for the camera: turning right turns the
    /// room left. Written on the main thread by the motion handler.
    @Published var compassYaw: Float = 0
    private var compassBearing: Double?
    /// The room's lamp: a render-layer multiplier on the daylight exposure
    /// (0.55 dark … 1.15 bright). iOS has no ambient-light API, so this is
    /// the person's own hand on the lamp — the honest label says so, and
    /// nothing here reaches the duck's observation.
    @Published var ambientScale: Double = 1.0
    @Published var parentMinutes: Int?
    @Published var parentLeftS: Double = 0
    private var companionRestored = false
    private var companionRestoreStarted = false
    private var lastCompanionSave = Date.distantPast
    /// Sleep mode: the duck put to bed on purpose, read from /state.night.
    /// The night is the same doze neglect reaches — `care.asleep` gates the
    /// ordinary wake rules — but chosen, and the quiet number is the core's.
    /// It is simulated seconds at the care's own scale: `(tick -
    /// started_tick) * 0.02`, never wall time, so a paused or slowed duck
    /// ages its night at the pace its care does.
    @Published var nightActive = false
    @Published var nightQuietSeconds: Double = 0
    /// Raised on the active → inactive edge, and only when a real stretch of
    /// quiet lay behind it, so the morning pill has something to say. A
    /// launch into an already-inactive night is silent.
    @Published var nightWakeTick = 0
    /// The whole quiet minutes at the moment the night ended, for the
    /// morning line; the wire's inactive quiet_s reads 0, so this is held
    /// from the last active publish instead of trusting the transition poll.
    @Published var nightWakeMinutes = 0
    private var nightWasActive = false
    private var lastNightQuietSeconds: Double = 0
    /// The last /state slices the 1 Hz poll read. A synchronous copy at the
    /// door cannot be killed with the process like an in-flight URLSession.
    private var latestCare: [String: Any]?
    private var latestLife: [String: Any]?
    /// Path Painter: the route the child drew on the stage, as the core
    /// reports it. The core owns the walk; reached/total only drive the
    /// step chip, and an inactive route always publishes zeros.
    @Published var pathActive = false
    @Published var pathReached = 0
    @Published var pathTotal = 0
    /// Increments each time a painted walk runs its whole course: the duck's
    /// own excited mood is the celebration, the pill names it. A Stop press
    /// is not a finish and does not tick.
    @Published var pathDoneTick = 0
    private var pathWasActive = false
    private var pathCancelRequested = false

    /// One line for the Stage Deck's mood pill, built from the core's care
    /// numbers rather than from what the shell imagines.
    var careLine: String? {
        guard ready else { return nil }
        return Copy.careLine(
            hunger: careHunger,
            mood: careMood,
            bond: careBond,
            asleep: careAsleep
        )
    }

    /// Tilt-to-look: the phone *is* the head. Off by default; the menu asks
    /// for it. While on and nobody is steering, the phone's attitude becomes
    /// the head command, the way the physical pad's IMU steers the real duck.
    /// Published rather than @AppStorage so the menu re-renders when it
    /// flips; the flag itself is persisted beside the language preference.
    @Published var tiltLook: Bool = UserDefaults.standard.bool(forKey: "tilt_look") {
        didSet { UserDefaults.standard.set(tiltLook, forKey: "tilt_look") }
    }
    /// The trick book: how many of the duck's own moves and emotions it has
    /// actually performed, from /state.life. A collection you fill by playing,
    /// not a list of names.
    @Published var trickBook = "—"
    /// The three lifetime numbers the vet book is about, read from /state.life
    /// and shown next to the export button.
    @Published var vetbookMetres: Double = 0
    @Published var vetbookRolls: Int = 0
    @Published var vetbookPickups: Int = 0
    /// The flashlight's colour while lit, nil when off. A toggle publishes the
    /// FFI's new state directly rather than waiting for the next poll.
    @Published var flashlightColor: String?
    @Published var depthSummary = "—"
    @Published var imuSummary = "—"
    /// What the sheet's microphone row shows. Denial is a real state, not an
    /// absent reading, so it takes precedence over the polled summary.
    @Published var micSummary = "—"
    /// Persisted so the room mic comes back on the next launch without a new
    /// permission prompt. Default off: nothing listens until the user asks.
    @AppStorage("mic_enabled") var micEnabled = false
    /// The duck's ear: spoken Chinese in, the duck's own verbs out. Persisted
    /// like the room mic, and off by default for the same reason.
    @AppStorage("voice_commands_enabled") var voiceCommandsEnabled = false
    /// The last thing the duck heard, in the user's own words. The words are
    /// the duck's input; the core's word table turns them into verbs.
    @Published var heardText = ""
    /// What the duck's ear is doing: off, listening, denied or unavailable.
    /// A permission answer and a missing recogniser are real states, not an
    /// absent reading, so the sheet says which one it is.
    @Published private(set) var speechState: SpeechState = .off

    private(set) var core: OpaquePointer?
    private var timer: Timer?
    private var lastStatus = Date.distantPast
    /// Performance counters, reset once a second by the sensor timer: frames
    /// the renderer drew and 50 Hz ticks actually achieved. Written back to
    /// the core so /state reports what the app measured, not a guess.
    private var frameCount = 0
    private var tickCount = 0
    private var perfAnchor = Date()
    private var policyDirectory = ""
    /// The walk slot as the shell remembers it across launches. The core's
    /// override is process memory; the startup restore hands the stored
    /// name back through `/config` once the intent server is up.
    private let walkPolicies = WalkPolicyStore()
    /// The words a person taught the duck (G2). Same shape as the walk
    /// slot: `/config` is process memory, so the phone keeps the table and
    /// replays it once after the core is up. Deliberately not published:
    /// the sheet owns its draft, and the controller's published surface is
    /// a ratchet.
    private let voiceAliases = VoiceAliasStore()
    private let motion = CMMotionManager()
    private var lastPush = Date.distantPast
    /// The duck's own voice: the core decides when, this owns the speaker.
    let voice = Voice()
    /// The system voice speaking for the duck (diary reads): possession-mode,
    /// honestly labelled. It renders through Voice's session without owning
    /// it; the tick's mouth target follows it like a quack.
    private let speaker = Speaker()
    /// The palm's heartbeat: the core's care numbers as a haptic rhythm.
    /// Reading, not event — nothing here writes back to the duck.
    let heart = Heart()
    /// The drawer row's switch. Persisted like the mic toggle; off means the
    /// duck's heart is silent in the palm, not that the duck has none.
    @Published var heartOn = UserDefaults.standard.bool(forKey: "duck_heart_on") {
        didSet {
            if heartOn != oldValue {
                UserDefaults.standard.set(heartOn, forKey: "duck_heart_on")
                heart.setEnabled(heartOn)
            }
        }
    }
    /// Observed by the diary's button so the icon flips the moment speech
    /// starts and ends. Written only on the main thread.
    @Published private(set) var ttsSpeaking = false
    /// Main-thread only: paces the voice diagnostic line, not the control loop.
    private var diagnosticFrames = 0
    /// Director mode: the rear camera behind the duck. The renderer goes
    /// transparent and drops its floor, and the room is the real one.
    let camera = CameraFeed()
    /// Read by the renderer each frame; true only when Director is open *and*
    /// a camera actually started (the simulator has none).
    @Published var cameraActive = false
    /// The instrument sheet is up: the renderer swaps the warm room for the
    /// cold checkerboard lab (STAGE_LOOK §3). Presentation only; the duck's
    /// observation and safety layer never see it.
    @Published var instrumentOpen = false
    @Published private(set) var cameraDenied = false
    private var wantsCamera = false
    private var cameraCancellable: AnyCancellable?
    private var cameraDeniedCancellable: AnyCancellable?

    /// The one-shot LiDAR scan that has to finish before `duck_core_new`:
    /// its `obstacles.xml` is compiled with the scene, and its floor height
    /// is the `/floor` intent the core receives once its server is up.
    private let depthScanner = DepthScanner()
    /// Set before the core is built and never changes afterwards; a plain
    /// value is enough because the sensors card opens only after the core is
    /// ready. Keeping it a plain value also keeps the UI debt ratchet's
    /// published-property count from growing for a launch-time fact.
    private(set) var depthScanStatus: DepthScanStatus = .unsupported
    /// Main-thread only: the scan result the core was (or will be) built
    /// with. Retrying a failed startup must not rescan the room.
    private var scanOutcome: DepthScanResult?
    /// Main-thread only: one core build at a time, scan or retry.
    private var coreCreationStarted = false

    /// The sensors card's depth hint. It is a line of text, not a control:
    /// the scan itself is a launch fact, not something the sheet switches.
    var depthScanLine: String {
        switch depthScanStatus {
        case .unsupported:
            return Copy.depthUnsupported
        case .denied:
            return Copy.denied
        case .scanning:
            return "…"
        case .failed:
            return "—"
        case .scanned(let boxes, _):
            return boxes > 0 ? "\(Copy.depthScanNote) · \(boxes)" : Copy.depthScanNote
        }
    }

    /// The pet-sound note promises the official model; show it only when
    /// that model was really staged beside the scene.
    var petSoundAvailable: Bool { PetPCM.modelStaged }
    /// The room's microphone. It taps Voice's engine rather than making a
    /// second one: one app, one AVAudioSession. Created at init but never
    /// started until the sheet's toggle is switched on.
    /// `voice` is another stored property: a stored-property initializer runs
    /// before self exists, so this must be lazy.
    private lazy var mic = MicFeed(voice: voice)
    /// Published answers from that feed; the sheet reads these instead of
    /// importing AVFoundation just to show "denied".
    @Published private(set) var micDenied = false
    @Published private(set) var micAvailable = false
    private var micCancellable: AnyCancellable?
    /// The duck's ear's answers, mirrored from the same feed: the sheet
    /// reads these instead of importing Speech.
    private var speechCancellable: AnyCancellable?
    private var heardCancellable: AnyCancellable?

    /// The renderer's triangle soups, built on the worker queue before the
    /// first frame, keyed by MuJoCo mesh id.
    private(set) var meshVertices: [Int32: [Float]] = [:]

    /// One body's pose at a tick boundary, for the renderer to interpolate
    /// between. MuJoCo steps at 50 Hz; a display draws at 60–120 Hz, so
    /// without this the duck moves in visible 20 ms stairs.
    struct BodyPose {
        var position: SIMD3<Float>
        var rotation: SIMD4<Float>
    }

    private(set) var previousPoses: [BodyPose] = []
    private(set) var currentPoses: [BodyPose] = []
    /// When `currentPoses` was captured, on the same clock the renderer reads.
    private(set) var poseCapturedAt: TimeInterval = 0
    /// Wall time between the last two captures. A fixed 20 ms would saturate
    /// alpha halfway through a slowed tick and freeze the pose.
    private var poseInterval: TimeInterval = 0.02
    private var bodyCount = 0

    /// The renderer drew one frame. Called from `draw(in:)` once per frame.
    func noteFrame() {
        frameCount += 1
    }

    /// 0…1 through the current 20 ms tick, for the renderer.
    func interpolationAlpha(now: TimeInterval = Date().timeIntervalSinceReferenceDate) -> Float {
        guard poseCapturedAt > 0 else { return 1 }
        let alpha = (now - poseCapturedAt) / max(poseInterval, 0.001)
        return Float(min(max(alpha, 0), 1))
    }

    /// A body's pose between the last two ticks. Falls back to the latest tick
    /// when there is no previous one yet (the first frame).
    func interpolatedPose(body: Int, alpha: Float) -> BodyPose? {
        guard body >= 0, body < currentPoses.count else { return nil }
        guard body < previousPoses.count else { return currentPoses[body] }
        let a = previousPoses[body]
        let b = currentPoses[body]
        var rotation = b.rotation
        // Quaternions are double covers: q and -q are the same rotation, and
        // lerping without the sign check takes the long way round.
        if simd_dot(a.rotation, b.rotation) < 0 {
            rotation = -b.rotation
        }
        let blended = a.rotation + (rotation - a.rotation) * alpha
        return BodyPose(
            position: a.position + (b.position - a.position) * alpha,
            rotation: simd_normalize(blended)
        )
    }

    /// Snapshot every body's pose, after a tick. Called on the main thread.
    private func capturePoses(core: OpaquePointer) {
        // Read every frame: a mode switch loads a model with a different body
        // count, and a cached one would silently drop the roller's bodies from
        // the snapshots (no interpolation, stale poses).
        bodyCount = Int(duck_core_body_count(core))
        guard bodyCount > 0 else { return }
        var poses = previousPoses
        poses.removeAll(keepingCapacity: true)
        poses.reserveCapacity(bodyCount)
        var position = [Double](repeating: 0, count: 3)
        var rotation = [Double](repeating: 0, count: 4)
        for body in 0..<bodyCount {
            position.withUnsafeMutableBufferPointer { pos in
                rotation.withUnsafeMutableBufferPointer { quat in
                    _ = duck_core_body_pose(core, Int32(body), pos.baseAddress, quat.baseAddress)
                }
            }
            poses.append(BodyPose(
                position: SIMD3<Float>(Float(position[0]), Float(position[1]), Float(position[2])),
                rotation: SIMD4<Float>(Float(rotation[0]), Float(rotation[1]),
                                       Float(rotation[2]), Float(rotation[3]))
            ))
        }
        previousPoses = currentPoses
        currentPoses = poses
        let now = Date().timeIntervalSinceReferenceDate
        if poseCapturedAt > 0 {
            poseInterval = min(max(now - poseCapturedAt, 0.001), 0.2)
        }
        poseCapturedAt = now
    }

    init() {
        // The camera coming up later than Director opening must flip the
        // renderer over without the shell having to poll.
        cameraCancellable = camera.$available
            .receive(on: RunLoop.main)
            .sink { [weak self] available in
                guard let self else { return }
                self.cameraActive = self.wantsCamera && available
            }
        // The diary's speak button follows the synthesizer's start/stop the
        // moment it happens; every callback is already on the main thread.
        // The same client intent a robot-side chorus writes goes out for the
        // open AND the close — a natural finish must not leave /state holding
        // a beak the duck is not keeping open.
        compassOn = UserDefaults.standard.bool(forKey: "duck_compass_on")
        speaker.onChange = { [weak self] speaking in
            guard let self else { return }
            self.ttsSpeaking = speaking
            self.postJSON("/mouth", ["open": speaking ? 1.0 : 0.0])
        }
        // A refused camera has no readings; mirror the answer so the sheet
        // can say "denied" without importing AVFoundation.
        cameraDeniedCancellable = camera.$denied
            .receive(on: RunLoop.main)
            .sink { [weak self] denied in
                self?.cameraDenied = denied
            }
        // The mic's permission answer arrives on a system callback; the sheet
        // reads it through these published properties so it never has to know
        // about AVFoundation. CombineLatest also catches stop()'s `available`
        // flip, which clears a stale reading from the row.
        micCancellable = Publishers.CombineLatest(mic.$denied, mic.$available)
            .receive(on: RunLoop.main)
            .sink { [weak self] denied, available in
                guard let self else { return }
                self.micDenied = denied
                self.micAvailable = available
                self.refreshMicSummary()
            }
        // The duck's ear answers on its own queue: a permission answer, a
        // recogniser that is not there, and the last thing it heard. The
        // sheet reads these mirrors, so it never imports Speech either.
        speechCancellable = mic.$speechState
            .receive(on: RunLoop.main)
            .sink { [weak self] state in
                self?.speechState = state
            }
        heardCancellable = mic.$heardText
            .receive(on: RunLoop.main)
            .sink { [weak self] text in
                guard let self, !text.isEmpty, text != self.heardText else { return }
                self.speechArrived(text)
            }
        // A persisted "on" from a previous launch is resumed only when the
        // earlier grant is still valid. This is NOT a launch-time permission
        // prompt: .undetermined never prompts here, and neither does .denied.
        if micEnabled {
            switch AVAudioApplication.shared.recordPermission {
            case .granted: mic.start()
            case .denied: mic.setDenied()
            case .undetermined: break
            @unknown default: break
            }
        }
        // The same rule for the duck's ear: what the user already granted
        // comes back, and nothing is asked at launch.
        if voiceCommandsEnabled {
            mic.resumeSpeech()
        }
        // Creating the core loads the MuJoCo scene (215k vertices) and the
        // ONNX policies — seconds on a device. Doing that here blocked the
        // main thread, so SwiftUI never rendered a frame and the app showed a
        // black screen. Build it on a worker queue and publish on the main
        // thread; the core itself is only touched from the main thread
        // afterwards (timer and renderer).
        //
        // On a LiDAR device the one-second depth scan runs first, because
        // obstacles.xml has to exist when MuJoCo compiles the scene. On any
        // other device (and in the simulator) that path is honestly skipped.
        beginLaunchScan()
    }

    /// The failure card's one action. A core that never came up is built
    /// again; a Metal view that failed is dropped so SwiftUI makes a fresh
    /// one on the next body pass. Nothing else is reset.
    func retryStartup() {
        if ready {
            renderError = nil
            return
        }
        // One retry in flight: the card's tap clears the error, so a second
        // tap while the loader runs is a no-op rather than a second core.
        guard errorMessage != nil else { return }
        errorMessage = nil
        // A retry reuses the launch scan: another AR pass would delay the
        // core again, and the furniture has not changed since this launch.
        coreCreationStarted = false
        createCoreOnWorker()
    }

    /// The launch scan's one owner. It must produce (or refuse) the
    /// obstacle file before the core compiles the scene, so the core is
    /// built from its completion. A camera the user has never granted is
    /// not a reason to prompt at launch (Director owns that prompt, and
    /// Android's scan makes the same call); the path answers "denied" and
    /// the next launch after a Director visit can scan. A four-second
    /// watchdog covers a session that never delivers a frame, so the app
    /// wakes without obstacles instead of sitting on "waking the duck…".
    private func beginLaunchScan() {
        guard DepthScanner.isSupported else {
            depthScanStatus = .unsupported
            scanOutcome = nil
            createCoreOnWorker()
            return
        }
        switch AVCaptureDevice.authorizationStatus(for: .video) {
        case .authorized:
            break
        case .denied, .restricted, .notDetermined:
            // The sheet says "denied"; it never blames a depth sensor the
            // phone does have, and it never asks here.
            depthScanStatus = .denied
            scanOutcome = nil
            createCoreOnWorker()
            return
        @unknown default:
            depthScanStatus = .denied
            scanOutcome = nil
            createCoreOnWorker()
            return
        }
        depthScanStatus = .scanning
        DispatchQueue.main.asyncAfter(deadline: .now() + 4.0) { [weak self] in
            guard let self, !self.coreCreationStarted else { return }
            NSLog("duck: depth scan watchdog fired; building without obstacles")
            self.depthScanner.cancel()
            self.depthScanStatus = .failed
            self.scanOutcome = nil
            self.createCoreOnWorker()
        }
        depthScanner.scan { [weak self] outcome in
            guard let self, !self.coreCreationStarted else { return }
            switch outcome {
            case .unsupported:
                self.depthScanStatus = .unsupported
                self.scanOutcome = nil
            case .denied:
                self.depthScanStatus = .denied
                self.scanOutcome = nil
            case .failed:
                self.depthScanStatus = .failed
                self.scanOutcome = nil
            case .done(let result):
                self.depthScanStatus = .scanned(
                    boxes: result.boxes.count,
                    floorOffset: result.floorOffset
                )
                self.scanOutcome = result
            }
            self.createCoreOnWorker()
        }
    }

    /// Main thread. Snapshot the scan so the worker never reads mutable
    /// controller state.
    private func createCoreOnWorker() {
        guard !coreCreationStarted else { return }
        coreCreationStarted = true
        let scan = scanOutcome
        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            self?.createCore(scan: scan)
        }
    }

    /// Runs off the main thread; hops back to publish the result.
    private func createCore(scan: DepthScanResult?) {
        guard let resourceURL = Bundle.main.resourceURL else {
            DispatchQueue.main.async { self.errorMessage = "Bundle resources not found" }
            return
        }
        // The scan's boxes become obstacles.xml. The bundle is read-only, so
        // the shell mirrors the scene beside it in Application Support and
        // hands the core that copy; without boxes the bundled scene is used
        // and no 23 MB mirror is made.
        let obstacleXML = DepthObstaclePlanner.makeXML(boxes: scan?.boxes ?? [])
        let staged = DuckSceneStaging.prepare(resourceURL: resourceURL, obstacleXML: obstacleXML)
        let scene = staged.sceneURL.path
        if !obstacleXML.isEmpty && !staged.wroteObstacles {
            NSLog("duck: scan found obstacles but obstacles.xml could not be staged")
            DispatchQueue.main.async { [weak self] in
                self?.depthScanStatus = .failed
            }
        }
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
            guard let created else {
                self.errorMessage = error.withUnsafeBufferPointer {
                    String(cString: $0.baseAddress!)
                }
                NSLog("duck: duck_core_new failed: %@", self.errorMessage ?? "unknown")
                return
            }

            self.core = created
            self.meshVertices = meshes
            self.ready = true
            // Core loading took seconds; start the performance window here,
            // not at object init, or the first report divides by load time.
            self.perfAnchor = Date()
            NSLog("duck: core created")
            // The pickup classifier: the duck pauses its policy in a hand for
            // the same reason the physical one does.
            let pickup = resourceURL
                .appendingPathComponent("DuckResources/models/pickup_detector.onnx").path
            let pickupLoaded = pickup.withCString { pointer in
                duck_core_load_pickup(created, pointer)
            }
            NSLog("duck: pickup detector -> %d", pickupLoaded)
            // `robot.do curious` and the other twelve: the official emotion
            // keyframes and their 48 kHz voices, staged beside the policies.
            let emotions = resourceURL.appendingPathComponent("DuckResources/emotions").path
            let emotionCount = emotions.withCString { pointer in
                duck_core_load_emotions(created, pointer)
            }
            NSLog("duck: emotions -> %d", emotionCount)
            let port = duck_core_start_intent_server(created, 8765)
            NSLog("duck: intent server -> %d", port)
            // The AR floor: a scene-relative offset, the core's current
            // `/floor` dimension (0 = unchanged). The scan defines the
            // detected plane as the duck's floor, so the honest offset is
            // 0; a camera height is explicitly refused by the core and is
            // not what this shell invents.
            if let floorOffset = scan?.floorOffset {
                NSLog("duck: AR floor offset %.2f m", floorOffset)
                self.postJSON("/floor", ["z": floorOffset])
            }
            self.voice.start()
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
        // Zero is a real answer at 0.5x: the next frame runs the tick. Intents
        // still drain on that tick, so nothing waits longer than 40 ms.
        return min(whole, 8)
    }

    deinit {
        timer?.invalidate()
        sensorTimer?.invalidate()
        // Release the input tap, the duck's ear and the 1 Hz POST timer with
        // the controller; the session is one per app and must not outlive its
        // owner.
        mic.stopAll()
        motion.stopDeviceMotionUpdates()
        if let core {
            duck_core_free(core)
        }
    }

    /// Whether the window is taller than it is wide, read from the active
    /// scene rather than UIScreen (which is deprecated and wrong in split
    /// view). Falls back to the first connected scene.
    private static var interfaceIsPortrait: Bool {
        let scene = UIApplication.shared.connectedScenes
            .compactMap { $0 as? UIWindowScene }
            .first { $0.activationState == .foregroundActive }
            ?? UIApplication.shared.connectedScenes.compactMap { $0 as? UIWindowScene }.first
        guard let orientation = scene?.interfaceOrientation else { return true }
        return orientation == .portrait || orientation == .portraitUpsideDown
    }

    /// The duck is in Twin mode: the phone's IMU does not replace its
    /// proprioception, it applies external disturbances (a shake is a shove).
    private func startMotion() {
        guard motion.isDeviceMotionAvailable else { return }
        motion.deviceMotionUpdateInterval = 0.02
        // The arbitrary-Z reference frame makes `heading` valid (a stable
        // yaw against the first frame's bearing), which the plain update
        // does not carry.
        motion.startDeviceMotionUpdates(
            using: .xArbitraryZVertical,
            to: .main,
        ) { [weak self] motion, _ in
            guard let self, let motion else { return }
            // The compass: a real bearing, not a guess. The delta from the
            // bearing at switch-on rotates the room under the camera; the
            // shortest-way-around keeps the motion continuous across the
            // 0/360 seam.
            if self.compassOn {
                let heading = motion.heading
                if heading >= 0 {
                    if self.compassBearing == nil { self.compassBearing = heading }
                    if let bearing = self.compassBearing {
                        var delta = (heading - bearing) * Double.pi / 180.0
                        while delta > Double.pi { delta -= 2 * Double.pi }
                        while delta < -Double.pi { delta += 2 * Double.pi }
                        self.compassYaw = Float(-delta)
                    }
                }
            }
            // Tilt-to-look: with the switch on and the joystick idle, the
            // phone's attitude steers the head, as the real pad's IMU does.
            // Landscape: rolling the phone pans (yaw), pitching it tilts
            // (pitch). Clamped to roughly the joystick's own range so a
            // wrist movement can never fold the neck over.
            if self.tiltLook,
               self.joystick == .zero,
               !self.carrying,
               self.ready {
                let gravity = motion.gravity
                // The device axes swap with the interface orientation, so the
                // mapping does too: in portrait the short edge is x (roll =
                // pan) and the long edge is y (tip = pitch); in landscape it
                // is the other way round.
                let portrait = Self.interfaceIsPortrait
                let yaw = portrait ? gravity.x * 0.9 : -gravity.z * 0.9
                let pitch = portrait ? -gravity.y * 0.45 : gravity.x * 0.45
                // A phone lying flat on a table is the neutral pose: below a
                // few degrees of tilt, say nothing, so the duck's own
                // look-around is not silenced by a resting phone.
                if abs(yaw) > 0.07 || abs(pitch) > 0.07 {
                    self.setHead(yaw: yaw, pitch: pitch)
                }
            }
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

    /// The app goes to the background: remember what the duck had done by
    /// then, so the return can say what happened since.
    func appWentBackground() {
        eventsAtBackground = lastEvents
        // Save the companion state at the door as well as on the 15 s cadence:
        // a phone can be killed while suspended, and the duck should wake up
        // with the tummy it went to sleep with. `simctl terminate` sends no
        // scenePhase change, so the cadence has to be frequent enough too.
        if let care = latestCare, companionRestored {
            var payload: [String: Any] = ["care": Self.plistSafe(care)]
            if let life = latestLife { payload["life"] = Self.plistSafe(life) }
            UserDefaults.standard.set(payload, forKey: "duck_companion_v1")
        }
        saveCompanionNow()
    }

    /// UserDefaults is a property list: one NSNull anywhere in a saved
    /// dictionary is an exception at write time, and the core publishes
    /// optional fields as JSON null. Strip them before anything lands in
    /// the shell's own store; a missing key reads as its default, which is
    /// exactly what "no allowance yet" means.
    static func plistSafe(_ value: Any) -> Any {
        switch value {
        case let dictionary as [String: Any]:
            var safe: [String: Any] = [:]
            for (key, inner) in dictionary {
                if inner is NSNull { continue }
                safe[key] = Self.plistSafe(inner)
            }
            return safe
        case let array as [Any]:
            return array.map { Self.plistSafe($0) }
        default:
            return value
        }
    }

    /// One JSON body to one of the core's POST routes. Fire-and-forget: the
    /// next /state poll shows the effect.
    private func post(_ path: String, _ body: Data) {
        guard let url = URL(string: "http://127.0.0.1:8765\(path)") else { return }
        var request = URLRequest(url: url)
        request.httpMethod = "POST"
        request.timeoutInterval = 2.0
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.httpBody = body
        URLSession.shared.dataTask(with: request).resume()
    }

    /// The shell's only persistent copy of the duck's inner life. It hands the
    /// numbers back to the core on the next launch; the core is the authority
    /// and clamps whatever it is given.
    private func saveCompanionState(care: [String: Any], life: [String: Any]?) {
        // Keep the most recent read even before the restore lands; the
        // background path needs a copy that no network request can outlive.
        latestCare = care
        latestLife = life
        // Until the saved payload has been handed back, a poll's default care
        // must not overwrite it: the first launch's 1 Hz save could otherwise
        // race the restore and persist a fresh duck over the real one.
        guard companionRestored else { return }
        guard Date().timeIntervalSince(lastCompanionSave) >= 4 else { return }
        var payload: [String: Any] = ["care": Self.plistSafe(care)]
        if let life { payload["life"] = Self.plistSafe(life) }
        UserDefaults.standard.set(payload, forKey: "duck_companion_v1")
        lastCompanionSave = Date()
    }

    private func saveCompanionNow() {
        // The same gate as the cadence save: until the saved payload has been
        // handed back, a poll's fresh-process defaults must not overwrite it.
        guard companionRestored else { return }
        guard let url = URL(string: "http://127.0.0.1:8765/state") else { return }
        var request = URLRequest(url: url)
        request.timeoutInterval = 1.5
        URLSession.shared.dataTask(with: request) { data, _, _ in
            guard let data,
                  let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
                  let care = json["care"] as? [String: Any]
            else { return }
            var payload: [String: Any] = ["care": Self.plistSafe(care)]
            if let life = json["life"] as? [String: Any] { payload["life"] = Self.plistSafe(life) }
            UserDefaults.standard.set(payload, forKey: "duck_companion_v1")
        }.resume()
    }

    /// Hand the stored walk-slot name back through `/config`. This is the
    /// route that carries the core's official `walk="none"` refusal — the
    /// C ABI's historic wrapper still clears that sentinel in silence — so
    /// the shell sends the name verbatim and lets the core rule. Runs on
    /// the restore worker; the intent server comes up with the core, so it
    /// waits for /state before the one POST. A refusal is final and is not
    /// retried; false means the name was not taken.
    private static func replayWalkPolicy(_ name: String) -> Bool {
        var attempts = 0
        while attempts < 40 {
            attempts += 1
            guard getStateSync() != nil else {
                Thread.sleep(forTimeInterval: 0.25)
                continue
            }
            guard let body = try? JSONSerialization.data(
                withJSONObject: ["walk_policy": name]
            ) else { return false }
            let accepted = postSyncData("/config", body)
            if !accepted {
                NSLog("duck: walk policy %@ refused by the core", name)
            }
            return accepted
        }
        NSLog("duck: walk policy %@ not replayed: core not ready", name)
        return false
    }

    /// Hand the learned words back through `/config` once the intent server
    /// answers. Same readiness loop as the walk slot: the server comes up
    /// with the core, and a launch can beat it. A refusal is final and is
    /// not retried; false means the table was not taken. Called on the
    /// restore worker.
    private static func replayVoiceAliases(_ table: [String: String]) -> Bool {
        guard let body = VoiceAliasStore.configBody(table) else { return false }
        var attempts = 0
        while attempts < 40 {
            attempts += 1
            guard getStateSync() != nil else {
                Thread.sleep(forTimeInterval: 0.25)
                continue
            }
            return postSyncData("/config", body)
        }
        NSLog("duck: voice aliases not replayed: core not ready")
        return false
    }

    /// Whether the boot set actually carries the stored walk file. The
    /// core's loader drops an override whose file is missing from the
    /// directory it is loading, so reloading the boot set for a name that
    /// lives beside a user's set would clear the override instead of
    /// preserving it for that set. When it is not here, the override waits
    /// for the reload that loads its own set.
    private func bootSetHasWalkPolicy(_ name: String) -> Bool {
        setHasWalkPolicy(name, in: policyDirectory)
    }

    /// Whether a policy directory carries the named walk file.
    private func setHasWalkPolicy(_ name: String, in directory: String) -> Bool {
        guard !directory.isEmpty else { return false }
        let path = URL(fileURLWithPath: directory)
            .appendingPathComponent(name)
            .path
        return FileManager.default.fileExists(atPath: path)
    }

    /// The directory Reload AI resolves: the person's own set when it has a
    /// manifest, otherwise the bundled official set.
    private func policyReloadDirectory() -> String {
        let documents = FileManager.default.urls(for: .documentDirectory, in: .userDomainMask).first
        let userPolicies = documents?.appendingPathComponent("policies")
        if let userPolicies,
           FileManager.default.fileExists(atPath: userPolicies.appendingPathComponent("manifest.json").path) {
            return userPolicies.path
        }
        return policyDirectory
    }

    /// Re-resolve the set the core booted with so a replayed walk override
    /// lands: the override is read while a set loads, and `duck_core_new`
    /// loaded its set before the replay. The caller only does this when the
    /// boot set carries the named file — see `bootSetHasWalkPolicy` — so
    /// the loader cannot drop the override on the way.
    private func reloadBootPolicies() {
        guard let core, !policyDirectory.isEmpty else { return }
        let loaded = policyDirectory.withCString {
            duck_core_reload_policies(core, $0)
        }
        if loaded != 0, let reason = duck_core_last_error() {
            NSLog("duck: walk policy reload refused: %@", String(cString: reason))
        }
    }

    /// Called once the core is up: give back what the last session saved. A
    /// fresh install has nothing; the duck starts at its defaults. The walk
    /// slot is part of this hand-back: the core's override is process
    /// memory, so the name goes through `/config` here, and the boot set is
    /// re-resolved when it carries that file so the duck wakes on the gait
    /// it went to sleep on. A name that belongs to a user's own set waits
    /// for the reload that loads it.
    func restoreCompanionState() {
        guard !companionRestoreStarted else { return }
        companionRestoreStarted = true
        let defaults = UserDefaults.standard
        let saved = defaults.dictionary(forKey: "duck_companion_v1")
        var rulesPayload: Data?
        var savedRules: [DuckRule] = []
        let questSaved = defaults.dictionary(forKey: Self.questKey) as? [String: Any]
        let day = Self.isoDay()
        lastQuestDayPosted = day
        // Tell the core whose calendar day it is, with the board we saved.
        // This is independent of care/rules restore: a fresh install still
        // gets three quests today.
        let dayPayload: [String: Any] = [
            "day": day,
            "board": questSaved ?? [:],
        ]
        if let body = try? JSONSerialization.data(withJSONObject: dayPayload) {
            post("/quests/day", body)
        }
        if let data = defaults.data(forKey: Self.rulesKey),
           let decoded = try? JSONDecoder().decode([DuckRule].self, from: data) {
            savedRules = decoded
            // The same payload saveRules builds, follow-up steps and a
            // timer's period included: a restore that drops either
            // silently truncates the child's program, and the next Blocks
            // edit writes the truncation back.
            let payload = Self.rulesPayload(decoded)
            rulesPayload = try? JSONSerialization.data(withJSONObject: payload)
        }
        let walkName = walkPolicies.name
        // A fresh install with only learned words has something to hand
        // back too; leaving this out of the guard silently skipped the
        // alias replay until some other state existed.
        let aliases = voiceAliases.table
        guard saved != nil || rulesPayload != nil || walkName != nil || !aliases.isEmpty else {
            companionRestored = true
            return
        }
        // The intent server comes up with the core; a launch can beat it.
        // Retry on a worker until the core acknowledges, and keep the save
        // gate closed until then so a fresh duck cannot overwrite the file.
        DispatchQueue.global(qos: .utility).async { [weak self] in
            var delivered = true
            // Before care and rules: the duck should wake on the gait it
            // went to sleep on. The name travels verbatim; the core rules
            // on it, `"none"` included.
            var walkTaken = false
            if let walkName {
                walkTaken = Self.replayWalkPolicy(walkName)
            }
            // The words a person taught the duck: one whole-table replace,
            // the same hand-back the walk slot gets. A refusal is the
            // core's ruling and is not retried; the table stays on the
            // phone for the next launch.
            if !aliases.isEmpty {
                let taken = Self.replayVoiceAliases(aliases)
                if !taken {
                    NSLog("duck: voice aliases refused by the core")
                }
            }
            if let saved {
                let care = saved["care"] as? [String: Any]
                let life = saved["life"] as? [String: Any]
                // POST /care is fire-and-forget, so a 2xx only proves the
                // server received it. Wait for /state to publish the bond
                // before opening the save gate.
                let wanted = care?["bond"] as? Double
                var ok = false
                var attempts = 0
                while attempts < 40 && !ok {
                    attempts += 1
                    guard Self.getStateSync() != nil else {
                        Thread.sleep(forTimeInterval: 0.25)
                        continue
                    }
                    let careOK = care.map { Self.postSync("/care", $0) } ?? true
                    let lifeOK = life.map { Self.postSync("/life", $0) } ?? true
                    if careOK && lifeOK && wanted != nil {
                        for _ in 0..<8 {
                            Thread.sleep(forTimeInterval: 0.1)
                            if let now = Self.getStateSync(),
                               let c = now["care"] as? [String: Any],
                               let bond = c["bond"] as? Double,
                               abs(bond - wanted!) < 0.02 {
                                ok = true
                                break
                            }
                        }
                    } else {
                        ok = careOK && lifeOK
                    }
                    if !ok { Thread.sleep(forTimeInterval: 0.25) }
                }
                delivered = delivered && ok
            }
            if let rulesPayload {
                var attempts = 0
                var ok = false
                while attempts < 40 && !ok {
                    attempts += 1
                    ok = Self.postSyncData("/rules", rulesPayload)
                    if !ok { Thread.sleep(forTimeInterval: 0.25) }
                }
                delivered = delivered && ok
            }
            DispatchQueue.main.async {
                guard let self else { return }
                if !savedRules.isEmpty { self.rules = savedRules }
                self.companionRestored = delivered
                // The override is read by a load; the boot set was
                // resolved before this replay. Re-resolve it only when it
                // owns the named file — otherwise the override stays in
                // the core for the reload that loads its own set.
                if walkTaken, let walkName, self.bootSetHasWalkPolicy(walkName) {
                    self.reloadBootPolicies()
                }
            }
        }
    }

    /// Every official verb with its real count. `firstSeen` is stamped the
    /// first time this phone sees a nonzero count; it is a local keepsake,
    /// not a second source of truth.
    private func updateDex(skills: [String: Any], emotions: [String: Any]) {
        let names = skillNames()
        guard !names.isEmpty else { return }
        var first = UserDefaults.standard.dictionary(forKey: Self.dexKey) as? [String: String] ?? [:]
        var entries: [DexEntry] = []
        for name in names {
            let skill = skills[name] as? Int ?? 0
            let emotion = emotions[name] as? Int ?? 0
            let count = max(skill, emotion)
            if count > 0 && first[name] == nil {
                first[name] = Self.isoDay()
            }
            entries.append(DexEntry(
                id: name,
                count: count,
                firstSeen: first[name],
                isEmotion: Self.emotionNames.contains(name)
            ))
        }
        UserDefaults.standard.set(first, forKey: Self.dexKey)
        dex = entries.sorted { a, b in
            if a.isEmotion != b.isEmotion { return !a.isEmotion }
            if a.count != b.count { return a.count > b.count }
            return a.id < b.id
        }
    }

    /// robotd's thirteen official emotions.
    private static let emotionNames: Set<String> = [
        "angry", "curious", "defiant", "devastated", "excited", "impatient",
        "laugh", "mmh", "mock", "no", "sad", "yes", "yes_fast",
    ]

    /// The phone's calendar day in ISO form. The core measures quests; the
    /// shell owns what "today" means.
    private static func isoDay() -> String {
        let formatter = DateFormatter()
        formatter.calendar = Calendar(identifier: .gregorian)
        formatter.locale = Locale(identifier: "en_US_POSIX")
        formatter.timeZone = .current
        formatter.dateFormat = "yyyy-MM-dd"
        return formatter.string(from: Date())
    }

    /// Tell the core whose calendar day it is — but only when it changed.
    /// The board travels with the post so a relaunch keeps today's progress.
    private func postQuestDayIfRollover() {
        let day = Self.isoDay()
        guard day != lastQuestDayPosted else { return }
        lastQuestDayPosted = day
        let board = UserDefaults.standard.dictionary(forKey: Self.questKey) ?? [:]
        let payload: [String: Any] = ["day": day, "board": board]
        if let body = try? JSONSerialization.data(withJSONObject: payload) {
            post("/quests/day", body)
        }
    }

    /// A blocking POST for the restore path only: it must know whether the
    /// core really took the state, which the fire-and-forget helpers cannot
    /// tell it. Called on a utility queue.
    private static func postSync(_ path: String, _ object: [String: Any]) -> Bool {
        guard let data = try? JSONSerialization.data(withJSONObject: object) else { return false }
        return postSyncData(path, data)
    }

    /// One /state on the restore worker. Its presence means the loop is
    /// publishing; its content is used to verify a restore actually landed.
    private static func getStateSync() -> [String: Any]? {
        guard let url = URL(string: "http://127.0.0.1:8765/state") else { return nil }
        var request = URLRequest(url: url)
        request.timeoutInterval = 2
        let semaphore = DispatchSemaphore(value: 0)
        var result: [String: Any]?
        URLSession.shared.dataTask(with: request) { data, _, _ in
            if let data {
                result = try? JSONSerialization.jsonObject(with: data) as? [String: Any]
            }
            semaphore.signal()
        }.resume()
        _ = semaphore.wait(timeout: .now() + 3)
        return result
    }

    private static func postSyncData(_ path: String, _ body: Data) -> Bool {
        guard let url = URL(string: "http://127.0.0.1:8765\(path)") else { return false }
        var request = URLRequest(url: url)
        request.httpMethod = "POST"
        request.timeoutInterval = 2
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.httpBody = body
        let semaphore = DispatchSemaphore(value: 0)
        var ok = false
        URLSession.shared.dataTask(with: request) { data, response, _ in
            if let http = response as? HTTPURLResponse {
                ok = (200..<300).contains(http.statusCode)
            }
            // The intent API answers refusals with HTTP 200 and {"ok":false};
            // only a body that did not refuse counts as delivered.
            if let data,
               let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
               let accepted = json["ok"] as? Bool {
                ok = ok && accepted
            }
            semaphore.signal()
        }.resume()
        _ = semaphore.wait(timeout: .now() + 3)
        return ok
    }

    /// Open the LAN socket and announce this phone. Called when the user
    /// opens Neighbours — the first local-network use is the moment iOS asks
    /// for permission, not every launch.
    func startPeers() {
        let payload = ["name": UIDevice.current.name]
        guard let body = try? JSONSerialization.data(withJSONObject: payload) else { return }
        post("/peers/start", body)
    }

    // MARK: - The user's own model

    func saveAIConfig(_ config: AIConfig) {
        aiConfig = config
        config.save()
        // Do not rewind the clock here: "Think now" calls save then think,
        // and a rewound stamp made the 1 Hz poll fire a second request right
        // after the first.
        lastAIThink = Date()
        aiStatus = config.enabled ? Copy.aiOn : Copy.aiOff
        aiError = nil
    }

    /// Runs from the 1 Hz sensor poll; never more often than the configured
    /// interval, and never twice at once (AIBrain refuses while busy).
    private func maybeThinkAI() {
        guard aiConfig.enabled, ready,
              !aiConfig.endpoint.trimmingCharacters(in: .whitespaces).isEmpty,
              !aiRequestInFlight,
              Date().timeIntervalSince(lastAIThink) >= max(10, aiConfig.interval)
        else { return }
        lastAIThink = Date()
        aiThinkNow()
    }

    /// The person just said something. A conversation cannot wait out the
    /// periodic think (10 s floor, 30 s default), so a fresh transcript asks
    /// for one decision immediately — still rate-limited to the same floor,
    /// because a model answered twice a second is not a conversation either.
    func speechArrived(_ text: String) {
        heardText = text
        guard aiConfig.enabled, ready else { return }
        if Date().timeIntervalSince(lastAIThink) >= 10 {
            lastAIThink = Date()
            aiThinkNow()
        }
    }

    /// One decision now, whether or not the loop is on.
    func aiThinkNow() {
        guard !aiRequestInFlight else { return }
        guard !aiConfig.endpoint.trimmingCharacters(in: .whitespaces).isEmpty else {
            aiError = Copy.aiNeedEndpoint
            return
        }
        aiRequestInFlight = true
        lastAIThink = Date()
        aiStatus = Copy.aiThinking
        aiError = nil
        let group = DispatchGroup()
        var state: [String: Any]?
        var skills: [String] = []
        group.enter()
        URLSession.shared.dataTask(
            with: URL(string: "http://127.0.0.1:8765/state")!
        ) { data, _, _ in
            if let data {
                state = try? JSONSerialization.jsonObject(with: data) as? [String: Any]
            }
            group.leave()
        }.resume()
        group.enter()
        URLSession.shared.dataTask(
            with: URL(string: "http://127.0.0.1:8765/skills")!
        ) { data, _, _ in
            if let data,
               let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
               let list = json["skills"] as? [String] {
                skills = list
            }
            group.leave()
        }.resume()
        group.notify(queue: .main) { [weak self] in
            guard let self else { return }
            guard let state else {
                self.aiRequestInFlight = false
                self.aiError = "the duck did not answer /state"
                self.aiStatus = Copy.aiOffline
                return
            }
            let heard = (state["last_heard"] as? [String: Any])?["text"] as? String
                ?? self.heardText
            self.aiBrain.think(
                config: self.aiConfig,
                state: state,
                skills: skills,
                heard: heard
            ) { [weak self] decision, error in
                guard let self else { return }
                self.aiRequestInFlight = false
                if let error {
                    self.aiError = error
                    self.aiStatus = Copy.aiOffline
                    return
                }
                guard let decision else { return }
                self.aiStatus = Copy.aiOn
                self.executeAI(decision)
            }
        }
    }

    private func executeAI(_ decision: AIDecision) {
        // The duck's answer comes first: `say` is the conversation, the
        // action is the body following it. A talk-only turn executes
        // nothing and just speaks. The system voice reads it in the
        // language the app is showing; the jaw follows through the
        // speaker's onChange, exactly as the diary reading does.
        if !decision.say.isEmpty {
            speaker.updateLanguage(Copy.language)
            speaker.speak(decision.say)
        }
        guard decision.action != "talk" else {
            aiDecision = decision.say
            return
        }
        // The model answers with wire names (`kick_left`). The stage says
        // what a person calls them; the instrument/API keep the wire.
        // When the duck also said something, the pill shows the sentence —
        // the words are the conversation, the action is the footnote.
        if !decision.say.isEmpty {
            aiDecision = decision.say
        } else {
            let shownValue = decision.action == "skill" || decision.action == "emotion"
                ? SkillLabel.of(decision.value)
                : decision.value
            let shown = decision.value.isEmpty
                ? decision.action
                : "\(decision.action) · \(shownValue)"
            aiDecision = decision.why.isEmpty ? "AI · \(shown)" : "AI · \(shown) — \(decision.why)"
        }
        // Every decision cancels a previous bounded move. Otherwise an old
        // twist chain would keep posting after a stop or a new look.
        moveGeneration &+= 1
        if decision.action != "move" {
            // Cancel any bounded walk before a new verb owns the duck; its
            // last twist otherwise keeps the 500 ms lease alive.
            postJSON("/twist", ["vx": 0.0, "vy": 0.0, "wz": 0.0])
        }
        switch decision.action {
        case "skill":
            postJSON("/skill", ["name": decision.value])
        case "sound":
            postJSON("/sound", ["tag": decision.value])
        case "sit":
            postJSON("/posture", ["sit": true])
        case "stand":
            postJSON("/posture", ["sit": false])
        case "stop":
            postJSON("/twist", ["vx": 0.0, "vy": 0.0, "wz": 0.0])
        case "look":
            postJSON("/look", ["x": decision.x, "y": decision.y, "z": decision.z])
        case "move":
            moveAI(vx: decision.vx, vz: decision.vz, seconds: decision.seconds)
        default:
            break
        }
    }

    /// A bounded walk: the model may ask for at most two seconds, and the
    /// loop stops it even if the app forgets. The deadman would too.
    private func moveAI(vx: Double, vz: Double, seconds: Double) {
        let frames = max(1, Int(seconds * 10))
        let generation = moveGeneration
        func step(_ remaining: Int) {
            guard generation == moveGeneration else { return }
            guard remaining > 0 else {
                postJSON("/twist", ["vx": 0.0, "vy": 0.0, "wz": 0.0])
                return
            }
            postJSON("/twist", ["vx": vx, "vy": 0.0, "wz": vz])
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.1) {
                step(remaining - 1)
            }
        }
        step(frames)
    }

    private func postJSON(_ path: String, _ object: [String: Any]) {
        guard let data = try? JSONSerialization.data(withJSONObject: object) else { return }
        post(path, data)
    }

    /// The chat reply through the diary's system voice: one synthesizer,
    /// the language the screen is showing, the same local /mouth intent.
    func speakChat(_ text: String) {
        speaker.updateLanguage(Copy.language)
        speaker.speak(text)
    }

    /// Say hello to one duck, or to every duck on the Wi-Fi.
    /// A room-wide score — a formation, a role dance or a voice choir.
    /// The wire carries only official mood/sound names; every duck plays
    /// its own part through its own 50 Hz runtime.
    func startEnsemble(moves: [String], sounds: [String] = []) {
        postJSON("/ensemble", ["moves": moves, "sounds": sounds])
    }

    /// The move-out document: the core packs care, quests and rules into
    /// one versioned file. The person carries it; the core never sees a
    /// server, and the file is as honest as the intents it came from.
    func fetchBackup(done: @escaping (String?) -> Void) {
        guard let url = URL(string: "http://127.0.0.1:8765/backup") else {
            done(nil)
            return
        }
        URLSession.shared.dataTask(with: url) { data, _, _ in
            done(data.flatMap { String(data: $0, encoding: .utf8) })
        }.resume()
    }

    /// A backup read back from a file: the core's restore door answers
    /// per-part, and the shell relays the verdict.
    func postRestore(_ text: String, done: @escaping (Bool) -> Void) {
        guard let url = URL(string: "http://127.0.0.1:8765/backup/restore") else {
            done(false)
            return
        }
        var request = URLRequest(url: url)
        request.httpMethod = "POST"
        request.timeoutInterval = 5.0
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.httpBody = Data(text.utf8)
        URLSession.shared.dataTask(with: request) { data, response, _ in
            let ok = (response as? HTTPURLResponse)?.statusCode == 200
                && (data.flatMap { try? JSONSerialization.jsonObject(with: $0) as? [String: Any] })
                    .map { $0["ok"] as? Bool == true } == true
            DispatchQueue.main.async { done(ok) }
        }.resume()
    }

    /// The system share sheet over one file URL, presented on the main
    /// thread from wherever the backup export finished.
    func shareFile(url: URL) {
        DispatchQueue.main.async {
            guard let scene = UIApplication.shared.connectedScenes
                .compactMap({ $0 as? UIWindowScene }).first,
                let root = scene.keyWindow?.rootViewController
            else { return }
            var top = root
            while let presented = top.presentedViewController { top = presented }
            let sheet = UIActivityViewController(activityItems: [url], applicationActivities: nil)
            top.present(sheet, animated: true)
        }
    }

    // MARK: - The time machine's thirds

    /// Push & compare: checkpoint, fork, then shove branch B. The rewind
    /// machine already keeps both branches; the shell only hands out the
    /// shove and tells the person where to look.
    func pushCompare() {
        postJSON("/checkpoint", [:])
        postJSON("/fork", [:])
        postJSON("/push", ["vx": 0.6, "vy": 0.0, "vz": 0.0])
    }

    /// Policy A/B: drop the last B, fork a fresh one, select it, and load the
    /// policy DIRECTORY there. The reload route's own verdict comes back —
    /// nil when the folder loaded, otherwise the core's reason — so the sheet
    /// shows what actually happened instead of a fixed "loaded". The posts
    /// are chained, not fired together: a reload that lands before the fork
    /// would dress branch A in the new set. (The routes read {"label": …} and
    /// {"dir": …}; the old keys here matched neither, so nothing ever reached
    /// branch B.)
    func loadPolicyOnBranchB(directory: String, completion: @escaping (String?) -> Void) {
        postForVerdict("/branch/drop", ["label": "branch-b"]) { _ in
            self.postForVerdict("/fork", ["label": "branch-b"]) { forkVerdict in
                if let forkVerdict {
                    completion(forkVerdict)
                    return
                }
                self.postForVerdict("/branch/select", ["label": "branch-b"]) { selectVerdict in
                    if let selectVerdict {
                        completion(selectVerdict)
                        return
                    }
                    // Loading a policy set is seconds of ONNX work; the route
                    // allows the reload 30 s.
                    self.postForVerdict("/reload", ["dir": directory], timeout: 35) { verdict in
                        completion(verdict)
                    }
                }
            }
        }
    }

    /// One POST that reads the answer: the intent routes return
    /// {"ok":true,…} or {"ok":false,"error":…} — nil on ok, the error text
    /// otherwise. (The fire-and-forget `postJSON` cannot carry a verdict.)
    private func postForVerdict(
        _ path: String,
        _ object: [String: Any],
        timeout: TimeInterval = 20,
        completion: @escaping (String?) -> Void
    ) {
        guard let data = try? JSONSerialization.data(withJSONObject: object),
              let url = URL(string: "http://127.0.0.1:8765\(path)") else {
            completion("no local server")
            return
        }
        var request = URLRequest(url: url)
        request.httpMethod = "POST"
        request.timeoutInterval = timeout
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.httpBody = data
        URLSession.shared.dataTask(with: request) { data, _, _ in
            let verdict: String? = data
                .flatMap { try? JSONSerialization.jsonObject(with: $0) as? [String: Any] }
                .flatMap { json in
                    if json["ok"] as? Bool == true { return nil }
                    return (json["error"] as? String) ?? "request failed"
                }
            DispatchQueue.main.async { completion(verdict) }
        }.resume()
    }

    /// The observation HUD's toggles, persisted like the mic.
    @Published var hudOn = UserDefaults.standard.bool(forKey: "duck_hud_on") {
        didSet {
            if hudOn != oldValue { UserDefaults.standard.set(hudOn, forKey: "duck_hud_on") }
        }
    }
    @Published var p2On = UserDefaults.standard.bool(forKey: "duck_p2_on") {
        didSet {
            if p2On != oldValue { UserDefaults.standard.set(p2On, forKey: "duck_p2_on") }
        }
    }
    /// The vet book's per-day rows, newest last — the parent's view.
    @Published var vetbookDays: [String] = []
    /// The duck 8x8 head grid as a tiny heat text: the official resampler
    /// output from whatever depth the phone published. Empty means nothing
    /// was published - the row says so rather than drawing a room.
    @Published var tofStrip = "-"

    func refreshTofStrip() {
        guard let url = URL(string: "http://127.0.0.1:8765/sensors") else { return }
        URLSession.shared.dataTask(with: url) { [weak self] data, _, _ in
            guard let self, let data,
                  let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
                  let grid = json["tof_grid"] as? [Double], grid.count == 64
            else { return }
            let rows = (0..<8).map { row in
                (0..<8).map { col -> String in
                    let value = grid[row * 8 + col]
                    switch value {
                    case ..<0.001: return "."
                    case ..<1.0: return "#"
                    case ..<2.0: return "="
                    case ..<3.0: return "-"
                    default: return "_"
                    }
                }.joined()
            }
            DispatchQueue.main.async { self.tofStrip = rows.joined(separator: " / ") }
        }.resume()
    }
    /// This phone as the official DKPD pad (the sender half). Only ever
    /// dials a PAIRED neighbour; the stick it sends is the shell's own.
    let padSender = PadSender()
    @Published var padTargetName = ""

    func togglePad(peer: PeerInfo) {
        if padSender.active {
            padSender.stop()
            padTargetName = ""
        } else if padSender.start(addr: peer.addr) {
            padTargetName = peer.name
        }
    }

    /// Rewind-play: while on, the tick rewinds 200 ticks every 150 ms -
    /// the same /seek the rail rewind button uses, continuously.
    @Published var rewinding = false
    private var rewindTimer: Timer?

    func toggleRewindPlay() {
        rewinding.toggle()
        if rewinding {
            rewindTimer = Timer.scheduledTimer(withTimeInterval: 0.15, repeats: true) { [weak self] _ in
                self?.postJSON("/seek", ["ticks": 200])
            }
        } else {
            rewindTimer?.invalidate()
            rewindTimer = nil
        }
    }

    /// The parent's daily allowance. Zero clears it; the core snapshots
    /// its own powered time and puts the duck to bed when the day is spent.
    func setParent(minutes: Int) {
        postJSON("/parent", ["minutes": minutes])
    }

    /// The official UDP pad: OFF by default, gated in the core by the pair
    /// code. A standard DKPD sender (duckctl udp-pad, a script) then drives
    /// this duck through the same twist the joystick uses.
    func setNetPad(on: Bool) {
        postJSON("/pad", ["on": on])
    }

    /// Both people type the same six digits; only paired neighbours may
    /// send gifts or start a chorus. The code itself never leaves the LAN
    /// outside the pair handshake, and no control verb rides with it.
    func pairPeer(code: String, id: String?) {
        var payload: [String: Any] = ["code": code]
        if let id { payload["id"] = id }
        postJSON("/peers/pair", payload)
    }

    func greetPeer(_ id: String?) {
        let payload: [String: Any] = id.map { ["id": $0] } ?? [:]
        guard let body = try? JSONSerialization.data(withJSONObject: payload) else { return }
        post("/peers/greet", body)
    }

    /// Send a neighbour a treat or a cheer. The core validates the peer and
    /// the gift, and its own 3 s throttle answers a double tap with
    /// "too soon"; a refusal never queues a packet, so nothing to wait for.
    func giftPeer(_ id: String, gift: String, value: String) {
        postJSON("/peers/gift", ["id": id, "gift": gift, "value": value])
    }

    /// Start a chorus: POST /chorus asks the core to lead with its two-move
    /// default. The core validates the names, refuses while one is already
    /// playing and never queues a refusal; fire-and-forget, so the next
    /// /state poll reports which answer it gave.
    func startChorus() {
        postJSON("/chorus", ["moves": ["laugh", "yes_fast"]])
    }

    // MARK: - The block program on the wire

    /// One rule as the core's wire object. Save, restore and share all use
    /// this one builder, so `period_s` cannot go missing on one path while
    /// the other keeps it. The period travels only for a timer, exactly as
    /// the core reads it (`rules.rs:160-164`).
    static func ruleWireObject(_ rule: DuckRule) -> [String: Any] {
        var item: [String: Any] = [
            "trigger": rule.trigger,
            "action": rule.action,
            "value": rule.value,
        ]
        if rule.trigger == "timer", let periodS = rule.periodS {
            item["period_s"] = periodS
        }
        if !rule.then.isEmpty {
            item["then"] = rule.then.map {
                ["action": $0.action, "value": $0.value]
            }
        }
        return item
    }

    /// The exact POST /rules body, and the set a backup carries.
    static func rulesPayload(_ rules: [DuckRule]) -> [String: Any] {
        ["rules": rules.map { Self.ruleWireObject($0) }]
    }

    /// The share document: a block program travels between phones as text
    /// (share sheet, message, clipboard). The version tag lets a future
    /// format change without guessing; the rules are the exact wire shape
    /// the core accepts, so an imported program runs unchanged.
    static func shareText(_ rules: [DuckRule]) -> String {
        var payload = rulesPayload(rules)
        payload["microduck-rules"] = 1
        guard let data = try? JSONSerialization.data(withJSONObject: payload),
              let text = String(data: data, encoding: .utf8)
        else { return "" }
        return text
    }

    /// A wire `period_s` as whole seconds, or nil for anything JSON cannot
    /// honestly call a whole number inside the core's own 5...3600 window.
    static func rulePeriod(_ raw: Any?) -> Int? {
        guard let number = raw as? NSNumber else { return nil }
        let seconds = number.doubleValue
        guard seconds.rounded() == seconds,
              seconds >= Double(rulePeriodBounds.lowerBound),
              seconds <= Double(rulePeriodBounds.upperBound)
        else { return nil }
        return Int(seconds)
    }

    /// Read one rule as the running core published it. The core is the
    /// authority here, so the shell translates the shape without applying
    /// the editor's vocabulary; a timer keeps the period the core stores.
    static func runningRule(fromWire item: [String: Any]) -> DuckRule? {
        guard let trigger = item["trigger"] as? String,
              let action = item["action"] as? String,
              let raw = item["value"] as? String
        else { return nil }
        let value = raw.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !value.isEmpty else { return nil }
        let periodS = trigger == "timer" ? rulePeriod(item["period_s"]) : nil
        let then = (item["then"] as? [[String: Any]] ?? [])
            .prefix(4)
            .compactMap { step -> DuckRuleStep? in
                guard let action = step["action"] as? String,
                      let raw = step["value"] as? String
                else { return nil }
                let value = raw.trimmingCharacters(in: .whitespacesAndNewlines)
                guard !value.isEmpty else { return nil }
                return DuckRuleStep(action: action, value: value)
            }
        return DuckRule(
            trigger: trigger,
            action: action,
            value: value,
            then: then,
            periodS: periodS
        )
    }

    /// The blocks a pasted document really yields, and how many timer
    /// blocks its sender wrote with a period the core would refuse.
    struct SharedRulesImport {
        let rules: [DuckRule]
        let skippedTimers: Int
    }

    /// Read a shared program from pasted text: either the versioned
    /// document or a bare rules array. The names must be in the core's
    /// trigger list and the editor's action list; a timer keeps its
    /// `period_s`, because the core stores and re-serialises that field.
    /// A timer outside the core's 5...3600 window is dropped and counted —
    /// sending it would make the core refuse the whole program — and the
    /// sheet names that refusal instead of importing a silently shorter
    /// program. nil means the text was not a program at all.
    static func parseSharedRules(_ text: String) -> SharedRulesImport? {
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard let data = trimmed.data(using: .utf8),
              let root = try? JSONSerialization.jsonObject(with: data)
        else { return nil }
        let list: [[String: Any]]
        if let array = root as? [[String: Any]] {
            list = array
        } else if let doc = root as? [String: Any],
                  doc["microduck-rules"] as? Int == 1,
                  let array = doc["rules"] as? [[String: Any]] {
            list = array
        } else {
            return nil
        }
        var imported: [DuckRule] = []
        var skippedTimers = 0
        for item in list {
            guard let trigger = item["trigger"] as? String,
                  let action = item["action"] as? String,
                  let raw = item["value"] as? String,
                  ruleTriggers.contains(trigger),
                  ruleActions.contains(action)
            else { continue }
            // The core trims with Rust's `trim()`, which knows the whole
            // Unicode White_Space set; `.whitespaces` does not include a
            // newline, and a value the core would refuse is not a block.
            let value = raw.trimmingCharacters(in: .whitespacesAndNewlines)
            guard !value.isEmpty else { continue }
            var periodS: Int?
            if trigger == "timer" {
                guard let seconds = rulePeriod(item["period_s"]) else {
                    skippedTimers += 1
                    continue
                }
                periodS = seconds
            }
            let then = (item["then"] as? [[String: Any]] ?? [])
                .prefix(4)
                .compactMap { step -> DuckRuleStep? in
                    guard let action = step["action"] as? String,
                          let raw = step["value"] as? String,
                          ruleActions.contains(action)
                    else { return nil }
                    let value = raw.trimmingCharacters(in: .whitespacesAndNewlines)
                    guard !value.isEmpty else { return nil }
                    return DuckRuleStep(action: action, value: value)
                }
            imported.append(DuckRule(
                trigger: trigger,
                action: action,
                value: value,
                then: then,
                periodS: periodS
            ))
        }
        return SharedRulesImport(rules: imported, skippedTimers: skippedTimers)
    }

    /// The block program the core is running. A GET, because the core is the
    /// authority; the local copy is only the editor's working set.
    func loadRules() {
        guard let url = URL(string: "http://127.0.0.1:8765/rules"),
              let data = try? Data(contentsOf: url),
              let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              let summary = json["rules"] as? [String: Any],
              let list = summary["rules"] as? [[String: Any]]
        else {
            if let data = UserDefaults.standard.data(forKey: Self.rulesKey),
               let saved = try? JSONDecoder().decode([DuckRule].self, from: data) {
                rules = saved
            }
            return
        }
        rules = list.compactMap { Self.runningRule(fromWire: $0) }
    }

    /// Replace the set in the core and on disk. The core validates; an
    /// invalid block comes back refused rather than run.
    func saveRules(_ next: [DuckRule]) {
        guard next.count <= 8 else { return }
        let previous = rules
        rules = next
        let payload = Self.rulesPayload(next)
        guard let body = try? JSONSerialization.data(withJSONObject: payload) else { return }
        // The core is the authority. It stays off the main thread, so the
        // answer comes back on a worker; a refusal puts the old program back
        // and says why, because an editor showing blocks the duck is not
        // running is an editor telling a lie.
        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            let accepted = Self.postSyncData("/rules", body)
            DispatchQueue.main.async {
                guard let self else { return }
                if accepted {
                    if let data = try? JSONEncoder().encode(next) {
                        UserDefaults.standard.set(data, forKey: Self.rulesKey)
                    }
                    self.rulesError = ""
                } else if self.rules == next {
                    // Only if this is still the program the editor shows: a
                    // newer edit has its own answer on the way.
                    self.rules = previous
                    self.rulesError = Copy.rulesRefusedText
                }
            }
        }
    }

    // MARK: - Sharing the block program

    /// The share document, built from the same wire shape the core accepts.
    var rulesShareText: String {
        Self.shareText(rules)
    }

    /// What a paste did: how many blocks it carried, how many were actually
    /// added to the child's own set (duplicates and the eight-block cap can
    /// leave that lower), and how many timer blocks it skipped.
    struct RulesImportResult {
        let parsed: Int
        let added: Int
        let skippedTimers: Int
    }

    /// Read a shared program from pasted text and merge it into the child's
    /// own. nil means the text was not a program. A timer with an illegal
    /// period is not silent: it is left out and counted in `skippedTimers`,
    /// and the sheet names that refusal. The rest of a legal program still
    /// imports, the same rule the Android shell follows.
    @discardableResult
    func importRules(from text: String) -> RulesImportResult? {
        guard let imported = Self.parseSharedRules(text) else { return nil }
        guard !imported.rules.isEmpty else {
            // Nothing legal to merge; the sheet still has the timer count
            // to report, and the core is not sent an unchanged program.
            return RulesImportResult(
                parsed: 0,
                added: 0,
                skippedTimers: imported.skippedTimers
            )
        }
        var next = rules
        var added = 0
        for rule in imported.rules {
            if next.count >= 8 { break }
            // The period is part of a timer's identity: a 30 s timer and a
            // 60 s timer with the same action are two different blocks.
            let exists = next.contains {
                $0.trigger == rule.trigger && $0.action == rule.action
                    && $0.value == rule.value && $0.periodS == rule.periodS
            }
            if !exists {
                next.append(rule)
                added += 1
            }
        }
        saveRules(next)
        return RulesImportResult(
            parsed: imported.rules.count,
            added: added,
            skippedTimers: imported.skippedTimers
        )
    }

    // MARK: - The diary

    /// Today's record as the diary page shows it: the real event lines, and
    /// the paragraph the person's own model writes from them.
    @Published var diaryLines: [String] = []
    @Published var diaryModelText = ""
    @Published var diaryError: String?
    @Published var diaryBusy = false
    private var diaryLoaded = false

    /// Load the diary's facts from the duck's own record; ask the configured
    /// model to tell the day in its own words. Without a model the facts
    /// stand alone — nothing is fabricated.
    func loadDiary(force: Bool = false) {
        if diaryBusy || (diaryLoaded && !force) { return }
        diaryBusy = true
        diaryLoaded = true
        diaryError = nil
        diaryModelText = ""
        guard let url = URL(string: "http://127.0.0.1:8765/state") else {
            diaryBusy = false
            return
        }
        var request = URLRequest(url: url)
        request.timeoutInterval = 2.0
        URLSession.shared.dataTask(with: request) { [weak self] data, _, _ in
            guard let self else { return }
            let json = data.flatMap {
                try? JSONSerialization.jsonObject(with: $0) as? [String: Any]
            } ?? [:]
            let life = json["life"] as? [String: Any] ?? [:]
            let events = life["events"] as? [String] ?? []
            let care = json["care"] as? [String: Any] ?? [:]
            func count(_ name: String) -> Int { events.filter { $0 == name }.count }
            func prefix(_ head: String) -> Int { events.filter { $0.hasPrefix(head) }.count }
            var lines: [String] = []
            let pets = count("pet")
            let feeds = count("feed")
            let rituals = count("idle_ritual")
            let falls = events.filter(LifeEvent.isFall).count
            let moods = prefix("emotion:")
            let gifts = prefix("gift:")
            if pets > 0 { lines.append(Copy.awayPets(pets)) }
            if feeds > 0 { lines.append(Copy.awayFeeds(feeds)) }
            if rituals > 0 { lines.append(Copy.awayRituals(rituals)) }
            if falls > 0 { lines.append(Copy.awayFalls(falls)) }
            if moods > 0 { lines.append(Copy.awayMoods(moods)) }
            if gifts > 0 {
                lines.append(Copy.diaryGifts(gifts))
            }
            let metres = life["metres"] as? Double ?? 0
            if metres >= 0.5 { lines.append(Copy.postcardMetres(metres)) }
            let facts = """
            events: \(events.suffix(40).joined(separator: ","))
            pets=\(pets) feeds=\(feeds) rituals=\(rituals) falls=\(falls) moods=\(moods) gifts=\(gifts)
            metres=\(String(format: "%.1f", metres)) hunger=\(care["hunger"] ?? 0) mood=\(care["mood"] ?? 0)
            Write in \(Copy.language == .chinese ? "Chinese" : "English").
            """
            let hasModel = self.aiConfig.enabled
                && !self.aiConfig.endpoint.trimmingCharacters(in: .whitespaces).isEmpty
            DispatchQueue.main.async {
                self.diaryLines = lines
                if !hasModel {
                    self.diaryBusy = false
                    return
                }
                self.aiBrain.writeDiary(config: self.aiConfig, dayFacts: facts) { text, error in
                    DispatchQueue.main.async {
                        self.diaryModelText = text ?? ""
                        self.diaryError = error
                        self.diaryBusy = false
                    }
                }
            }
        }.resume()
    }

    /// The app comes back: whatever the duck logged on its own - rituals,
    /// falls, feeds - becomes one line on the sheet. The 1 Hz poll is
    /// suspended in the background, so fetch the current events first;
    /// otherwise the diff would always be empty.
    func appBecameActive() {
        // The OS deactivates the audio session while the app is suspended.
        // Without this, the first background trip muted the duck until the
        // next launch — a real phone behaviour the simulator does not show.
        voice.resume()
        guard let url = URL(string: "http://127.0.0.1:8765/state") else { return }
        var request = URLRequest(url: url)
        request.timeoutInterval = 2.0
        URLSession.shared.dataTask(with: request) { [weak self] data, _, _ in
            guard let self,
                  let data,
                  let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
                  let life = json["life"] as? [String: Any],
                  let events = life["events"] as? [String]
            else { return }
            DispatchQueue.main.async {
                self.lastEvents = events
                self.summarizeAway()
            }
        }.resume()
    }

    private func summarizeAway() {
        let known = Set(eventsAtBackground)
        let fresh = lastEvents.filter { !known.contains($0) }
        guard !fresh.isEmpty else { return }
        var parts: [String] = []
        let rituals = fresh.filter { $0 == "idle_ritual" }.count
        let falls = fresh.filter(LifeEvent.isFall).count
        let feeds = fresh.filter { $0 == "feed" }.count
        let pets = fresh.filter { $0 == "pet" }.count
        if rituals > 0 { parts.append(Copy.awayRituals(rituals)) }
        if falls > 0 { parts.append(Copy.awayFalls(falls)) }
        if feeds > 0 { parts.append(Copy.awayFeeds(feeds)) }
        if pets > 0 { parts.append(Copy.awayPets(pets)) }
        let moods = fresh.filter { $0.hasPrefix("emotion:") }
        if !moods.isEmpty {
            let names = Set(moods.map { String($0.dropFirst("emotion:".count)) })
            parts.append(Copy.awayMoods(names.count))
        }
        guard !parts.isEmpty else { return }
        awaySummary = parts.joined(separator: " · ")
    }

    /// The vet book as a real file in the app's temporary directory. The core
    /// writes the document; the shell only places it where a share sheet can
    /// pick it up. Returns nil rather than an empty file if the core is down.
    func vetbookFile() -> URL? {
        guard let url = URL(string: "http://127.0.0.1:8765/export/vetbook.md"),
              let data = try? Data(contentsOf: url),
              !data.isEmpty else { return nil }
        let target = FileManager.default.temporaryDirectory
            .appendingPathComponent("MicroDuck-vetbook.md")
        do {
            try data.write(to: target, options: .atomic)
            return target
        } catch {
            return nil
        }
    }

    /// The Director shutter: read the frame the screen is showing, ask the
    /// camera for the room, and put the duck in the room. With no camera the
    /// frame already has the studio behind it and that is the photo. The
    /// result is written to Documents/Photos before the preview appears.
    func captureDirectorPhoto() {
        guard !photoInFlight, let renderer else { return }
        photoInFlight = true
        renderer.snapshot { [weak self] duck in
            guard let self else { return }
            guard let duck else {
                self.photoInFlight = false
                return
            }
            let finish: (UIImage) -> Void = { [weak self] image in
                guard let self else { return }
                self.photoInFlight = false
                let url = self.writePhoto(image)
                self.capturedPhoto = Self.CapturedPhoto(image: image, url: url)
                ProgressStore.shared.record(.photo)
            }
            if self.cameraActive {
                self.camera.capture { [weak self] room in
                    guard let self else { return }
                    finish(Self.composite(room: room, duck: duck))
                }
            } else {
                finish(duck)
            }
        }
    }

    /// The camera photo behind, the duck frame over it. Both fill the screen
    /// the user saw, so the composite matches the viewfinder (screen-space,
    /// not a tracked AR projection — this duck has no camera pose). With no
    /// room (the still failed) the duck frame is transparent in Director, so
    /// paint the studio behind it rather than saving an empty alpha PNG.
    private static func composite(room: UIImage?, duck: UIImage) -> UIImage {
        let size = duck.size
        guard size.width > 0, size.height > 0 else { return duck }
        let format = UIGraphicsImageRendererFormat()
        format.scale = 1
        return UIGraphicsImageRenderer(size: size, format: format).image { context in
            if let room {
                let target = size.width / size.height
                let source = room.size.width / max(room.size.height, 1)
                var rect = CGRect(origin: .zero, size: size)
                if source > target {
                    let width = size.height * source
                    rect = CGRect(x: (size.width - width) / 2, y: 0, width: width, height: size.height)
                } else if source < target {
                    let height = size.width / source
                    rect = CGRect(x: 0, y: (size.height - height) / 2, width: size.width, height: height)
                }
                room.draw(in: rect)
            } else {
                let colors = [
                    UIColor(red: 0.055, green: 0.075, blue: 0.10, alpha: 1).cgColor,
                    UIColor(red: 0.09, green: 0.11, blue: 0.14, alpha: 1).cgColor,
                ] as CFArray
                if let gradient = CGGradient(
                    colorsSpace: CGColorSpaceCreateDeviceRGB(),
                    colors: colors,
                    locations: [0, 1]
                ) {
                    context.cgContext.drawLinearGradient(
                        gradient,
                        start: CGPoint(x: 0, y: 0),
                        end: CGPoint(x: 0, y: size.height),
                        options: []
                    )
                }
            }
            duck.draw(in: CGRect(origin: .zero, size: size))
        }
    }

    private func writePhoto(_ image: UIImage) -> URL? {
        guard let data = image.pngData() else { return nil }
        let files = FileManager.default
        // Caches, not Documents: a preview is a look, not a keepsake. What
        // the family keeps is copied into Documents/Postcards or the photo
        // library; an un-kept shot must not pile up in the iCloud backup.
        guard let caches = files.urls(for: .cachesDirectory, in: .userDomainMask).first
        else { return nil }
        let directory = caches.appendingPathComponent("Photos", isDirectory: true)
        try? files.createDirectory(at: directory, withIntermediateDirectories: true)
        // Housekeeping: yesterday's leftovers go, whatever dismissed them.
        let dayAgo = Date().addingTimeInterval(-86_400)
        if let stale = try? files.contentsOfDirectory(at: directory, includingPropertiesForKeys: nil) {
            for file in stale where (try? file.resourceValues(forKeys: [.contentModificationDateKey]))?
                .contentModificationDate.map({ $0 < dayAgo }) == true
            {
                try? files.removeItem(at: file)
            }
        }
        let formatter = DateFormatter()
        formatter.dateFormat = "yyyyMMdd-HHmmss"
        let target = directory
            .appendingPathComponent("MicroDuck-\(formatter.string(from: Date())).png")
        do {
            try data.write(to: target, options: .atomic)
            return target
        } catch {
            return nil
        }
    }

    /// A stroke of the hand: POST /pet. The core counts it, coos and stays
    /// lively a little longer; `/state` and the away recap read it back.
    func pet() {
        dance.record("pet", label: Copy.danceStepPet)
        ProgressStore.shared.record(.pet)
        guard let url = URL(string: "http://127.0.0.1:8765/pet") else { return }
        var request = URLRequest(url: url)
        request.httpMethod = "POST"
        request.timeoutInterval = 2.0
        request.httpBody = Data("{}".utf8)
        URLSession.shared.dataTask(with: request).resume()
    }

    /// A treat on the floor: POST /feed, the core runs the official
    /// ground-pick peck and counts it in the life record. Fire-and-forget;
    /// the sheet shows the skill when it starts.
    func feed() {
        dance.record("feed", label: Copy.feed)
        ProgressStore.shared.record(.feed)
        guard let url = URL(string: "http://127.0.0.1:8765/feed") else { return }
        var request = URLRequest(url: url)
        request.httpMethod = "POST"
        request.timeoutInterval = 2.0
        request.httpBody = Data("{}".utf8)
        URLSession.shared.dataTask(with: request).resume()
    }

    /// Sleep mode's two faces. POST /night: on puts the duck to bed on
    /// purpose; off is the morning, which clears the night and writes the
    /// vetbook note "morning". A pet or a real hand ends the night too, with
    /// its own note, so the next /state poll reports whichever came last.
    /// Fire-and-forget; `nightActive` updates when the poll reads it back.
    func postNight(on: Bool) {
        postJSON("/night", ["on": on])
    }

    /// The system voice reading the diary aloud. The jaw follows locally
    /// (zero-latency, same 90 ms time constant as a quack); the /mouth intent
    /// rides the speaker's onChange, so start, natural finish, cancel and the
    /// sheet's dismissal all leave the same state behind. The core keeps the
    /// honest overrides: asleep shuts the beak regardless.
    func speakDiary(_ text: String) {
        speaker.updateLanguage(Copy.language)
        speaker.speak(text)
    }

    func stopSpeaking() {
        speaker.stop()
    }

    /// Paint a route: POST /path with body-frame waypoints in metres (+x
    /// forward, +y left at the route's start). The core drops the first
    /// point (the duck's own origin), refuses a route over its caps without
    /// queueing, and otherwise hands it to the walk policy. Fire-and-forget:
    /// /state's `path` slice reports whether it started and how far it got.
    func postPath(points: [[Double]]) {
        guard let body = try? JSONSerialization.data(withJSONObject: ["points": points])
        else { return }
        post("/path", body)
    }

    /// Stop the painted route: POST /path/cancel. The core answers ok even
    /// when nothing is walking, so the chip needs no guard here. The flag
    /// keeps the ending from reading as a finish.
    func cancelPath() {
        pathCancelRequested = true
        post("/path/cancel", Data("{}".utf8))
    }

    func pollSensors() {
        reportPerformance()
        maybeThinkAI()
        guard let url = URL(string: "http://127.0.0.1:8765/sensors") else { return }
        var request = URLRequest(url: url)
        request.timeoutInterval = 1.5
        // The same 1 Hz poll reads the duck's behaviour: an emotion timeline
        // in progress, or the idle look-around's gain. A duck that stands
        // perfectly still reads as switched off, so the sheet says what it is
        // doing rather than leaving the user to guess.
        let life = URL(string: "http://127.0.0.1:8765/state").flatMap { url in
            var request = URLRequest(url: url)
            request.timeoutInterval = 1.5
            return request
        }
        if let life {
            URLSession.shared.dataTask(with: life) { [weak self] data, _, _ in
                guard let data,
                      let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any]
                else { return }
                DispatchQueue.main.async {
                    guard let self else { return }
                    if let emotion = json["emotion"] as? [String: Any],
                       let name = emotion["name"] as? String {                        let elapsed = emotion["elapsed_s"] as? Double ?? 0
                        let duration = emotion["duration_s"] as? Double ?? 0
                        self.behaviorSummary = String(format: "%@ %.1f/%.1fs", SkillLabel.of(name), elapsed, duration)
                    } else if let idle = json["idle"] as? [String: Any],
                              let takeover = idle["takeover"] as? Double, takeover > 0.1 {
                        self.behaviorSummary = Copy.lookingAround(Int((takeover * 100).rounded()))
                    } else {
                        self.behaviorSummary = "—"
                    }
                    self.parseLeds(json["leds"])
                    self.parsePath(json["path"])
                    self.parseChorus(json["chorus"])
                    if let board = json["quests"] as? [String: Any] {
                        let chinese = Copy.language == .chinese
                        let list = board["quests"] as? [[String: Any]] ?? []
                        self.quests = list.compactMap { item in
                            guard let id = item["id"] as? String else { return nil }
                            let title = (chinese ? item["title_zh"] : item["title_en"]) as? String
                                ?? item["title_en"] as? String ?? id
                            return QuestCard(
                                id: id,
                                title: title,
                                progress: min(max(item["progress"] as? Double ?? 0, 0), 1),
                                done: item["done"] as? Bool ?? false
                            )
                        }
                        self.questStreak = board["streak"] as? Int ?? 0
                        self.questDay = board["day"] as? String ?? ""
                        if !list.isEmpty {
                            UserDefaults.standard.set(board, forKey: Self.questKey)
                        }
                        // The phone owns the calendar: when its day has
                        // rolled over since the core was last told, tell it
                        // again, or the board never advances past launch day.
                        self.postQuestDayIfRollover()
                    }
                    if let event = json["sound_event"] as? [String: Any],
                       let kind = event["kind"] as? String,
                       let tick = event["tick"] as? Int {
                        if tick != self.lastSoundEventTick {
                            self.lastSoundEventTick = tick
                            self.roomSound = Copy.heardSound(kind)
                        }
                    }
                    if let rule = json["rule_event"] as? [String: Any],
                       let trigger = rule["trigger"] as? String,
                       let action = rule["action"] as? String,
                       let value = rule["value"] as? String,
                       let tick = rule["tick"] as? Int {
                        if tick != self.lastRuleTick {
                            self.lastRuleTick = tick
                            let shown = action == "sound" ? value : SkillLabel.of(value)
                            self.ruleNotice = "\(Copy.ruleTrigger(trigger)) → \(shown)"
                        }
                    }
                    if let peers = json["peers"] as? [String: Any] {
                        self.peerName = peers["name"] as? String ?? self.peerName
                        let list = peers["peers"] as? [[String: Any]] ?? []
                        self.peers = Self.peerFixtureOn
                            ? [PeerInfo(
                                id: "fixture", name: "Sunny", replied: false,
                                paired: true, addr: "192.0.2.10:8765"
                            )]
                            : list.compactMap { item in
                            guard let id = item["id"] as? String,
                                  let name = item["name"] as? String
                            else { return nil }
                            return PeerInfo(
                                id: id,
                                name: name,
                                replied: item["replied"] as? Bool ?? false,
                                paired: item["paired"] as? Bool ?? false,
                                addr: item["addr"] as? String ?? ""
                            )
                        }
                    }
                    if let pad = json["pad"] as? [String: Any] {
                        self.netPadOn = pad["on"] as? Bool ?? false
                    }
                    if let event = json["peer_event"] as? [String: Any],
                       let kind = event["kind"] as? String,
                       let name = event["name"] as? String,
                       let tick = event["tick"] as? Int {
                        if tick != self.lastPeerTick {
                            self.lastPeerTick = tick
                            self.peerNotice = kind == "greeted"
                                ? Copy.peerGreeted(name)
                                : Copy.peerReplied(name)
                        }
                    }
                    // A null gift_event (nobody has sent one this session) is
                    // ignored; the last pill simply stays in memory. Only the
                    // two real gifts raise a line: an unknown kind is not
                    // invented.
                    if let event = json["gift_event"] as? [String: Any],
                       let kind = event["kind"] as? String,
                       kind == "treat" || kind == "emotion",
                       let tick = event["tick"] as? Int {
                        if tick != self.lastGiftTick {
                            self.lastGiftTick = tick
                            let name = event["from_name"] as? String ?? "Duck"
                            // A mood the duck was too busy to play still
                            // arrived: say what happened, not what did not.
                            self.giftLine = kind == "emotion"
                                ? (event["ok"] as? Bool == true
                                    ? Copy.giftReceivedEmotion(name)
                                    : Copy.giftMissedEmotion(name))
                                : Copy.giftReceivedTreat(name)
                            self.giftTick += 1
                            // The wall credits a gift only on the core's own
                            // verdict (ok=true: it ate, it played the mood);
                            // giftPeer is fire-and-forget, so the tap proves
                            // nothing and must never be the trigger (FE-2).
                            ProgressStore.shared.recordPeerGift(
                                ok: event["ok"] as? Bool == true
                            )
                        }
                    }
                    // Sleep mode's nightly slice sits beside care in the
                    // same publish; parse it before the care block so the
                    // wake edge is seen exactly once.
                    if let night = json["night"] as? [String: Any] {
                        self.parseNight(night)
                    }
                    if let care = json["care"] as? [String: Any] {
                        self.careHunger = care["hunger"] as? Double ?? 0
                        self.careMood = care["mood"] as? Double ?? 0
                        self.careBond = care["bond"] as? Double ?? 0
                        self.careAsleep = care["asleep"] as? Bool ?? false
                        // The palm reads the same numbers the capsules do.
                        self.heartBpm = care["heart_bpm"] as? Double ?? 0
                        // The parent's day, as the core keeps it.
                        if let parent = care["parent"] as? [String: Any] {
                            self.parentMinutes = parent["minutes"] as? Int
                            self.parentLeftS = parent["left_s"] as? Double ?? 0
                        }
                        self.saveCompanionState(care: care, life: json["life"] as? [String: Any])
                    }
                    if let life = json["life"] as? [String: Any] {
                        let emotions = life["emotions"] as? [String: Any] ?? [:]
                        let skills = life["skills"] as? [String: Any] ?? [:]
                        self.lastEvents = life["events"] as? [String] ?? self.lastEvents
                        self.updateDex(skills: skills, emotions: emotions)
                        self.vetbookMetres = life["metres"] as? Double ?? 0
                        // The parent's per-day rows, newest last.
                        if let days = life["days"] as? [[String: Any]] {
                            self.vetbookDays = days.prefix(30).map { row in
                                let day = row["day"] as? String ?? ""
                                let metres = row["metres"] as? Double ?? 0
                                let powered = row["powered_s"] as? Double ?? 0
                                let rolls = row["rolls"] as? Int ?? 0
                                return String(
                                    format: "%@: %.1fm · %ds · %d rolls",
                                    day, metres, Int(powered), rolls
                                )
                            }
                        }
                        if Self.dayFixtureOn {
                            // Same shape the core publishes; two synthetic
                            // rows only for the view test, never shown without
                            // the launch argument.
                            self.vetbookDays = [
                                "2026-11-14: 6.0m · 120s · 2 rolls",
                                "2026-11-15: 3.5m · 60s · 1 roll",
                            ]
                        }
                        self.vetbookRolls = skills["roulade"] as? Int ?? 0
                        self.vetbookPickups = life["pickups"] as? Int ?? 0
                        // 成长系统：散步按增量记账，停走一条 poll 后结一次。
                        let metres = self.vetbookMetres
                        if let base = self.walkBaselineMetres {
                            let delta = metres - base
                            if delta > 0.05 {
                                self.walkSessionMetres += Int(delta.rounded())
                                self.walkBaselineMetres = metres
                            } else if self.walkSessionMetres > 0 {
                                let session = self.walkSessionMetres
                                self.walkSessionMetres = 0
                                self.walkBaselineMetres = metres
                                ProgressStore.shared.record(.walk, metric: session)
                            } else {
                                self.walkBaselineMetres = metres
                            }
                        } else {
                            self.walkBaselineMetres = metres
                        }
                        // 翻滚：只记首读之后新增的次数。
                        let rolls = self.vetbookRolls
                        if let last = self.lastRecordedRolls {
                            if rolls > last {
                                for _ in 0..<min(rolls - last, 10) {
                                    ProgressStore.shared.record(.trick)
                                }
                            }
                            self.lastRecordedRolls = rolls
                        } else {
                            self.lastRecordedRolls = rolls
                        }
                        self.trickBook = String(
                            format: "%d/13 moods · %d tricks",
                            emotions.count,
                            skills.reduce(0) { $0 + ($1.value as? Int ?? 0) }
                        )
                    }
                }
            }.resume()
        }
        URLSession.shared.dataTask(with: request) { [weak self] data, _, _ in
            guard let data,
                  let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any]
            else { return }
            DispatchQueue.main.async {
                guard let self else { return }
                self.sensorMode = json["mode"] as? String ?? "twin"
                // A refused camera has no reading; a refused microphone has
                // no reading either. Both are permission answers, not absent
                // sensors. Depth has no app-owned feed yet: it shares the
                // camera's permission on LiDAR hardware only, so on a device
                // without LiDAR the plain "—" stays honest.
                self.cameraSummary = self.cameraDenied
                    ? Copy.denied
                    : Self.summary(json["camera"], keys: ["width", "height", "exposure_s"])
                self.microphoneSummary = self.micDenied
                    ? Copy.denied
                    : Self.summary(json["microphone"], keys: ["rms", "peak_hz"])
                self.depthSummary = Self.hasLiDAR && self.cameraDenied
                    ? Copy.denied
                    : Self.summary(json["depth"], keys: ["rows", "cols", "min_m", "max_m"])
                self.imuSummary = Self.summary(json["imu"], keys: ["acceleration"])
                // Keep the existing microphoneSummary parsing working as the
                // live feed's source of truth; the sheet shows it only when on.
                self.refreshMicSummary()
            }
        }.resume()
    }

    /// The virtual face, from the `leds` object the /state fetch already
    /// carries. Runs on the main thread inside the poll's completion.
    private func parseLeds(_ leds: Any?) {
        guard let leds = leds as? [String: Any] else {
            // The core predates the face: keep "—" rather than a stale
            // verdict, and leave the flashlight alone.
            faceStatus = "—"
            faceGauge = "—"
            faceBattery = "—"
            faceBars = 0
            return
        }
        faceStatus = leds["status"] as? String ?? "—"
        faceGauge = leds["gauge"] as? String ?? "—"
        faceBars = leds["bars"] as? Int ?? 0
        if let percent = leds["battery_percent"] as? Double {
            faceBattery = String(format: "%.0f%%", percent)
        } else {
            faceBattery = "—"
        }
        // The colour travels with the flashlight state: off publishes no
        // colour, so `nil` here *is* the off answer.
        flashlightColor = leds["color"] as? String
    }

    /// The Path Painter's slice of /state: the route's progress in the body
    /// frame the shell posted. A core without the feature publishes no
    /// `path` object; zeros are the same honest answer as an idle route.
    private func parsePath(_ path: Any?) {
        guard let path = path as? [String: Any],
              path["active"] as? Bool == true
        else {
            // The walk ended on this publish: a Stop press stays quiet, a
            // route that ran out of waypoints is a finish worth naming.
            if pathWasActive {
                if pathCancelRequested {
                    pathCancelRequested = false
                } else {
                    pathDoneTick += 1
                }
            }
            pathWasActive = false
            pathActive = false
            pathReached = 0
            pathTotal = 0
            return
        }
        pathWasActive = true
        pathCancelRequested = false
        pathActive = true
        pathReached = path["reached"] as? Int ?? 0
        pathTotal = path["total"] as? Int ?? 0
    }

    /// The chorus slice of /state: the room's shared song. A core without the
    /// feature publishes no `chorus` object; that is the same honest answer
    /// as an inactive one. The inactive -> active edge raises the pill's
    /// tick, so a song announces itself once however long it plays.
    private func parseChorus(_ chorus: Any?) {
        guard let chorus = chorus as? [String: Any] else {
            chorusWasActive = false
            chorusActive = false
            chorusRole = ""
            chorusMoves = []
            chorusPeers = 0
            return
        }
        let active = chorus["active"] as? Bool ?? false
        chorusActive = active
        chorusRole = chorus["role"] as? String ?? ""
        chorusMoves = chorus["moves"] as? [String] ?? []
        chorusPeers = chorus["peers"] as? Int ?? 0
        if active && !chorusWasActive {
            chorusTick += 1
        }
        chorusWasActive = active
    }

    /// The night's slice of /state: whether the chosen sleep is running and
    /// how long it has been quiet. An inactive answer publishes quiet_s 0,
    /// so the wake pill's minutes come from the last active publish, held
    /// here for the single transition poll that follows. The wake tick only
    /// fires when that held stretch was real: a duck that was put to bed and
    /// taken out again within the same tick has no morning to announce.
    private func parseNight(_ night: [String: Any]) {
        let active = night["active"] as? Bool ?? false
        let quiet = night["quiet_s"] as? Double ?? 0
        if nightWasActive && !active && lastNightQuietSeconds > 0 {
            nightWakeMinutes = Int(lastNightQuietSeconds / 60)
            nightWakeTick += 1
        }
        nightWasActive = active
        lastNightQuietSeconds = active ? quiet : 0
        nightActive = active
        nightQuietSeconds = quiet
    }

    /// `robot.flashlight` on a phone: a press toggles, in white. The core is
    /// not thread-safe, so this runs on the main thread like every other
    /// core call; the FFI returns the new state (0 off, 1..7 colour index+1),
    /// which publishes faster than waiting a second for /state.
    func toggleFlashlight() {
        guard let core else { return }
        func toggle() {
            let state = "white".withCString { pointer in
                duck_core_flashlight(core, 1, 0, pointer)
            }
            flashlightColor = Self.flashlightColorName(state)
        }
        if Thread.isMainThread {
            toggle()
        } else {
            DispatchQueue.main.async(execute: toggle)
        }
    }

    /// A dance step's light: "off" or one of the seven official colours,
    /// through the same client intent any rule uses. The lamp is the duck's
    /// own LED; a choreography may light it between two quacks.
    func setFlashlight(color: String) {
        postJSON("/flashlight", ["toggle": false, "on": color != "off", "color": color])
    }

    /// `robotctl wheee` on a phone: holding starts the ride's segments,
    /// releasing ends it. The core is main-thread-only, and the FFI itself
    /// ignores a repeated hold/release, so no shell-side state is needed.
    func setWheee(_ held: Bool) {
        guard let core else { return }
        func apply() {
            duck_core_wheee(core, held ? 1 : 0)
        }
        if Thread.isMainThread {
            apply()
        } else {
            DispatchQueue.main.async(execute: apply)
        }
    }

    /// The seven colours in `robotd`'s cycling order — the FFI's non-zero
    /// return is the position here plus one. The names stay the wire's, like
    /// the skill names: they are what /state publishes, not display text.
    private static func flashlightColorName(_ state: Int32) -> String? {
        let colors = ["white", "red", "green", "blue", "yellow", "cyan", "magenta"]
        guard state >= 1, Int(state) <= colors.count else { return nil }
        return colors[Int(state) - 1]
    }

    /// One row, three states: off shows "—", denied says so, on shows the
    /// last reading the core published. Off also clears a stale reading left
    /// by the previous second's POST.
    private func refreshMicSummary() {
        if micDenied {
            micSummary = Copy.denied
        } else if micEnabled && micAvailable {
            micSummary = microphoneSummary
        } else {
            micSummary = "—"
        }
    }

    /// The sheet's microphone toggle. Permission is asked only here, on the
    /// user's explicit enable — never at launch. An already-denied mic is not
    /// re-asked from the app (the OS refuses a second prompt); the sheet says
    /// "denied" and the user can change it in Settings.
    func setMicEnabled(_ enabled: Bool) {
        micEnabled = enabled
        if enabled && micDenied {
            refreshMicSummary()
            return
        }
        if enabled {
            mic.start()
        } else {
            mic.stop()
        }
        refreshMicSummary()
    }

    /// The sheet's voice-command toggle. The speech permission is asked only
    /// here, on the user's explicit enable — never at launch. The room's
    /// microphone toggle is a different switch: the ear and the loudness feed
    /// are independent, and either one keeps the one input tap alive.
    ///
    /// An already-refused recogniser is not re-asked from the app (the OS
    /// refuses a second prompt); the sheet says "denied" and the user can
    /// change it in Settings.
    func setVoiceCommands(_ enabled: Bool) {
        voiceCommandsEnabled = enabled
        guard enabled else {
            mic.stopSpeech()
            return
        }
        if speechState == .denied {
            // The OS will not ask again, so do not pretend it will.
            return
        }
        mic.startSpeech()
    }

    /// One measurement per second: frames per second the renderer achieved
    /// and control ticks per second against the 50 Hz target. Diagnostic only.
    private func reportPerformance() {
        guard let core else { return }
        let now = Date()
        let elapsed = now.timeIntervalSince(perfAnchor)
        // A window that spans app suspension, a mode switch or a long main
        // thread stall is not a frame rate: it would publish a false ~0 and
        // then an inflated catch-up number. Discard it and start fresh.
        guard elapsed > 0.5, elapsed < 2.0 else {
            frameCount = 0
            tickCount = 0
            perfAnchor = now
            return
        }
        duck_core_set_performance(core, Double(frameCount) / elapsed, Double(tickCount) / elapsed)
        frameCount = 0
        tickCount = 0
        perfAnchor = now
    }

    /// LiDAR is a hardware capability. Devices without it must keep showing
    /// "—" rather than "denied", which would blame the user for a sensor the
    /// phone does not have.
    private static let hasLiDAR = AVCaptureDevice.default(
        .builtInLiDARDepthCamera,
        for: .video,
        position: .back
    ) != nil

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
        if let pointer = duck_core_intent_token(core) {
            intentToken = String(cString: pointer)
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
        // The core is not thread-safe, so the voice pulls its next sound from
        // the same main-thread tick that steps MuJoCo. Only the PCM crosses
        // to the audio queue.
        voice.poll(core: core)
        // The beak follows the voice. robotd's chorus opens the mouth servo
        // through a 90 ms time constant; the shell uses the same one, so a
        // quack opens the jaw instead of snapping it open and shut.
        // The pad sender's stick is this shell's own joystick, read every
        // tick; a receiver-owned deadzone means raw values travel.
        padSender.stickX = joystick.x
        padSender.stickY = joystick.y
        let mouthTarget = (voice.isSpeaking || speaker.isSpeaking) ? 1.0 : 0.0
        visualMouth += (mouthTarget - visualMouth) * (1 - exp(-0.02 / 0.09))
        // The palm's beat: one tick checks whether the next beat of the
        // polled bpm is due. A disabled heart is one branch and no engine.
        heart.tick(pollBpm: heartBpm)
        if Voice.diagnosticsEnabled {
            // Five times a second is enough for the UI test's polling and
            // cheap enough for the control loop; the string is only built
            // when the launch argument asked for it.
            diagnosticFrames &+= 1
            if diagnosticFrames % 10 == 0 {
                voiceLine = voice.diagnostics
            }
        }

        // The core has robotd's 500 ms deadman: a twist must keep arriving.
        // The joystick is sent every tick, zero included.
        // robotd's pad: a 0.1 deadzone per axis, yaw negated (clockwise is
        // positive on the real controller).
        let x = deadzone(Float(joystick.x))
        let y = deadzone(Float(joystick.y))
        duck_core_set_twist(core, Double(-y) * 0.4, 0.0, Double(-x) * 1.0)
        // Paused or slowed: the timer keeps firing, simulation time does not.
        // duck_core_tick returns 0 even for a paused no-op, so count the
        // simulation clock's advance instead: at 0.5x that is 25 Hz, at 2x
        // 100 Hz, and a frozen duck reports 0 rather than a fake 50.
        var stepped = false
        let before = duck_core_tick_index(core)
        for _ in 0..<ticksForThisFrame() {
            if duck_core_tick(core) == 0 { stepped = true }
        }
        // A mode switch builds a fresh runtime whose clock starts at 0, so
        // the next frame's index is lower than `before`. Skip that boundary
        // frame: a UInt64 subtraction would trap and kill the app.
        let after = duck_core_tick_index(core)
        if after >= before {
            tickCount += Int(after - before)
        }
        // One snapshot per frame, after the steps: the renderer interpolates
        // between this and the previous one, so the duck moves at the display
        // rate rather than in 20 ms steps.
        if stepped {
            capturePoses(core: core)
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
                imuReady = status.imu_ready != 0
                pickedUp = status.picked_up != 0
                mouth = status.mouth
                volts = status.volts
                hasMouth = status.has_mouth != 0
                fps = status.fps
                ticksPerSecond = status.ticks_per_second
                if let pointer = duck_core_mode(core) {
                    driveMode = String(cString: pointer)
                }
            }
            lastStatus = Date()
            refreshClock()
        }
    }

    /// Switch drive bundles: "walk" or "roller". The whole policy set changes,
    /// so this is refused while a move is running.
    func setMode(_ mode: String) {
        guard let core else { return }
        _ = mode.withCString { duck_core_set_mode(core, $0) }
    }

    /// Enter or leave Director mode. Opening starts the camera; leaving stops
    /// it, so the indicator goes out and the sensor is released.
    func setDirector(_ on: Bool) {
        wantsCamera = on
        if on {
            camera.start()
        } else {
            camera.stop()
        }
        cameraActive = on && camera.available
    }

    func sit() {
        dance.record("sit", label: Copy.sit)
        guard let core else { return }
        _ = duck_core_sit_toggle(core)
    }

    /// Pick the duck up where it stands. The hand's anchor is where the trunk
    /// is at this instant; `carry(dx:dy:dz:)` moves it from there.
    func grab() {
        guard let core else { return }
        carrying = true
        joystick = .zero
        duck_core_carry(core, 0, 0, 0, 1)
    }

    /// Set it down. The duck drops the last few centimetres, lands, and the
    /// policy takes it back — through the pickup classifier, not a flag.
    func release() {
        guard let core else { return }
        carrying = false
        duck_core_carry(core, 0, 0, 0, 0)
    }

    /// The pad's head stick: yaw and pitch, radians.
    func setHead(yaw: Double, pitch: Double) {
        guard let core else { return }
        duck_core_set_head(core, 0.0, pitch, yaw, 0.0)
    }

    /// The pad's body stick: crouch (z) and sideways lean (roll).
    func setBody(z: Double, roll: Double) {
        guard let core else { return }
        duck_core_set_body(core, z, roll, 0.0)
    }

    /// Let go: the pose snaps back to nominal and the body-active flag that
    /// zeroes the twist is cleared, which is the pad's B exit. `set_body` only
    /// *starts* a pose — clearing it needs the release entry, or driving is
    /// dead for the rest of the session.
    func releaseBody() {
        guard let core else { return }
        duck_core_release_body(core)
    }

    func carry(dx: Double, dy: Double, dz: Double) {
        guard let core, carrying else { return }
        duck_core_carry(core, dx, dy, dz, 1)
    }

    func groundPick() {
        guard let core else { return }
        _ = duck_core_ground_pick(core)
    }

    /// One verb from the skill ring. `sit`, `stand` and `ground_pick` are the
    /// loop's own; the rest are manifest skills, and the core refuses an
    /// unknown name rather than pretending it started.
    func trigger(_ name: String) {
        dance.record("skill", value: name, label: SkillLabel.of(name))
        guard let core else { return }
        let started = name.withCString { duck_core_trigger_skill(core, $0) }
        if started != 0 { lastSkill = name }
    }

    /// Remember the walk slot and hand it to the core now. No screen calls
    /// this yet: it is the shell-side twin of `robotctl policy load walk`
    /// and of `POST /config {"walk_policy": …}` for a tool driving this
    /// phone. The name travels verbatim — `"none"` included — and the core
    /// rules on it; a refusal stores nothing, and a blank name forgets the
    /// override. A taken name is persisted, and the reload that loads the
    /// set carrying that file resolves it.
    func setWalkPolicy(_ name: String) {
        if name.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            walkPolicies.forget()
            return
        }
        guard let body = try? JSONSerialization.data(
            withJSONObject: ["walk_policy": name]
        ) else { return }
        guard Self.postSyncData("/config", body) else {
            NSLog("duck: walk policy %@ refused by the core", name)
            return
        }
        walkPolicies.remember(name)
        // Reload only when the set the button would load carries the file;
        // otherwise leave the override in the core for the reload that
        // loads the set that owns it. Loading a set without the file would
        // drop the override as a stale key.
        let directory = policyReloadDirectory()
        if setHasWalkPolicy(name, in: directory) {
            reloadPolicies()
        }
    }

    /// Reload a user-supplied policy set from Documents/policies when it has a
    /// manifest, otherwise the bundled official set. Custom ONNX runs under the
    /// same 61-D observation contract as the robot. The core validates the set
    /// at load and refuses with a reason — which comes back here and is shown,
    /// because "loaded" for a load that never happened is a lie the next
    /// stride discredits.
    func reloadPolicies() {
        guard let core else { return }
        let directory = policyReloadDirectory()
        // The walk override is process memory, handed over by the startup
        // replay or setWalkPolicy; this load is where it resolves.
        let loaded = directory.withCString { duck_core_reload_policies(core, $0) }
        if loaded != 0 {
            if let reason = duck_core_last_error() {
                label = String(cString: reason)
            } else {
                label = "reload failed"
            }
        }
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
