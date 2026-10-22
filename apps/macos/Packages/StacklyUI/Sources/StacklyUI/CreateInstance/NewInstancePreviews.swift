#if DEBUG
    import SwiftUI

    /// The New Instance sheet in duplicate mode over the fixture rig, for the canvas.
    private struct DuplicateSheetPreview: View {
        @State private var rig: CreateRig = {
            let rig = CreateRig()
            rig.service.onPreflight = { _ in FixtureCreateDuplicate.ready }
            return rig
        }()

        var body: some View {
            NewInstanceSheet(model: rig.model)
                .frame(width: 600)
                .task {
                    await rig.load()
                    rig.model.beginDuplicate(of: FixtureCreateDuplicate.source)
                }
        }
    }

    #Preview("Duplicate, light") {
        DuplicateSheetPreview()
            .preferredColorScheme(.light)
    }

    #Preview("Duplicate, dark") {
        DuplicateSheetPreview()
            .preferredColorScheme(.dark)
    }
#endif
