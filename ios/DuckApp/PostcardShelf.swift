import Foundation
import UIKit

/// The postcard shelf: a composed Director postcard plus one honest line about
/// what the duck actually did. Local only; Share hands the composed PNG to the
/// system sheet.
struct Postcard: Identifiable, Codable {
    var id = UUID()
    /// 成片文件名：带字幕带、日期与品牌标，货架缩略图和货架分享都用它。
    var file: String
    var caption: String
    var day: String
    /// 原帧文件名：不带字幕的裸渲染帧，与成片一起保存、在元数据里区分。
    /// 旧卡片没有这个字段（nil），只有一张原帧，照旧显示。
    var originalFile: String? = nil
}

final class PostcardShelf: ObservableObject {
    @Published private(set) var cards: [Postcard] = []
    private static let key = "duck_postcards_v1"
    private static let max = 30

    static var directory: URL {
        let files = FileManager.default
        let base = files.urls(for: .documentDirectory, in: .userDomainMask).first
            ?? files.temporaryDirectory
        let dir = base.appendingPathComponent("Postcards", isDirectory: true)
        try? files.createDirectory(at: dir, withIntermediateDirectories: true)
        return dir
    }

    init() {
        cards = Self.load()
    }

    private static func load() -> [Postcard] {
        guard let data = UserDefaults.standard.data(forKey: key),
              let cards = try? JSONDecoder().decode([Postcard].self, from: data)
        else { return [] }
        return cards.filter { card in
            FileManager.default.fileExists(
                atPath: directory.appendingPathComponent(card.file).path
            )
        }
    }

    func keep(image: UIImage, caption: String) -> Bool {
        let now = Date()
        // 毫秒时间戳：同一秒里连按两次也不会互相覆盖。
        let stamp = Int(now.timeIntervalSince1970 * 1000)
        let name = "card-\(stamp).png"          // 成片
        let frameName = "card-\(stamp)-frame.png" // 原帧
        let dir = Self.directory

        // 成片：字幕与日期直接烙进像素，货架缩略图与分享件因此一致。
        let postcard = PostcardCompose.make(from: image, caption: caption, date: now)
        guard let cardData = postcard.pngData(), let frameData = image.pngData() else {
            return false
        }
        do {
            try cardData.write(to: dir.appendingPathComponent(name), options: .atomic)
            try frameData.write(to: dir.appendingPathComponent(frameName), options: .atomic)
        } catch {
            // 半成品不留：任何一份写失败都把两份都清掉。
            try? FileManager.default.removeItem(at: dir.appendingPathComponent(name))
            try? FileManager.default.removeItem(at: dir.appendingPathComponent(frameName))
            return false
        }

        let card = Postcard(
            file: name,
            caption: caption,
            day: Self.isoDay(),
            originalFile: frameName
        )
        cards.insert(card, at: 0)
        while cards.count > Self.max {
            let last = cards.removeLast()
            try? FileManager.default.removeItem(
                at: dir.appendingPathComponent(last.file)
            )
            if let original = last.originalFile {
                try? FileManager.default.removeItem(
                    at: dir.appendingPathComponent(original)
                )
            }
        }
        persist()
        return true
    }

    /// 货架分享与缩略图都用成片；原帧只作留存，不在界面上使用。
    func url(for card: Postcard) -> URL {
        Self.directory.appendingPathComponent(card.file)
    }

    private func persist() {
        if let data = try? JSONEncoder().encode(cards) {
            UserDefaults.standard.set(data, forKey: Self.key)
        }
    }

    static func isoDay() -> String {
        let formatter = DateFormatter()
        formatter.calendar = Calendar(identifier: .gregorian)
        formatter.locale = Locale(identifier: "en_US_POSIX")
        formatter.timeZone = .current
        formatter.dateFormat = "yyyy-MM-dd"
        return formatter.string(from: Date())
    }
}
