import SwiftUI

/// The five small games (2026-11-25), inline on Play more like Echo and RPS.
///
/// Each one only asks the duck for verbs it actually has and reads only the
/// fields /state actually publishes: the kicks, the 13 official moods, the
/// today-vs-day_started deltas, and the room-sound classifier's events. No
/// game tallies a score the duck did not earn — the counters are the vet
/// book's own.

/// The 13 official robotd moods the core loads.
private let officialMoods = [
    "angry", "curious", "defiant", "devastated", "excited", "impatient",
    "laugh", "mmh", "mock", "no", "sad", "yes", "yes_fast",
]

/// Each mood's official timeline length, from upstream's keyframes.
private func moodDurationMs(_ name: String) -> Int {
    switch name {
    case "angry": return 2500
    case "curious": return 2900
    case "defiant": return 2100
    case "devastated": return 8500
    case "excited": return 3300
    case "impatient": return 2500
    case "laugh": return 2900
    case "mmh": return 1900
    case "mock": return 2500
    case "no": return 1700
    case "sad": return 7500
    case "yes": return 1400
    case "yes_fast": return 1400
    default: return 2500
    }
}

private func duckState() async -> [String: Any]? {
    guard let url = URL(string: "http://127.0.0.1:8765/state") else { return nil }
    var request = URLRequest(url: url)
    request.timeoutInterval = 2
    guard let (data, _) = try? await URLSession.shared.data(for: request),
          let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any]
    else { return nil }
    return json
}

private func duckEmotionPlaying() async -> Bool {
    (await duckState())?["emotion"] as? [String: Any] != nil
}

private func moodSeed(_ fingerprint: String, _ round: Int) -> UInt64 {
    var hash: UInt64 = 0xcbf29ce484222325
    for byte in fingerprint.utf8 {
        hash ^= UInt64(byte)
        hash = hash &* 0x100000001b3
    }
    return hash ^ UInt64(bitPattern: Int64(round &* 2_654_435_761))
}

private struct GameChip: View {
    let title: String
    let identifier: String
    var enabled = true
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            Text(title)
                .font(.duckBody.weight(.semibold))
                .frame(maxWidth: .infinity, minHeight: 48)
        }
        .buttonStyle(DuckPressStyle())
        .disabled(!enabled)
        .opacity(enabled ? 1 : 0.4)
        .accessibilityIdentifier(identifier)
    }
}

/// 左右开弓: alternate feet on the count; each correct tap is a real kick.
struct KickRally: View {
    @ObservedObject var controller: DuckController
    @State private var step = 0
    @State private var notice = ""

    private var leftTurn: Bool { step % 2 == 0 }
    private var status: String {
        step >= 6 ? Copy.rallyDone
            : Copy.rallyStep(step + 1) + " · " + (leftTurn ? Copy.rallyLeftTurn : Copy.rallyRightTurn)
    }

    private func tap(left: Bool) {
        guard left == leftTurn, step < 6 else {
            notice = Copy.rallyWrong
            return
        }
        guard controller.trigger(left ? "kick_left" : "kick_right") else {
            notice = Copy.rallyWrong
            return
        }
        notice = ""
        step += 1
        if step >= 6 { controller.trigger("excited") }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: DuckSpace.sm) {
            HStack {
                Text(Copy.rallyTitle).font(.duckLabel.weight(.semibold)).foregroundStyle(.white)
                Spacer(minLength: DuckSpace.sm)
                Text(notice.isEmpty ? status : notice)
                    .font(.duckCaption)
                    .foregroundStyle(.white.opacity(0.6))
                    .accessibilityIdentifier("rally-status")
            }
            DuckAdaptiveRow(spacing: DuckSpace.sm) {
                GameChip(title: Copy.rallyLeftTurn, identifier: "rally-left",
                         enabled: step < 6) { tap(left: true) }
                GameChip(title: Copy.rallyRightTurn, identifier: "rally-right",
                         enabled: step < 6) { tap(left: false) }
            }
            GameChip(title: Copy.rallyGo, identifier: "rally-start") {
                step = 0
                notice = ""
            }
        }
    }
}

/// 情绪哑剧: the duck performs a real mood; the child names it.
struct MoodCharade: View {
    @ObservedObject var controller: DuckController
    @State private var answer = ""
    @State private var choices: [String] = []
    @State private var phase = 0 // 0 idle, 1 performing, 2 guessing, 3 answered
    @State private var outcome = ""
    @State private var round = 0

    private var status: String {
        switch phase {
        case 1: return "…"
        case 2: return Copy.charadesPrompt
        default: return outcome
        }
    }

    private func start() {
        round += 1
        let seed = moodSeed(controller.fingerprint, round)
        let index = Int(seed % UInt64(officialMoods.count))
        answer = officialMoods[index]
        let others = officialMoods.filter { $0 != answer }.shuffled()
        choices = (Array(others.prefix(3)) + [answer]).shuffled()
        outcome = ""
        guard controller.trigger(answer) else {
            phase = 0
            outcome = Copy.charadesWrong
            return
        }
        phase = 1
        // The guess opens when the mood's own timeline ends; the published
        // durations are the honest clock (polling /state can lag under load
        // and leave the chips disabled after the player already saw idle).
        Task { @MainActor in
            try? await Task.sleep(nanoseconds: UInt64(moodDurationMs(answer) + 300) * 1_000_000)
            phase = 2
        }
    }

    private func guess(_ mood: String) {
        guard phase == 2 else { return }
        let right = mood == answer
        controller.trigger(right ? "yes" : "no")
        outcome = right ? Copy.charadesRight : Copy.charadesWrong
        phase = 3
    }

    var body: some View {
        VStack(alignment: .leading, spacing: DuckSpace.sm) {
            HStack {
                Text(Copy.charadesTitle).font(.duckLabel.weight(.semibold)).foregroundStyle(.white)
                Spacer(minLength: DuckSpace.sm)
                Text(status)
                    .font(.duckCaption)
                    .foregroundStyle(.white.opacity(0.6))
                    .accessibilityIdentifier("charade-status")
            }
            DuckAdaptiveRow(spacing: DuckSpace.sm) {
                ForEach(Array(choices.prefix(2)), id: \.self) { mood in
                    GameChip(title: SkillLabel.of(mood), identifier: "charade-\(mood)",
                             enabled: phase == 2) { guess(mood) }
                }
            }
            DuckAdaptiveRow(spacing: DuckSpace.sm) {
                ForEach(Array(choices.dropFirst(2)), id: \.self) { mood in
                    GameChip(title: SkillLabel.of(mood), identifier: "charade-\(mood)",
                             enabled: phase == 2) { guess(mood) }
                }
            }
            GameChip(
                title: Copy.charadeGo,
                identifier: "charade-start",
                enabled: phase == 0 || phase == 3
            ) { start() }
        }
    }
}

/// 表情木头人: the duck calls the round with its own timelines.
struct MoodFreeze: View {
    @ObservedObject var controller: DuckController
    @State private var playing = false
    @State private var banner = ""

    private let sequence = ["excited", "angry", "excited", "yes"]

    private func start() {
        guard !playing else { return }
        playing = true
        Task { @MainActor in
            // The round clock is each mood's own timeline length (excited
            // 3.3 s, angry 2.5 s, yes 1.7 s) plus a breath. Polling /state
            // would let a slow fetch skip a mood and let other games'
            // emotions bleed in; the published durations are the honest
            // schedule.
            for mood in sequence {
                banner = mood == "excited" ? Copy.freezeMove : Copy.freezeStop
                _ = controller.trigger(mood)
                let ms: UInt64 = mood == "excited" ? 3600 : (mood == "angry" ? 2800 : 2000)
                try? await Task.sleep(nanoseconds: ms * 1_000_000)
            }
            banner = Copy.freezeDone
            playing = false
        }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: DuckSpace.sm) {
            HStack {
                Text(Copy.freezeTitle).font(.duckLabel.weight(.semibold)).foregroundStyle(.white)
                Spacer(minLength: DuckSpace.sm)
                Text(banner)
                    .font(.duckCaption.weight(.semibold))
                    .foregroundStyle(.white.opacity(0.75))
                    .accessibilityIdentifier("freeze-banner")
            }
            GameChip(title: Copy.freezeGo, identifier: "freeze-start", enabled: !playing) {
                start()
            }
        }
    }
}

/// 它今天有没有: the duck answers from its own day facts, with the number.
struct DidItHappenToday: View {
    @ObservedObject var controller: DuckController
    @State private var fact = ""

    private func ask(field: String) {
        Task { @MainActor in
            guard let json = await duckState(),
                  let life = json["life"] as? [String: Any] else { return }
            let started = life["day_started"] as? [String: Any]
            let today = life[field] as? Double ?? 0
            let base = started?[field] as? Double ?? 0
            let delta = max(today - base, 0)
            let yes = delta > 0
            controller.trigger(yes ? "yes" : "no")
            let word = yes ? Copy.todayYes : Copy.todayNo
            fact = word + " · " +
                (field == "metres" ? Copy.todayMetres(delta) : Copy.todayCount(Int(delta)))
        }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: DuckSpace.sm) {
            HStack {
                Text(Copy.todayTitle).font(.duckLabel.weight(.semibold)).foregroundStyle(.white)
                Spacer(minLength: DuckSpace.sm)
                Text(fact)
                    .font(.duckCaption)
                    .foregroundStyle(Color.duckBrass)
                    .accessibilityIdentifier("today-fact")
            }
            DuckAdaptiveRow(spacing: DuckSpace.sm) {
                GameChip(title: Copy.todayWalk, identifier: "today-walk") { ask(field: "metres") }
                GameChip(title: Copy.todayPets, identifier: "today-pets") { ask(field: "pets") }
            }
            DuckAdaptiveRow(spacing: DuckSpace.sm) {
                GameChip(title: Copy.todayFalls, identifier: "today-falls") { ask(field: "falls") }
                GameChip(title: Copy.todayFed, identifier: "today-fed") { ask(field: "fed") }
            }
        }
    }
}

/// 声音指挥官: the room answers with a clap or a whistle; the duck reacts.
struct SoundSimon: View {
    @ObservedObject var controller: DuckController
    @State private var status = ""
    @State private var listening = false

    private func listen(kind: String) {
        guard !listening else { return }
        listening = true
        status = kind == "clap" ? Copy.soundPromptLaugh : Copy.soundPromptCurious
        Task { @MainActor in
            let start = (await duckState())?["tick"] as? Int ?? 0
            var heard = false
            let deadline = Date().addingTimeInterval(25)
            while Date() < deadline {
                try? await Task.sleep(nanoseconds: 300_000_000)
                guard let json = await duckState(),
                      let event = json["sound_event"] as? [String: Any] else { continue }
                if (event["kind"] as? String) == kind,
                   (event["tick"] as? Int ?? 0) > start {
                    heard = true
                    break
                }
            }
            status = heard ? Copy.soundHeard : Copy.soundMissed
            listening = false
        }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: DuckSpace.sm) {
            HStack {
                Text(Copy.soundTitle).font(.duckLabel.weight(.semibold)).foregroundStyle(.white)
                Spacer(minLength: DuckSpace.sm)
                Text(status)
                    .font(.duckCaption)
                    .foregroundStyle(.white.opacity(0.6))
                    .accessibilityIdentifier("soundgame-status")
            }
            DuckAdaptiveRow(spacing: DuckSpace.sm) {
                GameChip(title: Copy.soundPromptLaugh, identifier: "sound-clap",
                         enabled: !listening) { listen(kind: "clap") }
                GameChip(title: Copy.soundPromptCurious, identifier: "sound-whistle",
                         enabled: !listening) { listen(kind: "whistle") }
            }
        }
    }
}

/// 反向指令: the prompt names a mood; the child must perform its opposite.
/// Emotions always start fresh (no idempotent already-in-state answer like
/// sit/stand), so every correct press is a real mood with a real counter.
struct ReverseCommand: View {
    @ObservedObject var controller: DuckController
    private let pairs: [(String, String)] = [
        ("laugh", "sad"), ("sad", "laugh"), ("excited", "curious"), ("curious", "excited"),
    ]
    private let faces = ["laugh", "sad", "excited", "curious"]
    @State private var round = 0
    @State private var notice = ""

    private var answer: String { pairs[round % pairs.count].1 }
    private var prompt: String { pairs[round % pairs.count].0 }

    private func press(_ mood: String) {
        guard round < 4 else { return }
        guard mood == answer else {
            notice = Copy.reverseWrong
            return
        }
        guard controller.trigger(mood) else {
            notice = Copy.reverseWrong
            return
        }
        notice = ""
        round += 1
    }

    var body: some View {
        VStack(alignment: .leading, spacing: DuckSpace.sm) {
            HStack {
                Text(Copy.reverseTitle).font(.duckLabel.weight(.semibold)).foregroundStyle(.white)
                Spacer(minLength: DuckSpace.sm)
                Text(notice.isEmpty ? (round >= 4 ? Copy.rallyDone : SkillLabel.of(prompt)) : notice)
                    .font(.duckCaption)
                    .foregroundStyle(.white.opacity(0.6))
                    .accessibilityIdentifier("reverse-status")
            }
            DuckAdaptiveRow(spacing: DuckSpace.sm) {
                ForEach(Array(faces.prefix(2)), id: \.self) { mood in
                    GameChip(title: SkillLabel.of(mood), identifier: "reverse-\(mood)") { press(mood) }
                }
            }
            DuckAdaptiveRow(spacing: DuckSpace.sm) {
                ForEach(Array(faces.dropFirst(2)), id: \.self) { mood in
                    GameChip(title: SkillLabel.of(mood), identifier: "reverse-\(mood)") { press(mood) }
                }
                GameChip(title: Copy.echoStart, identifier: "reverse-start") {
                    round = 0
                    notice = ""
                }
            }
        }
    }
}

/// 照我做: show one real pose at a time; the child makes the duck do it.
struct PostureMirror: View {
    @ObservedObject var controller: DuckController
    private let poses = ["sit", "stand", "ground_pick", "roulade"]
    @State private var round = 0
    @State private var notice = ""

    private func press(_ name: String) {
        guard round < poses.count else { return }
        guard name == poses[round] else {
            notice = Copy.reverseWrong
            return
        }
        let started = name == "ground_pick" ? controller.groundPick() : controller.trigger(name)
        guard started else {
            notice = Copy.reverseWrong
            return
        }
        notice = ""
        round += 1
    }

    var body: some View {
        VStack(alignment: .leading, spacing: DuckSpace.sm) {
            HStack {
                Text(Copy.postureTitle).font(.duckLabel.weight(.semibold)).foregroundStyle(.white)
                Spacer(minLength: DuckSpace.sm)
                Text(notice.isEmpty
                     ? (round >= poses.count ? Copy.rallyDone
                        : Copy.posturePrompt + " · " + SkillLabel.of(poses[round]))
                     : notice)
                    .font(.duckCaption)
                    .foregroundStyle(.white.opacity(0.6))
                    .accessibilityIdentifier("posture-status")
            }
            DuckAdaptiveRow(spacing: DuckSpace.sm) {
                GameChip(title: SkillLabel.of("sit"), identifier: "posture-sit") { press("sit") }
                GameChip(title: SkillLabel.of("stand"), identifier: "posture-stand") { press("stand") }
                GameChip(title: SkillLabel.of("ground_pick"), identifier: "posture-pick") {
                    press("ground_pick")
                }
            }
            DuckAdaptiveRow(spacing: DuckSpace.sm) {
                GameChip(title: SkillLabel.of("roulade"), identifier: "posture-roll") { press("roulade") }
                GameChip(title: Copy.echoStart, identifier: "posture-start") {
                    round = 0
                    notice = ""
                }
            }
        }
    }
}

/// 红灯停绿灯行: green keeps a real forward command going; red stops dead.
struct RedLightGreenLight: View {
    @ObservedObject var controller: DuckController
    @State private var green = false

    var body: some View {
        VStack(alignment: .leading, spacing: DuckSpace.sm) {
            HStack {
                Text(Copy.lightTitle).font(.duckLabel.weight(.semibold)).foregroundStyle(.white)
                Spacer(minLength: DuckSpace.sm)
                Text(green ? Copy.lightGreen : Copy.lightRed)
                    .font(.duckCaption)
                    .foregroundStyle(green ? Color.duckGood : .white.opacity(0.7))
                    .accessibilityIdentifier("light-status")
            }
            DuckAdaptiveRow(spacing: DuckSpace.sm) {
                GameChip(title: Copy.lightGreen, identifier: "light-green") {
                    green = true
                    drive()
                }
                GameChip(title: Copy.lightRed, identifier: "light-red") {
                    green = false
                    controller.joystick = .zero
                }
            }
        }
        .onChange(of: green) { _, on in
            if on { drive() } else { controller.joystick = .zero }
        }
    }

    /// The real velocity channel, one tick at a time, while the light is green.
    private func drive() {
        Task { @MainActor in
            while green {
                controller.joystick = CGPoint(x: 0, y: 0.4)
                try? await Task.sleep(nanoseconds: 100_000_000)
            }
            controller.joystick = .zero
        }
    }
}

/// 快进一天: the duck's own care clock at 60x for a minute of real life.
struct FastForwardDay: View {
    @ObservedObject var controller: DuckController
    @State private var fast = false

    var body: some View {
        VStack(alignment: .leading, spacing: DuckSpace.sm) {
            HStack {
                Text(Copy.fastTitle).font(.duckLabel.weight(.semibold)).foregroundStyle(.white)
                Spacer(minLength: DuckSpace.sm)
                Text(fast ? Copy.fastOn : Copy.fastNote)
                    .font(.duckCaption)
                    .foregroundStyle(.white.opacity(0.6))
                    .accessibilityIdentifier("fast-status")
            }
            GameChip(title: fast ? Copy.fastOff : Copy.fastOn, identifier: "fast-toggle") {
                fast.toggle()
                controller.setCareTimeScale(fast ? 60 : 1)
            }
        }
    }
}

/// 找一找: the prompt names a body part; the touch is the real reaction.
struct FindIt: View {
    @ObservedObject var controller: DuckController
    private let parts = ["head", "belly", "feet"]
    @State private var round = 0
    @State private var notice = ""

    private func title(_ part: String) -> String {
        switch part {
        case "head": return Copy.findHead
        case "belly": return Copy.findBelly
        default: return Copy.findFeet
        }
    }

    private func press(_ part: String) {
        guard round < parts.count, part == parts[round] else {
            notice = Copy.reverseWrong
            return
        }
        switch part {
        case "head": controller.trigger("curious")
        case "belly": controller.pet()
        default: controller.trigger("kick_left")
        }
        notice = ""
        round += 1
    }

    var body: some View {
        VStack(alignment: .leading, spacing: DuckSpace.sm) {
            HStack {
                Text(Copy.findTitle).font(.duckLabel.weight(.semibold)).foregroundStyle(.white)
                Spacer(minLength: DuckSpace.sm)
                Text(notice.isEmpty
                     ? (round >= parts.count ? Copy.rallyDone
                        : Copy.findPrompt + " · " + title(parts[round]))
                     : notice)
                    .font(.duckCaption)
                    .foregroundStyle(.white.opacity(0.6))
                    .accessibilityIdentifier("find-status")
            }
            DuckAdaptiveRow(spacing: DuckSpace.sm) {
                ForEach(parts, id: \.self) { part in
                    GameChip(title: title(part), identifier: "find-\(part)") { press(part) }
                }
            }
            GameChip(title: Copy.echoStart, identifier: "find-start") {
                round = 0
                notice = ""
            }
        }
    }
}

/// 小账本: today's real deltas, read straight from the duck's own book.
struct LittleLedger: View {
    @ObservedObject var controller: DuckController
    @State private var lines: [String] = []

    private func read() {
        Task { @MainActor in
            guard let json = await duckState(),
                  let life = json["life"] as? [String: Any] else { return }
            let day = life["day_started"] as? [String: Any]
            func delta(_ name: String) -> Double {
                max((life[name] as? Double ?? 0) - (day?[name] as? Double ?? 0), 0)
            }
            var tricks = 0
            if let skills = life["skills"] as? [String: Any] {
                for key in skills.keys { tricks += skills[key] as? Int ?? 0 }
            }
            lines = [
                Copy.ledgerWalk + " " + Copy.todayMetres(delta("metres")),
                Copy.ledgerPets + " " + Copy.todayCount(Int(delta("pets"))),
                Copy.ledgerFalls + " " + Copy.todayCount(Int(delta("falls"))),
                Copy.ledgerFed + " " + Copy.todayCount(Int(delta("fed"))),
                Copy.ledgerSkills + " " + Copy.todayCount(tricks),
            ]
            controller.trigger("yes")
        }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: DuckSpace.sm) {
            HStack {
                Text(Copy.ledgerTitle).font(.duckLabel.weight(.semibold)).foregroundStyle(.white)
                Spacer(minLength: DuckSpace.sm)
                GameChip(title: Copy.ledgerRefresh, identifier: "ledger-read") { read() }
            }
            ForEach(lines, id: \.self) { line in
                Text(line)
                    .font(.duckCaption)
                    .foregroundStyle(Color.duckBrass)
                    .accessibilityIdentifier("ledger-line")
            }
        }
    }
}

/// 传悄悄话: the duck whispers one of its voices; the child names it.
struct WhisperGame: View {
    @ObservedObject var controller: DuckController
    private let tags = ["chirp", "coo", "inquire", "peck"]
    @State private var target = ""
    @State private var phase = 0 // 0 idle, 1 guessing, 2 answered
    @State private var notice = ""

    private func listen() {
        let second = Int(Date().timeIntervalSince1970)
        target = tags[second % tags.count]
        notice = ""
        phase = 1
        controller.queueSound(target)
    }

    private func guess(_ tag: String) {
        guard phase == 1 else { return }
        let right = tag == target
        controller.trigger(right ? "yes" : "no")
        notice = right ? Copy.charadesRight : Copy.charadesWrong
        phase = 2
    }

    var body: some View {
        VStack(alignment: .leading, spacing: DuckSpace.sm) {
            HStack {
                Text(Copy.whisperTitle).font(.duckLabel.weight(.semibold)).foregroundStyle(.white)
                Spacer(minLength: DuckSpace.sm)
                Text(phase == 1 ? Copy.whisperPrompt : (phase == 2 ? notice : ""))
                    .font(.duckCaption)
                    .foregroundStyle(.white.opacity(0.6))
                    .accessibilityIdentifier("whisper-status")
            }
            DuckAdaptiveRow(spacing: DuckSpace.sm) {
                ForEach(Array(tags.prefix(2)), id: \.self) { tag in
                    GameChip(title: Copy.echoSound(tag), identifier: "whisper-\(tag)",
                             enabled: phase == 1) { guess(tag) }
                }
            }
            DuckAdaptiveRow(spacing: DuckSpace.sm) {
                ForEach(Array(tags.dropFirst(2)), id: \.self) { tag in
                    GameChip(title: Copy.echoSound(tag), identifier: "whisper-\(tag)",
                             enabled: phase == 1) { guess(tag) }
                }
                GameChip(title: phase == 0 ? Copy.whisperListen : Copy.whisperAgain,
                         identifier: "whisper-start") { listen() }
            }
        }
    }
}
