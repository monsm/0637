import SwiftUI
import UniformTypeIdentifiers

/// The move-out page: one document carries care, quests and rules to
/// another phone. The core writes it (`GET /backup`) and reads it back
/// (`POST /backup/restore`) through the same doors the shells use every
/// day, so nothing here can carry a field the duck would not accept. The
/// postcards are the phone's own files and do not travel — the record says
/// so rather than pretending.
struct BackupSheet: View {
    @ObservedObject var controller: DuckController
    @Environment(\.dismiss) private var dismiss
    @State private var status: String?
    @State private var showingImporter = false

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            HStack {
                Text(Copy.backup)
                    .font(.system(size: 20, weight: .semibold, design: .rounded))
                    .accessibilityIdentifier("backup-title")
                Spacer()
                Button {
                    dismiss()
                } label: {
                    Image(systemName: "xmark")
                        .font(.system(size: 12, weight: .semibold))
                        .foregroundStyle(.secondary)
                        .frame(width: 48, height: 48)
                        .background(Color.white.opacity(0.10), in: Circle())
                        .contentShape(Circle())
                }
                .buttonStyle(.plain)
                .accessibilityLabel(Copy.close)
                .accessibilityIdentifier("backup-close")
            }
            Text(Copy.backupNote)
                .font(.system(size: 12, design: .rounded))
                .foregroundStyle(.secondary)

            Button {
                export()
            } label: {
                Label(Copy.backupExport, systemImage: "square.and.arrow.up")
                    .font(.system(size: 15, weight: .semibold, design: .rounded))
                    .frame(maxWidth: .infinity, minHeight: 52)
            }
            .buttonStyle(.borderedProminent)
            .accessibilityIdentifier("backup-export")

            Button {
                showingImporter = true
            } label: {
                Label(Copy.backupImport, systemImage: "square.and.arrow.down")
                    .font(.system(size: 15, weight: .semibold, design: .rounded))
                    .frame(maxWidth: .infinity, minHeight: 52)
            }
            .buttonStyle(.bordered)
            .accessibilityIdentifier("backup-import")

            if let status {
                Text(status)
                    .font(.system(size: 12, design: .rounded))
                    .foregroundStyle(.secondary)
                    .accessibilityIdentifier("backup-status")
            }
            Spacer()
        }
        .padding(20)
        .preferredColorScheme(.dark)
        .fileImporter(
            isPresented: $showingImporter,
            allowedContentTypes: [.json, .data],
        ) { result in
            importBackup(result)
        }
    }

    /// Ask the core for its document and hand it to the system share sheet.
    private func export() {
        status = Copy.backupWorking
        controller.fetchBackup { text in
            guard let text else {
                status = Copy.backupFailed
                return
            }
            let url = FileManager.default.temporaryDirectory
                .appendingPathComponent("MicroDuck-backup.json")
            do {
                try text.data(using: .utf8)?.write(to: url)
                controller.shareFile(url: url)
                status = Copy.backupReady
            } catch {
                status = Copy.backupFailed
            }
        }
    }

    /// Read the person's file and give it to the core: the duck clamps,
    /// re-measures and refuses whatever it does not recognise.
    private func importBackup(_ result: Result<URL, Error>) {
        switch result {
        case .failure:
            status = nil
        case .success(let url):
            let secured = url.startAccessingSecurityScopedResource()
            defer { if secured { url.stopAccessingSecurityScopedResource() } }
            guard let text = try? String(contentsOf: url, encoding: .utf8) else {
                status = Copy.backupFailed
                return
            }
            status = Copy.backupWorking
            controller.postRestore(text) { ok in
                status = ok ? Copy.backupDone : Copy.backupFailed
            }
        }
    }
}
