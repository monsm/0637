import Foundation
import UIKit

/// The postcard shelf: a Director frame plus one honest line about what the
/// duck actually did. Local only; Share hands the PNG to the system sheet.
struct Postcard: Identifiable, Codable {
    var id = UUID()
    var file: String
    var caption: String
    var day: String
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
        let name = "card-\(Int(Date().timeIntervalSince1970)).png"
        guard let data = image.pngData() else { return false }
        do {
            try data.write(
                to: Self.directory.appendingPathComponent(name),
                options: .atomic
            )
        } catch {
            return false
        }
        let card = Postcard(
            file: name,
            caption: caption,
            day: Self.isoDay()
        )
        cards.insert(card, at: 0)
        while cards.count > Self.max {
            let last = cards.removeLast()
            try? FileManager.default.removeItem(
                at: Self.directory.appendingPathComponent(last.file)
            )
        }
        persist()
        return true
    }

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
