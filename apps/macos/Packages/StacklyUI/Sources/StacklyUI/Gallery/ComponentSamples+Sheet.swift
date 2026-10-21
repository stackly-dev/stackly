#if DEBUG
    import SwiftUI

    /// The sample values of milestone 2 (spec 0067): the sheet, the checklist, and the staged
    /// progress. Taken from the mockup, with no real credentials.
    extension ComponentSamples {
        // MARK: - Sheet

        /// The provider tile of the sample create sheet.
        static let sheetTile = TileSpec(mark: "Pg", tile: .blue, name: "PostgreSQL")
        /// The subtitle of the sample create sheet.
        static let sheetSubtitle = "PostgreSQL 17.4 · managed runtime"
        /// The main action of the sample create sheet.
        static let createAction = StacklyAction(
            id: "sheet.create", title: "Create", mutates: true, perform: {})
        /// The main action while a field is wrong: it says why.
        static let createDisabledAction = StacklyAction(
            id: "sheet.create", title: "Create", mutates: true,
            disabledReason: "Fix the errors above.", perform: {})
        /// The main action while the instance is being created.
        static let createBusyAction = StacklyAction(
            id: "sheet.create", title: "Create", mutates: true, isBusy: true, perform: {})
        /// A destructive main action: bordered, red, with no Return shortcut.
        static let sheetRemoveAction = StacklyAction(
            id: "sheet.remove", title: "Remove…", role: .destructive(removeConfirmation),
            mutates: true, perform: {})
    }

    /// The body of the sample create sheet: a grouped form on the sheet background.
    struct SheetSampleForm: View {
        @State private var name = "pg17-dev"
        @State private var port = "5432"
        @State private var startsAfterCreating = true

        var body: some View {
            Form {
                TextField("Name", text: $name)
                TextField("Port", text: $port)
                Toggle("Start after creating", isOn: $startsAfterCreating)
            }
            .formStyle(.grouped)
            .scrollContentBackground(.hidden)
            .scrollDisabled(true)
        }
    }
#endif
