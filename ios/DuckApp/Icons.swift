import Foundation
import SwiftUI

/// One semantic icon per action, mapped to SF Symbols. The B4 spec keeps this
/// table beside Copy: Android mirrors the same names (Material equivalents)
/// in its own Icons.kt, so the two platforms cannot drift apart.
enum Icons {
    static let feed = "fork.knife"
    static let skills = "sparkles"
    static let quack = "waveform"
    static let grab = "hand.point.up.left.fill"
    static let drop = "hand.raised.fill"
    static let more = "ellipsis.circle"
    static let path = "scribble"
    static let blocks = "square.grid.2x2.fill"
    static let dance = "music.note.list"
    static let controls = "gamecontroller"
    static let peers = "dot.radiowaves.left.and.right"
    static let chorus = "person.3.fill"
    static let gift = "gift.fill"
    static let quests = "checklist"
    static let diary = "book.closed.fill"
    static let postcards = "photo.on.rectangle.angled"
    static let dex = "square.grid.3x3.fill"
    static let ai = "brain.head.profile"
    static let night = "moon.stars.fill"
    static let morning = "sun.max.fill"
    static let flashlight = "flashlight.on.fill"
    static let heart = "heart.fill"
    static let preferences = "slider.horizontal.3"
    static let time = "clock.arrow.circlepath"
    static let director = "viewfinder"
    static let live = "circle.dashed"

    /// The stick modes: the same four verbs a joystick pad has.
    static func stick(_ mode: StickMode) -> String {
        switch mode {
        case .drive: return "figure.walk"
        case .head: return "eye"
        case .body: return "arrow.up.and.down"
        case .path: return "scribble"
        }
    }
}