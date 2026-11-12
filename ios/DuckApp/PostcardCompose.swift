import UIKit

/// 明信片成片：原帧 + 底部字幕带 + 真实字幕 + 日期 + 品牌小字标。
///
/// 纯函数——只依赖入参（图、字幕、日期），不读文件、不碰控制器；
/// 同样的输入（同一时区/区域设置下）得到同样的成片，便于复用和测试。
enum PostcardCompose {
    /// 设计基准宽：iPhone 竖屏约 390 pt。
    ///
    /// Director 的原帧来自 Metal 回读，`scale = 1`、`size` 是像素网格
    /// （例如 1179×2556）。若直接按 13 pt 画字，字在成片里只有 13 像素，
    /// 分享出去几乎看不见。因此字号与边距都按「短边 / 390」换算：
    /// 3x 帧上 13 pt 画成约 39 px，观感仍是 13 pt，同时保持原图分辨率。
    private static let referenceWidth: CGFloat = 390

    /// 合成明信片。caption 原样落字，绝不改写——那一行是鸭子的真实计数。
    static func make(from image: UIImage, caption: String, date: Date = Date()) -> UIImage {
        let size = image.size
        guard size.width >= 1, size.height >= 1 else { return image }

        let unit = min(size.width, size.height) / referenceWidth

        let format = UIGraphicsImageRendererFormat()
        format.scale = image.scale // 与原图同倍率：不额外放大，也不压糊
        format.opaque = false
        let renderer = UIGraphicsImageRenderer(size: size, format: format)

        return renderer.image { context in
            // 1. 原帧铺满
            image.draw(in: CGRect(origin: .zero, size: size))

            // 2. 底部字幕带：约 12% 高、55% 黑，压住画面但不遮住鸭子
            let bandHeight = max(size.height * 0.12, 52 * unit)
            let band = CGRect(
                x: 0,
                y: size.height - bandHeight,
                width: size.width,
                height: bandHeight
            )
            context.cgContext.setFillColor(UIColor.black.withAlphaComponent(0.55).cgColor)
            context.cgContext.fill(band)

            let paddingX = 14 * unit
            let paddingY = 8 * unit

            let captionFont = roundedFont(size: 13 * unit, weight: .medium)
            let dateFont = roundedFont(size: 11 * unit, weight: .regular)
            let markFont = roundedFont(size: 8 * unit, weight: .semibold)

            // 3. 右下角品牌小字标
            let markText = "MicroDuck" as NSString
            let markAttrs: [NSAttributedString.Key: Any] = [
                .font: markFont,
                .foregroundColor: UIColor.white.withAlphaComponent(0.55),
            ]
            let markSize = markText.size(withAttributes: markAttrs)
            let markOrigin = CGPoint(
                x: size.width - paddingX - markSize.width,
                y: size.height - paddingY - markSize.height
            )
            markText.draw(at: markOrigin, withAttributes: markAttrs)

            // 4. 右侧日期（本地日期，YYYY.M.D）
            let dateText = dayText(date) as NSString
            let dateAttrs: [NSAttributedString.Key: Any] = [
                .font: dateFont,
                .foregroundColor: UIColor.white.withAlphaComponent(0.85),
            ]
            let dateSize = dateText.size(withAttributes: dateAttrs)
            let textTop = band.minY + paddingY
            let textBottom = band.maxY - paddingY
            let textHeight = max(1, textBottom - textTop)
            let textMidY = textTop + textHeight / 2
            let dateOrigin = CGPoint(
                x: size.width - paddingX - dateSize.width,
                y: textMidY - dateSize.height / 2
            )
            dateText.draw(at: dateOrigin, withAttributes: dateAttrs)

            // 5. 左侧字幕：右边界避开日期与品牌标，垂直居中，超长时尾部省略
            guard !caption.isEmpty else { return }
            let captionRight = min(dateOrigin.x, markOrigin.x) - 14 * unit
            let captionWidth = max(1, captionRight - paddingX)
            let paragraph = NSMutableParagraphStyle()
            paragraph.lineBreakMode = .byTruncatingTail
            let captionAttrs: [NSAttributedString.Key: Any] = [
                .font: captionFont,
                .foregroundColor: UIColor.white,
                .paragraphStyle: paragraph,
            ]
            let captionText = caption as NSString
            let options: NSStringDrawingOptions = [
                .usesLineFragmentOrigin,
                .usesFontLeading,
                .truncatesLastVisibleLine,
            ]
            let bounds = captionText.boundingRect(
                with: CGSize(width: captionWidth, height: textHeight),
                options: options,
                attributes: captionAttrs,
                context: nil
            )
            let drawHeight = min(bounds.height.rounded(.up), textHeight)
            let drawRect = CGRect(
                x: paddingX,
                y: textMidY - drawHeight / 2,
                width: captionWidth,
                height: drawHeight
            )
            captionText.draw(with: drawRect, options: options, attributes: captionAttrs, context: nil)
        }
    }

    /// 圆体；系统不提供圆体设计时回退普通系统字。
    private static func roundedFont(size: CGFloat, weight: UIFont.Weight) -> UIFont {
        let base = UIFont.systemFont(ofSize: size, weight: weight)
        guard let descriptor = base.fontDescriptor.withDesign(.rounded) else { return base }
        return UIFont(descriptor: descriptor, size: size)
    }

    /// 右侧日期：本地时区的 YYYY.M.D。
    private static func dayText(_ date: Date) -> String {
        let formatter = DateFormatter()
        formatter.calendar = Calendar(identifier: .gregorian)
        formatter.locale = Locale.current
        formatter.timeZone = .current
        formatter.dateFormat = "yyyy.M.d"
        return formatter.string(from: date)
    }
}
