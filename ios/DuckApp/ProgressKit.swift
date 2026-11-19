import Foundation
import SwiftUI
import Combine

/// ProgressKit —— MicroDuck 的成长系统：每日任务、连续天数、成就墙。
///
/// 「每天有理由回来」。数据只有一条纪律：**事实永远是真的** —— 所有进度都
/// 来自 `record(_:)` 记到的真实事件，不做任何模拟、预填或倒推；成就解锁即
/// 落盘，永不回退。
///
/// =====================================================================
/// 接线说明（写给总调度：本文件是唯一新增文件，不改任何现有代码）
/// =====================================================================
///
/// 一、事件上报 —— `ProgressStore.shared.record(...)`
///
///   record 可从任意线程调用（内部串行队列保护落盘，@Published 主线程更新），
///   连续调用安全；`metric <= 0` 会被忽略。只有事件真实发生才调用，
///   启动时不要补记历史。
///
///   1. 摸摸它：`ContentView` 里 `controller.pet()` 成功后（手势与语音/无障碍
///      的 "pet" 两处，搜索 `controller.pet()` 定位）：
///          ProgressStore.shared.record(.pet)
///   2. 喂饭：喂食真正生效时（`controller.feed()` 的成功分支，deck 按钮 /
///      语音 / 菜单三个入口任选一处，别重复记）：
///          ProgressStore.shared.record(.feed)
///   3. 技能/翻滚：技能盘选中一个动作并真正开始执行时（DuckController 里
///      `duck_core_trigger_skill` 触发处）：
///          ProgressStore.shared.record(.trick)
///   4. 散步：**一次散步结束**调用一次，metric 传本次米数（odometry 差值）：
///          ProgressStore.shared.record(.walk, metric: meters)
///      注意 metric 必传；「一次散步 30 米」成就按单次调用判定，若按小段
///      频繁上报会把一次散步拆碎，语义失真 —— 请按散步会话上报。
///   5. 拍照明信片：明信片保存/保留成功时（PhotoPreviewSheet 的保存分支）：
///          ProgressStore.shared.record(.photo)
///   6. 邻居互送：送出或收到礼物成功时（PeersSheet 的 `controller.giftPeer`
///      chip 成功回调）：
///          ProgressStore.shared.record(.peer)
///
/// 二、弹出面板
///
///   ContentView 加一个入口（建议放拇指弧区，见 docs/UI_TOKENS.md §6）：
///       @State private var showProgress = false
///       Button { showProgress = true } label: { ... }
///       .sheet(isPresented: $showProgress) { ProgressSheet() }
///   若沿用现有 `.sheet(item: $modal)` 枚举模式，给 modal 枚举加一个 case，
///   分支里返回 ProgressSheet() 即可；面板自带 presentationDetents。
///
/// 三、可选的轻量入口（不接也不影响功能）
///
///   - 主界面小火苗/角标：`streak`（当前连续天数）、`bestStreak`（最长连续）、
///     `tasksDoneToday`（今日任务完成数 0...3）均为 @Published，可直接读。
///   - 新成就解锁瞬间：`latestUnlock` 会被置为最新解锁的成就（保持不清空），
///     可用 `.onChange(of: store.latestUnlock?.id ?? "")` 弹一个轻量 toast。
///
/// 四、存储与版本
///
///   Application Support/DuckProgress/progress.json，原子写入；JSON 顶层带
///   schemaVersion（当前 1），对不上或文件损坏则从零开始，旧文件在下次保存
///   时被覆盖。只保留最近 30 天的活跃日期明细；累计值、最值与成就永不裁剪。
///
/// 五、配色
///
///   语义色按 docs/UI_TOKENS.md §2.2 固化在本文件底部的 Color 扩展里；
///   DuckTheme.swift 落地后应改引用它，届时删除本文件内的私有扩展即可。

// MARK: - 事件

/// 一次真实发生的互动。rawValue 直接写进 JSON，改了就丢历史。
enum ProgressEvent: String, Codable, CaseIterable {
    case pet    // 摸
    case feed   // 喂饭
    case trick  // 技能/翻滚
    case walk   // 散步（metric = 米数，其余事件 metric = 次数）
    case photo  // 拍照明信片
    case peer   // 邻居互送
}

// MARK: - 存档

/// 磁盘上的全部进度。schemaVersion 对不上时整份作废重来。
private struct ProgressFile: Codable {
    var schemaVersion = 1
    /// 活跃日的当日累计，key 为本地自然日 "yyyy-MM-dd"，只保留最近 30 天。
    /// days 里有 key 即代表那天活跃（发生过任意事件）。
    var days: [String: DayRecord] = [:]
    /// 各事件历史累计（walk 的单位是米，其余是次数）。
    var totals: [String: Int] = [:]
    /// 单次散步最大米数（「一次散步 30 米」用）。
    var bestWalk = 0
    /// 历史最长连续天数。
    var bestStreak = 0
    /// 已解锁成就：id -> 解锁日 "yyyy-MM-dd"。只增不删。
    var unlocked: [String: String] = [:]
}

/// 一天里各事件的累计量。
private struct DayRecord: Codable {
    var amounts: [String: Int] = [:]
}

// MARK: - 日历条小格

struct DayTile: Identifiable {
    let id: String      // "yyyy-MM-dd"
    let weekday: String // "一" … "日"
    let active: Bool
    let isToday: Bool
}

// MARK: - 每日任务（每天固定三条）

struct DailyTask: Identifiable {
    let event: ProgressEvent
    let title: String
    let goal: Int
    let unit: String
    var id: String { event.rawValue }

    static let catalog: [DailyTask] = [
        DailyTask(event: .pet, title: "摸摸它", goal: 3, unit: "次"),
        DailyTask(event: .feed, title: "喂一次饭", goal: 1, unit: "次"),
        DailyTask(event: .walk, title: "散步", goal: 10, unit: "米"),
    ]
}

// MARK: - 进度仓库

final class ProgressStore: ObservableObject {
    static let shared = ProgressStore()

    // ---- UI 读的快照（全部主线程更新） ----
    /// 今日各事件累计（walk 是米数，其余是次数）。
    @Published private(set) var todayAmounts: [ProgressEvent: Int] = [:]
    /// 当前连续活跃天数（今天活跃则含今天；今天还没结束不算断）。
    @Published private(set) var streak = 0
    /// 历史最长连续天数。
    @Published private(set) var bestStreak = 0
    /// 今日任务完成数（0...3）。
    @Published private(set) var tasksDoneToday = 0
    /// 最近 7 天日历条（旧→新，含今天）。
    @Published private(set) var week: [DayTile] = []
    @Published private(set) var unlockedIDs: Set<String> = []
    @Published private(set) var unlockDates: [String: String] = [:]
    /// 最新解锁的成就（不清空，供可选 toast 用）。
    @Published private(set) var latestUnlock: Achievement?

    static let schemaVersion = 1

    private let queue = DispatchQueue(label: "ai.duck.progress")
    /// 只允许在 queue 上读写（init 除外）。
    private var file: ProgressFile

    /// Injectable for the unit tests: they pass a throwaway directory so
    /// one test's facts never leak into another's wall.
    init(directory: URL? = nil) {
        let url = (directory ?? Self.directory())?
            .appendingPathComponent("progress.json")
        if let url, let data = try? Data(contentsOf: url),
           let decoded = try? JSONDecoder().decode(ProgressFile.self, from: data),
           decoded.schemaVersion == Self.schemaVersion {
            file = decoded
        } else {
            // 首次启动、文件损坏或版本不符：从零开始（旧文件下次保存时覆盖）。
            file = ProgressFile()
        }
        apply(makeSnapshot())
    }

    /// A neighbour gift reaches the wall only when the core itself said
    /// ok — the duck actually ate the treat or played the mood. A refused,
    /// busy or throttled delivery (the tap happened, the gift did not)
    /// never becomes an achievement; that would be inventing a fact.
    func recordPeerGift(ok: Bool) {
        guard ok else { return }
        record(.peer)
    }

    /// 记一次真实事件。任意线程可调；metric 为本次累计量（散步传米数）。
    func record(_ event: ProgressEvent, metric: Int = 1) {
        guard metric > 0 else { return }
        queue.async {
            let now = Date()
            let key = Self.dayKey(now)
            // 当日累计
            var day = self.file.days[key] ?? DayRecord()
            day.amounts[event.rawValue, default: 0] += metric
            self.file.days[key] = day
            // 历史累计与最值
            self.file.totals[event.rawValue, default: 0] += metric
            if event == .walk, metric > self.file.bestWalk {
                self.file.bestWalk = metric
            }
            // 连续天数：今天刚被记活，从今天往前数
            let streakNow = self.streakCount(asOf: now)
            if streakNow > self.file.bestStreak {
                self.file.bestStreak = streakNow
            }
            // 成就：逐条检查还没解锁的，解锁即写入存档（永不回退）
            let facts = ProgressStore.Facts(
                totals: self.file.totals,
                bestWalk: self.file.bestWalk,
                bestStreak: self.file.bestStreak,
                isLateNight: (0...5).contains(Calendar.current.component(.hour, from: now))
            )
            var fresh: [Achievement] = []
            for achievement in Achievement.catalog where self.file.unlocked[achievement.id] == nil {
                if achievement.isMet(facts) {
                    self.file.unlocked[achievement.id] = key
                    fresh.append(achievement)
                }
            }
            self.pruneDays()
            self.save()
            let snapshot = self.makeSnapshot()
            DispatchQueue.main.async {
                self.apply(snapshot)
                if let unlockedNow = fresh.last {
                    self.latestUnlock = unlockedNow
                }
            }
        }
    }

    /// 跨天（或面板再次出现）时刷新快照；只重算，不落盘。
    func refreshDayIfNeeded() {
        queue.async {
            let snapshot = self.makeSnapshot()
            DispatchQueue.main.async { self.apply(snapshot) }
        }
    }

    // MARK: 成就判定的事实切片

    /// 判定成就那一刻的事实（不持有 store，纯值）。
    struct Facts {
        var totals: [String: Int]
        var bestWalk: Int
        var bestStreak: Int
        var isLateNight: Bool
        func total(_ event: ProgressEvent) -> Int { totals[event.rawValue] ?? 0 }
    }

    // MARK: 快照

    private struct Snapshot {
        var todayAmounts: [ProgressEvent: Int] = [:]
        var streak = 0
        var bestStreak = 0
        var tasksDoneToday = 0
        var week: [DayTile] = []
        var unlockedIDs: Set<String> = []
        var unlockDates: [String: String] = [:]
    }

    /// 只在 queue 上调用（file 只允许 queue 触碰）。
    private func makeSnapshot() -> Snapshot {
        let now = Date()
        let key = Self.dayKey(now)
        let day = file.days[key] ?? DayRecord()
        var amounts: [ProgressEvent: Int] = [:]
        for event in ProgressEvent.allCases {
            amounts[event] = day.amounts[event.rawValue] ?? 0
        }
        var snapshot = Snapshot()
        snapshot.todayAmounts = amounts
        snapshot.streak = streakCount(asOf: now)
        snapshot.bestStreak = file.bestStreak
        snapshot.tasksDoneToday = DailyTask.catalog.filter { (amounts[$0.event] ?? 0) >= $0.goal }.count
        snapshot.week = Self.weekTiles(now: now, days: file.days)
        snapshot.unlockedIDs = Set(file.unlocked.keys)
        snapshot.unlockDates = file.unlocked
        return snapshot
    }

    /// 主线程把快照铺到 @Published。
    private func apply(_ snapshot: Snapshot) {
        todayAmounts = snapshot.todayAmounts
        streak = snapshot.streak
        bestStreak = snapshot.bestStreak
        tasksDoneToday = snapshot.tasksDoneToday
        week = snapshot.week
        unlockedIDs = snapshot.unlockedIDs
        unlockDates = snapshot.unlockDates
    }

    // MARK: 日期与统计（均在 queue 上调用）

    /// 本地时区自然日的 key，"yyyy-MM-dd"。取不到分量时返回空串（只在
    /// 日期异常时可能发生，落进一个无害的空 key，绝不崩溃）。
    private static func dayKey(_ date: Date) -> String {
        let parts = Calendar.current.dateComponents([.year, .month, .day], from: date)
        guard let year = parts.year, let month = parts.month, let day = parts.day else { return "" }
        return String(format: "%04ld-%02ld-%02ld", year, month, day)
    }

    /// 当前连续活跃天数：今天活跃则从今天往前数，否则从昨天往前数
    /// （今天还没结束，连续不算断）；最近没有任何活跃日返回 0。
    private func streakCount(asOf now: Date) -> Int {
        let calendar = Calendar.current
        var day = calendar.startOfDay(for: now)
        if file.days[Self.dayKey(day)] == nil {
            guard let yesterday = calendar.date(byAdding: .day, value: -1, to: day) else { return 0 }
            day = yesterday
        }
        var count = 0
        while count < 400, file.days[Self.dayKey(day)] != nil {
            count += 1
            guard let previous = calendar.date(byAdding: .day, value: -1, to: day) else { break }
            day = previous
        }
        return count
    }

    /// 最近 7 天（含今天，旧→新）的日历条数据。
    private static func weekTiles(now: Date, days: [String: DayRecord]) -> [DayTile] {
        let calendar = Calendar.current
        let today = calendar.startOfDay(for: now)
        let weekdayNames = ["日", "一", "二", "三", "四", "五", "六"]
        var tiles: [DayTile] = []
        for offset in (0...6).reversed() {
            guard let date = calendar.date(byAdding: .day, value: -offset, to: today) else { continue }
            let key = dayKey(date)
            let index = calendar.component(.weekday, from: date) - 1
            let name = weekdayNames.indices.contains(index) ? weekdayNames[index] : "·"
            tiles.append(DayTile(id: key, weekday: name, active: days[key] != nil, isToday: offset == 0))
        }
        return tiles
    }

    /// 只保留最近 30 天的活跃明细（key 字典序即时间序）。
    private func pruneDays() {
        guard file.days.count > 30 else { return }
        var kept: [String: DayRecord] = [:]
        for key in file.days.keys.sorted().suffix(30) {
            kept[key] = file.days[key]
        }
        file.days = kept
    }

    // MARK: 落盘

    private static func directory() -> URL? {
        guard let base = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first else {
            return nil
        }
        let dir = base.appendingPathComponent("DuckProgress", isDirectory: true)
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        return dir
    }

    /// 原子写盘；失败只丢本次增量，内存里的事实还在，下次 record 再写。
    private func save() {
        guard let url = Self.directory()?.appendingPathComponent("progress.json") else { return }
        do {
            let data = try JSONEncoder().encode(file)
            try data.write(to: url, options: .atomic)
        } catch {
            NSLog("DuckProgress: 保存失败 %@", error.localizedDescription)
        }
    }
}

// MARK: - 成就

/// 判定成就那一刻的事实切片见 `ProgressStore.Facts`。
struct Achievement: Identifiable {
    let id: String
    let name: String   // 徽章名
    let detail: String // 未解锁时显示的达成条件一句话
    let icon: String   // SF Symbol
    let tint: Color    // docs/UI_TOKENS.md 语义色
    let isMet: (ProgressStore.Facts) -> Bool

    static let catalog: [Achievement] = [
        Achievement(id: "first_trick", name: "第一次翻滚", detail: "让它做一个技能动作",
                    icon: "sparkles", tint: .duckInfo,
                    isMet: { $0.total(.trick) >= 1 }),
        Achievement(id: "first_feed", name: "第一次喂食", detail: "喂它吃一顿饭",
                    icon: "fork.knife", tint: .duckAction,
                    isMet: { $0.total(.feed) >= 1 }),
        Achievement(id: "first_postcard", name: "第一张明信片", detail: "拍一张照片明信片",
                    icon: "camera", tint: .duckData,
                    isMet: { $0.total(.photo) >= 1 }),
        Achievement(id: "first_gift", name: "邻居礼物", detail: "和邻居互送一次礼物",
                    icon: "gift", tint: .duckWarn,
                    isMet: { $0.total(.peer) >= 1 }),
        Achievement(id: "streak_3", name: "连续 3 天", detail: "连续 3 天回来看它",
                    icon: "flame", tint: .duckAction,
                    isMet: { $0.bestStreak >= 3 }),
        Achievement(id: "streak_7", name: "连续 7 天", detail: "连续 7 天回来看它",
                    icon: "trophy", tint: .duckWarn,
                    isMet: { $0.bestStreak >= 7 }),
        Achievement(id: "walk_100", name: "累计散步 100 米", detail: "散步加起来走满 100 米",
                    icon: "figure.walk", tint: .duckGood,
                    isMet: { $0.total(.walk) >= 100 }),
        Achievement(id: "pet_100", name: "累计摸 100 次", detail: "摸它加起来满 100 次",
                    icon: "hand.raised", tint: .duckInfo,
                    isMet: { $0.total(.pet) >= 100 }),
        Achievement(id: "late_night", name: "深夜陪伴", detail: "在凌晨 0–5 点陪陪它",
                    icon: "moon.stars", tint: .duckData,
                    isMet: { $0.isLateNight }),
        Achievement(id: "walk_30_single", name: "一次散步 30 米", detail: "一口气散步走满 30 米",
                    icon: "hare", tint: .duckGood,
                    isMet: { $0.bestWalk >= 30 }),
    ]
}

// MARK: - 语义色（docs/UI_TOKENS.md §2.2；DuckTheme.swift 落地后改引用它）

// 语义色已迁到 DuckTheme.swift（UI_TOKENS §7 的唯一落点）。


// MARK: - 成长面板

struct ProgressSheet: View {
    /// The record page shows the core's last seven days under the book
    /// (IA reorder: the daily entry owns the day rows). Optional so the
    /// previews and any store-only caller keep working.
    var controller: DuckController? = nil
    @ObservedObject private var store = ProgressStore.shared
    @Environment(\.dismiss) private var dismiss

    init(controller: DuckController? = nil) {
        self.controller = controller
    }
    @Environment(\.scenePhase) private var scenePhase

    /// 系统的跨天通知（rawValue 与 Foundation 的 NSCalendarDayChangedNotification 一致）。
    private let dayChanged = Notification.Name("NSCalendarDayChangedNotification")

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            header
            ScrollView {
                VStack(alignment: .leading, spacing: DuckSpace.xxl) {
                    streakBanner
                    dayRows
                    taskSection
                    weekSection
                    achievementSection
                }
                .padding(.horizontal, DuckSpace.xl)
                .padding(.bottom, DuckSpace.xxxl)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Color.duckBgSurface.ignoresSafeArea())
        .presentationDetents([.medium, .large])
        .onAppear { store.refreshDayIfNeeded() }
        .onChange(of: scenePhase) { phase in
            if phase == .active { store.refreshDayIfNeeded() }
        }
        .onReceive(NotificationCenter.default.publisher(for: dayChanged)) { _ in
            store.refreshDayIfNeeded()
        }
    }

    /// The core's own per-day rows, newest last - the same seven the
    /// instrument's life card shows (FE-3's data, now on the record page).
    @ViewBuilder
    private var dayRows: some View {
        if let controller, !controller.vetbookDays.isEmpty {
            Text(controller.vetbookDays.suffix(7).joined(separator: " | "))
                .font(.duckCaption)
                .foregroundStyle(Color.duckTextSecondary)
                .lineLimit(3)
                .accessibilityIdentifier("day-rows")
        }
    }

    // MARK: 头部

    private var header: some View {
        HStack {
            Text("成长手册")
                .font(.duckTitle.weight(.semibold))
                .foregroundStyle(Color.duckTextPrimary)
                .accessibilityIdentifier("progress-title")
            Spacer()
            Button { dismiss() } label: {
                Image(systemName: "xmark")
                    .font(DuckIcon.xs)
                    .foregroundStyle(Color.duckTextPrimary.opacity(0.7))
                    .frame(width: 48, height: 48)
                    .background(Color.duckTextPrimary.opacity(0.10), in: Circle())
                    .contentShape(Circle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel("关闭")
            .accessibilityIdentifier("progress-close")
        }
        .padding(.horizontal, DuckSpace.xl)
        .padding(.vertical, DuckSpace.md)
    }

    // MARK: 连续天数

    private var streakBanner: some View {
        HStack(spacing: DuckSpace.md) {
            Image(systemName: "flame.fill")
                .font(DuckIcon.lg)
                .foregroundStyle(Color.duckAction)
                .frame(width: 44)
            VStack(alignment: .leading, spacing: DuckSpace.xxs) {
                Text("连续 \(store.streak) 天")
                    .font(.duckTitle.weight(.semibold))
                    .foregroundStyle(Color.duckTextPrimary)
                    .monospacedDigit()
                Text("最长连续 \(store.bestStreak) 天 · 每天来看看它")
                    .font(.duckCaption.weight(.medium))
                    .foregroundStyle(Color.duckTextPrimary.opacity(0.5))
            }
            Spacer()
        }
        .padding(DuckSpace.lg)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(RoundedRectangle(cornerRadius: DuckRadius.card).fill(Color.duckAction.opacity(0.10)))
    }

    // MARK: 今日任务

    private var taskSection: some View {
        VStack(alignment: .leading, spacing: DuckSpace.md) {
            sectionTitle("今日任务 · \(store.tasksDoneToday)/\(DailyTask.catalog.count)")
            ForEach(DailyTask.catalog) { task in
                taskRow(task, amount: store.todayAmounts[task.event] ?? 0)
            }
            if !extras.isEmpty {
                Text("今日还有：" + extras.joined(separator: " · "))
                    .font(.duckCaption.weight(.medium))
                    .foregroundStyle(Color.duckTextPrimary.opacity(0.5))
            }
        }
    }

    /// photo/peer/trick 没有每日任务，但记了数就如实展示（只列今天发生过的）。
    private var extras: [String] {
        var lines: [String] = []
        if let tricks = store.todayAmounts[.trick], tricks > 0 { lines.append("技能 \(tricks) 次") }
        if let photos = store.todayAmounts[.photo], photos > 0 { lines.append("拍照 \(photos) 次") }
        if let peers = store.todayAmounts[.peer], peers > 0 { lines.append("邻里互送 \(peers) 次") }
        return lines
    }

    private func taskRow(_ task: DailyTask, amount: Int) -> some View {
        let done = amount >= task.goal
        let fraction = task.goal > 0 ? Double(min(amount, task.goal)) / Double(task.goal) : 1.0
        return HStack(spacing: DuckSpace.md) {
            ZStack {
                Circle()
                    .fill(done ? Color.duckGood : Color.duckTextPrimary.opacity(0.08))
                if done {
                    Image(systemName: "checkmark")
                        .font(DuckIcon.xs)
                        .foregroundStyle(Color.duckOnAccent)
                }
            }
            .frame(width: 28, height: 28)
            VStack(alignment: .leading, spacing: DuckSpace.xs) {
                Text(task.title)
                    .font(.duckBody.weight(.medium))
                    .foregroundStyle(done ? Color.duckTextPrimary : Color.duckTextPrimary.opacity(0.7))
                ProgressView(value: fraction)
                    .tint(done ? Color.duckGood : Color.duckData)
            }
            Text("\(min(amount, task.goal))/\(task.goal) \(task.unit)")
                .font(.duckData.weight(.medium))
                .foregroundStyle(Color.duckTextPrimary.opacity(0.5))
                .monospacedDigit()
        }
        .accessibilityElement(children: .combine)
        .accessibilityIdentifier("progress-task-\(task.id)")
    }

    // MARK: 最近 7 天

    private var weekSection: some View {
        VStack(alignment: .leading, spacing: DuckSpace.md) {
            sectionTitle("最近 7 天")
            HStack(spacing: 0) {
                ForEach(store.week) { tile in
                    VStack(spacing: DuckSpace.xs) {
                        Text(tile.isToday ? "今天" : tile.weekday)
                            .font(.duckCaption.weight(.medium))
                            .foregroundStyle(tile.isToday
                                             ? Color.duckTextPrimary
                                             : Color.duckTextPrimary.opacity(0.5))
                        ZStack {
                            Circle()
                                .fill(tile.active ? Color.duckGood : Color.duckTextPrimary.opacity(0.08))
                            if tile.active {
                                Image(systemName: "checkmark")
                                    .font(DuckIcon.xs)
                                    .foregroundStyle(Color.duckOnAccent)
                            }
                        }
                        .frame(width: 32, height: 32)
                        .overlay(
                            Circle().stroke(tile.isToday ? Color.duckTextPrimary.opacity(0.9) : Color.clear,
                                            lineWidth: 1.5)
                        )
                    }
                    .frame(maxWidth: .infinity)
                }
            }
        }
    }

    // MARK: 成就墙

    private var achievementSection: some View {
        VStack(alignment: .leading, spacing: DuckSpace.md) {
            sectionTitle("成就墙 · \(store.unlockedIDs.count)/\(Achievement.catalog.count)")
            LazyVGrid(columns: [GridItem(.flexible(), spacing: DuckSpace.md), GridItem(.flexible(), spacing: DuckSpace.md)],
                      spacing: DuckSpace.md) {
                ForEach(Achievement.catalog) { achievement in
                    achievementCard(achievement)
                }
            }
        }
    }

    private func achievementCard(_ achievement: Achievement) -> some View {
        let unlocked = store.unlockedIDs.contains(achievement.id)
        return HStack(spacing: DuckSpace.sm) {
            ZStack {
                Circle()
                    .fill(unlocked ? achievement.tint.opacity(0.16) : Color.duckTextPrimary.opacity(0.06))
                Image(systemName: achievement.icon)
                    .font(DuckIcon.sm)
                    .foregroundStyle(unlocked ? achievement.tint : Color.duckTextPrimary.opacity(0.3))
            }
            .frame(width: 40, height: 40)
            VStack(alignment: .leading, spacing: DuckSpace.xxs) {
                Text(achievement.name)
                    .font(.duckLabel.weight(.semibold))
                    .foregroundStyle(unlocked ? Color.duckTextPrimary : Color.duckTextPrimary.opacity(0.5))
                    .lineLimit(1)
                    .minimumScaleFactor(0.85)
                if unlocked, let date = store.unlockDates[achievement.id] {
                    Text("\(Self.unlockLabel(date))解锁")
                        .font(.duckCaption.weight(.medium))
                        .foregroundStyle(Color.duckTextPrimary.opacity(0.7))
                } else {
                    Text(achievement.detail)
                        .font(.duckCaption.weight(.medium))
                        .foregroundStyle(Color.duckTextPrimary.opacity(0.5))
                        .lineLimit(2)
                }
            }
            Spacer(minLength: 0)
        }
        .padding(DuckSpace.md)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(RoundedRectangle(cornerRadius: DuckRadius.control).fill(Color.duckBgRoot))
        .accessibilityElement(children: .combine)
        .accessibilityIdentifier("progress-achievement-\(achievement.id)")
    }

    // MARK: 小件

    private func sectionTitle(_ text: String) -> some View {
        Text(text)
            .font(.duckCaption.weight(.semibold))
            .foregroundStyle(Color.duckTextPrimary.opacity(0.7))
    }

    /// "2026-11-13" → "11 月 13 日"；解析失败原样返回，不崩溃。
    private static func unlockLabel(_ key: String) -> String {
        let parts = key.split(separator: "-")
        guard parts.count == 3 else { return key }
        return "\(parts[1]) 月 \(parts[2]) 日"
    }
}

struct ProgressSheet_Previews: PreviewProvider {
    static var previews: some View {
        ProgressSheet()
    }
}
