import SwiftUI

/// Policy A/B: fork, then load a policy DIRECTORY on branch B. The reload
/// route's own refusal is the honest answer for a folder the manifest
/// cannot read; this page only carries the name and shows the verdict.
struct ABSheet: View {
    @ObservedObject var controller: DuckController
    @Environment(\.dismiss) private var dismiss
    @State private var folder = ""
    @State private var status: String?

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            HStack {
                Text(Copy.abTry)
                    .font(.system(size: 20, weight: .semibold, design: .rounded))
                    .accessibilityIdentifier("ab-title")
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
                .accessibilityIdentifier("ab-close")
            }
            Text(Copy.abNote)
                .font(.system(size: 12, design: .rounded))
                .foregroundStyle(.secondary)

            TextField(Copy.abTry, text: $folder)
                .textFieldStyle(.roundedBorder)
                .autocorrectionDisabled()
                .textInputAutocapitalization(.never)
                .accessibilityIdentifier("ab-folder")

            Button {
                guard !folder.trimmingCharacters(in: .whitespaces).isEmpty else { return }
                status = Copy.backupWorking
                controller.loadPolicyOnBranchB(directory: folder)
                DispatchQueue.main.asyncAfter(deadline: .now() + 1.2) {
                    status = Copy.abLoaded
                }
            } label: {
                Label(Copy.backupImport, systemImage: "arrow.triangle.branch")
                    .font(.system(size: 15, weight: .semibold, design: .rounded))
                    .frame(maxWidth: .infinity, minHeight: 52)
            }
            .buttonStyle(.borderedProminent)
            .accessibilityIdentifier("ab-load")

            if let status {
                Text(status)
                    .font(.system(size: 12, design: .rounded))
                    .foregroundStyle(.secondary)
                    .accessibilityIdentifier("ab-status")
            }
            Spacer()
        }
        .padding(20)
        .preferredColorScheme(.dark)
    }
}
