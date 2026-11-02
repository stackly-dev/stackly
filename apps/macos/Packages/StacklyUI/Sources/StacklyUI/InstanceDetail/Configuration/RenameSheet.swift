import Foundation
import Observation
import StacklyClient
import SwiftUI

/// The model of the Rename sheet (spec 0029, AC-13, AC-19): the name is checked as it is
/// typed against the rule of VAL-001 and the names of the other instances, and the daemon
/// checks it again when the rename commits.
@Observable
public final class RenameModel {
    /// The instance.
    public let source: InstanceEditSource
    /// The name as typed.
    public var name: String
    /// The sentence the daemon gave for the name, until the name changes.
    public private(set) var refusal: String?
    /// Whether the rename is on its way.
    public private(set) var isBusy = false

    @ObservationIgnored private let service: InstanceConfigService
    @ObservationIgnored private let others: @MainActor () -> Set<String>
    @ObservationIgnored private var unknown: String?

    /// Creates a model that starts on the current name.
    /// - Parameters:
    ///   - source: The instance.
    ///   - service: The mutation path.
    ///   - others: The names of the other instances, read when the name is checked.
    public init(
        source: InstanceEditSource, service: InstanceConfigService,
        others: @escaping @MainActor () -> Set<String>
    ) {
        self.source = source
        self.service = service
        self.others = others
        name = source.name
    }

    /// The sentence under the field, or `nil` while the name can be used.
    public var problem: String? {
        if let refusal { return refusal }
        let trimmed = name.trimmingCharacters(in: .whitespaces)
        guard trimmed != source.name, trimmed.isEmpty == false else { return nil }
        if Self.isValid(trimmed) == false {
            return "Use 1 to 63 lowercase letters, digits, or hyphens, starting with a letter."
        }
        if others().contains(trimmed) { return "An instance named \(trimmed) already exists." }
        return nil
    }

    /// Why the main button is off, or `nil` when the rename can go.
    public var disabledReason: String? {
        let trimmed = name.trimmingCharacters(in: .whitespaces)
        if trimmed.isEmpty { return "Enter a name." }
        if trimmed == source.name { return "Enter a new name." }
        return problem == nil ? nil : "Choose another name."
    }

    /// The rule of VAL-001: 1 to 63 lowercase letters, digits, or hyphens, starting with a
    /// letter.
    public static func isValid(_ name: String) -> Bool {
        guard let first = name.unicodeScalars.first, (1...63).contains(name.count) else {
            return false
        }
        func lower(_ scalar: Unicode.Scalar) -> Bool { ("a"..."z").contains(scalar) }
        return lower(first)
            && name.unicodeScalars.allSatisfy {
                lower($0) || ("0"..."9").contains($0) || $0 == "-"
            }
    }

    /// Forgets the daemon's sentence once the name is edited.
    public func edited() {
        refusal = nil
    }

    /// Renames the instance and follows the operation to its end.
    /// - Parameter context: Where the cards go.
    /// - Returns: Whether the sheet may close.
    public func submit(context: InstanceActionContext) async -> Bool {
        guard isBusy == false, disabledReason == nil else { return false }
        let trimmed = name.trimmingCharacters(in: .whitespaces)
        isBusy = true
        defer { isBusy = false }
        let call = Call.instanceRename(
            RenameInstanceParams(instanceId: source.id, name: trimmed, expectedRevision: nil))
        let key = unknown ?? MutationPath.newKey()
        let end = await ConfigMutationRunner(service: service).run(call, key: key)
        unknown = nil
        switch end {
        case .succeeded:
            context.report(.success("\(source.name) renamed to \(trimmed)."))
            return true
        case .unchanged:
            return true
        case .refused(let body):
            switch body.code {
            case "instance.name_taken":
                refusal = "An instance named \(trimmed) already exists."
            case "instance.name_invalid":
                refusal =
                    "Use 1 to 63 lowercase letters, digits, or hyphens, starting with a letter."
            default:
                context.report(.failure(body))
            }
            return false
        case .failed(let body, _):
            context.report(
                Feedback(
                    tone: .critical, message: "Couldn’t rename \(source.name).",
                    detail: body.map { DaemonText.sentence($0.message) }, code: body?.code))
            return true
        case .cancelled:
            context.report(.neutral("Renaming \(source.name) was cancelled."))
            return true
        case .notCompleted(let outcome):
            if case .outcomeUnknown(_, let key) = outcome { unknown = key }
            if let card = Feedback.outcome(
                outcome, success: nil, showOperations: context.showOperations)
            {
                context.report(card)
            }
            return false
        }
    }
}

/// The Rename sheet: one name field with its live check, and what stays as it is.
struct RenameSheet: View {
    @Environment(\.dismiss) private var dismiss
    @State private var model: RenameModel

    let context: InstanceActionContext

    /// The sheet of an instance, with a model of its own.
    init(
        source: InstanceEditSource, service: InstanceConfigService,
        others: @escaping @MainActor () -> Set<String>, context: InstanceActionContext
    ) {
        _model = State(
            initialValue: RenameModel(source: source, service: service, others: others))
        self.context = context
    }

    /// The sheet over a model made elsewhere, for a preview or a snapshot.
    init(model: RenameModel, context: InstanceActionContext) {
        _model = State(initialValue: model)
        self.context = context
    }

    var body: some View {
        StacklySheet(
            title: "Rename Instance", subtitle: model.source.name, primary: primary, minWidth: 440
        ) {
            VStack(alignment: .leading, spacing: Spacing.s8) {
                TextField(
                    "Name",
                    text: Binding(
                        get: { model.name },
                        set: {
                            model.name = $0
                            model.edited()
                        }))
                if let problem = model.problem {
                    Label(problem, systemImage: Tone.critical.symbol)
                        .font(.callout)
                        .foregroundStyle(Tone.critical.text)
                }
                Text("Only the name changes. Its data, ports, and configuration stay as they are.")
                    .font(.callout)
                    .foregroundStyle(StacklyColor.textSecondary)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        }
    }

    private var primary: StacklyAction {
        StacklyAction(
            id: "rename.confirm", title: model.isBusy ? ConfigurationCopy.renaming : "Rename",
            mutates: true, disabledReason: model.isBusy ? nil : model.disabledReason,
            isBusy: model.isBusy,
            perform: {
                Task {
                    if await model.submit(context: context) { dismiss() }
                }
            })
    }
}
