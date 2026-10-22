#if DEBUG
    import SwiftUI

    /// Loads the fixture stores, then shows the Instances page on the detail page of an
    /// instance: a preview cannot await.
    private struct InstanceDetailPreview: View {
        let number: UInt8
        let showsInspector: Bool
        @State private var rig = PageRig()
        @State private var isLoaded = false

        var body: some View {
            let id = FixtureID.uuid(kind: FixtureID.instance, number: number)
            Group {
                if isLoaded {
                    rig.instancesPage(
                        opening: id, selection: id.uuidString.lowercased(),
                        showsInspector: showsInspector)
                } else {
                    Color.clear
                }
            }
            .environment(\.fixtureNow, FixtureClock.now)
            .task {
                await rig.loadInstances()
                isLoaded = true
            }
        }
    }

    #Preview("Light") {
        InstanceDetailPreview(number: 1, showsInspector: false)
            .frame(width: 1044, height: 748)
            .preferredColorScheme(.light)
    }

    #Preview("Dark") {
        InstanceDetailPreview(number: 1, showsInspector: false)
            .frame(width: 1044, height: 748)
            .preferredColorScheme(.dark)
    }

    #Preview("640 pt") {
        InstanceDetailPreview(number: 2, showsInspector: false)
            .frame(width: 640, height: 748)
            .preferredColorScheme(.light)
    }

    #Preview("384 pt page column with the inspector") {
        InstanceDetailPreview(number: 1, showsInspector: true)
            .frame(width: 664, height: 748)
            .preferredColorScheme(.light)
    }
#endif
