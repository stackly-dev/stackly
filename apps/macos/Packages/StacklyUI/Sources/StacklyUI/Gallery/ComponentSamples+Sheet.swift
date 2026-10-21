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

        // MARK: - Checklist

        /// One row per `CheckStatus`, as a preflight would show them.
        static let checkItems: [CheckItem] = [
            CheckItem(id: "port", status: .passed, title: "Port 5432 is free"),
            CheckItem(
                id: "disk", status: .warning, title: "Low disk space",
                detail: "4.1 GB is left on this volume. A PostgreSQL instance needs about 2 GB."),
            CheckItem(
                id: "folder", status: .failed, title: "Data directory can’t be written",
                detail: "/Volumes/Backup/pg17-dev is read only. Choose a folder you own.",
                action: StacklyAction(
                    id: "check.chooseFolder", title: "Choose Folder…", mutates: false,
                    perform: {})),
            CheckItem(id: "runtime", status: .pending, title: "Checking the runtime"),
            CheckItem(
                id: "newer", status: .unrecognized, title: "A newer check",
                detail: "This version of Stackly doesn’t know this result."),
        ]
        /// A row whose detail is long: it wraps in full, never to an ellipsis.
        static let longDetailItem = CheckItem(
            id: "long", status: .warning, title: "Another instance uses this data directory",
            detail: "The instance “pg16-old” already keeps its data in "
                + "/Users/dev/Library/Application Support/Stackly/instances/pg16-old/data. "
                + "Two instances writing to one directory would corrupt it, so choose another "
                + "folder or remove the other instance first.")
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
