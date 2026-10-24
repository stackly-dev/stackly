#if DEBUG
    import StacklyClient
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

        // MARK: - Staged progress

        /// The five stages of an install (spec 0018, *Alur install*).
        static let installStages = [
            StageDefinition(key: "download", title: "Download"),
            StageDefinition(key: "verify_digest", title: "Verify digest"),
            StageDefinition(key: "extract", title: "Extract"),
            StageDefinition(key: "validate_binary", title: "Validate binary"),
            StageDefinition(key: "commit", title: "Commit"),
        ]

        /// An operation in `state` at `stage`, for the progress samples.
        static func operation(
            _ state: OperationState, stage: String? = nil,
            progress: OperationProgress = .indeterminate
        ) -> OperationSummary {
            OperationSummary(
                id: UUID(), kind: .installRuntime, resource: nil, state: state, stage: stage,
                progress: progress, waitingOn: nil, cancelRequestedAt: nil,
                pointOfNoReturnAt: nil, failure: nil, recovery: nil, result: nil,
                trigger: OperationTriggerInfo(origin: .user, client: .gui),
                createdAt: FixtureClock.now,
                startedAt: nil, updatedAt: FixtureClock.now, revision: 1)
        }

        /// The staged progress of an install operation in `state`.
        static func installProgress(
            _ state: OperationState, stage: String? = nil,
            progress: OperationProgress = .indeterminate
        ) -> StagedProgressValue {
            .from(
                operation: operation(state, stage: stage, progress: progress),
                stages: installStages)
        }

        /// The compact samples: 38%, a stage this build does not know, and queued.
        static let compactProgress: [(name: String, value: StagedProgressValue)] = [
            (
                "Running at 38%",
                installProgress(
                    .running, stage: "verify_digest",
                    progress: .counted(CountedProgress(done: 90, total: 100)))
            ),
            ("Unknown stage", installProgress(.running, stage: "unpacking")),
            ("Queued", installProgress(.queued)),
        ]

        /// The stages samples, one per outcome.
        static let stagesProgress: [(name: String, value: StagedProgressValue)] = [
            (
                "Running",
                installProgress(
                    .running, stage: "extract",
                    progress: .counted(CountedProgress(done: 50, total: 100)))
            ),
            (
                "Cancelling",
                installProgress(
                    .cancelRequested, stage: "extract",
                    progress: .counted(CountedProgress(done: 50, total: 100)))
            ),
            ("Failed", installProgress(.failed, stage: "extract")),
            ("Needs recovery", installProgress(.needsRecovery, stage: "commit")),
            ("Cancelled while queued", installProgress(.cancelled)),
            ("Succeeded", installProgress(.succeeded)),
        ]
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
