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
        VStack(alignment: .leading, spacing: DuckSpace.lg) {
            HStack {
                Text(Copy.abTry)
                    .font(.duckTitle.weight(.semibold))
                    .accessibilityIdentifier("ab-title")
                Spacer()
                Button {
                    dismiss()
                } label: {
                    Image(systemName: "xmark")
                        .font(DuckIcon.xs)
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
                .font(.duckCaption)
                .foregroundStyle(.secondary)

            TextField(Copy.abTry, text: $folder)
                .textFieldStyle(.roundedBorder)
                .autocorrectionDisabled()
                .textInputAutocapitalization(.never)
                .accessibilityIdentifier("ab-folder")

            Button {
                guard !folder.trimmingCharacters(in: .whitespaces).isEmpty else { return }
                status = Copy.backupWorking
                // The reload route's own verdict decides the line: a refusal
                // shows the core's reason, a load shows loaded. A fixed
                // "loaded" told the person a policy had been installed when
                // the core had refused the folder.
                controller.loadPolicyOnBranchB(directory: folder) { verdict in
                    status = verdict ?? Copy.abLoaded
                }
            } label: {
                Label(Copy.backupImport, systemImage: "arrow.triangle.branch")
                    .font(.duckBody.weight(.semibold))
                    .frame(maxWidth: .infinity, minHeight: 52)
            }
            .buttonStyle(.borderedProminent)
            .accessibilityIdentifier("ab-load")

            if let status {
                Text(status)
                    .font(.duckCaption)
                    .foregroundStyle(.secondary)
                    .accessibilityIdentifier("ab-status")
            }
            Spacer()
        }
        .padding(DuckSpace.xl)
    }
}
