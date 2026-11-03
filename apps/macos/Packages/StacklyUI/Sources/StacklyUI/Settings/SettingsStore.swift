import Foundation
import Observation
import StacklyClient

/// The daemon's settings as the Settings window shows them (spec 0032 AC-16, AC-17, AC-24).
///
/// It keeps the last copy it read, so the window still shows values while stacklyd is
/// offline, and it reads again when the daemon says the settings changed, when the events
/// have a gap, and when the link connects again. A change goes out with the revision of that
/// copy; a revision the daemon no longer holds shows "Changed elsewhere. Try again." on the
/// row and reads the new values. Edits go one at a time, so each carries the revision the
/// one before it produced.
@Observable
public final class SettingsStore {
    /// One setting a row can change.
    public enum Field: Hashable, Sendable {
        /// The startup policy of new instances.
        case defaultStartup
        /// The update channel.
        case updateChannel
        /// Sharing usage statistics.
        case telemetry
    }

    /// One change a row asks for.
    public enum Edit: Equatable, Sendable {
        /// The startup policy of new instances.
        case startup(StartupPolicyKind)
        /// The update channel.
        case channel(UpdateChannel)
        /// Sharing usage statistics.
        case telemetry(Bool)

        /// The setting the change is for.
        public var field: Field {
            switch self {
            case .startup: .defaultStartup
            case .channel: .updateChannel
            case .telemetry: .telemetry
            }
        }

        var patch: SettingsPatch {
            switch self {
            case .startup(let value):
                SettingsPatch(defaultStartup: value, updateChannel: nil, telemetry: nil)
            case .channel(let value):
                SettingsPatch(defaultStartup: nil, updateChannel: value, telemetry: nil)
            case .telemetry(let value):
                SettingsPatch(defaultStartup: nil, updateChannel: nil, telemetry: value)
            }
        }
    }

    /// The settings last read in this session, or `nil` before the first answer.
    public private(set) var settings: DaemonSettingsView?
    /// The storage locations last read, the built in ones first.
    public private(set) var locations: [StorageLocationView] = []
    /// The change on its way to the daemon, or `nil`.
    public private(set) var saving: Edit?
    /// The line under a row whose last change did not go through.
    public private(set) var notices: [Field: String] = [:]
    /// The location kind whose default is being changed, or `nil`.
    public private(set) var savingLocation: LocationKind?
    /// The line under a location row whose last change did not go through.
    public private(set) var locationNotices: [LocationKind: String] = [:]

    @ObservationIgnored private let client: SettingsClient
    @ObservationIgnored private let newKey: () -> String
    @ObservationIgnored private var isRefreshing = false
    @ObservationIgnored private var refreshAgain = false

    /// Creates a store with nothing read yet.
    /// - Parameters:
    ///   - client: The settings calls.
    ///   - newKey: Makes the idempotency key of one change.
    public init(client: SettingsClient, newKey: @escaping () -> String = { MutationPath.newKey() })
    {
        self.client = client
        self.newKey = newKey
    }

    /// Whether the settings were read once in this session.
    public var hasLoaded: Bool { settings != nil }

    // MARK: - Reading

    /// Reads the settings and the locations. Several calls while one runs make one more read
    /// after it. An answer that does not come keeps the copy it has.
    public func refresh() async {
        if isRefreshing {
            refreshAgain = true
            return
        }
        isRefreshing = true
        defer { isRefreshing = false }
        repeat {
            refreshAgain = false
            do throws(ClientError) {
                accept(try await client.get())
                locations = try await client.locations()
            } catch {
                // The link reports why; the copy stays as it is.
            }
        } while refreshAgain
    }

    /// Reads again when `event` says the settings changed to a revision this copy lacks.
    /// - Returns: The read, so a test can wait for it; `nil` for any other event.
    @discardableResult
    public func receive(event: Event) -> Task<Void, Never>? {
        guard case .settingsChanged(let changed) = event.kind,
            changed.revision > (settings?.revision ?? 0)
        else { return nil }
        return Task { [weak self] in await self?.refresh() }
    }

    private func accept(_ view: DaemonSettingsView) {
        if let current = settings, view.revision < current.revision { return }
        settings = view
    }

    // MARK: - Changing

    /// The value a row shows for the startup policy: the one asked for while it is saving.
    public var defaultStartup: StartupPolicyKind? {
        if case .startup(let value)? = saving { return value }
        return settings?.defaultStartup
    }

    /// The value a row shows for the update channel.
    public var updateChannel: UpdateChannel? {
        if case .channel(let value)? = saving { return value }
        return settings?.updateChannel
    }

    /// The value a row shows for sharing usage statistics.
    public var telemetry: Bool? {
        if case .telemetry(let value)? = saving { return value }
        return settings?.telemetry
    }

    /// Sends one change. Nothing happens before the first read or while another change is on
    /// its way. A failure puts the row back to the daemon's value and writes the reason under
    /// it.
    public func apply(_ edit: Edit) async {
        guard saving == nil, let current = settings else { return }
        notices[edit.field] = nil
        saving = edit
        defer { saving = nil }
        do throws(ClientError) {
            accept(
                try await client.update(
                    expectedRevision: current.revision, patch: edit.patch,
                    idempotencyKey: newKey()))
        } catch {
            await fail(error, field: edit.field)
        }
    }

    /// Writes why `field` did not change.
    func fail(_ error: ClientError, field: Field) async {
        notices[field] = await message(for: error)
    }

    /// The line for a change that failed, after the reads that make the screen right again:
    /// a stale revision or a lost answer reads the daemon's values.
    private func message(for error: ClientError) async -> String {
        switch error {
        case .rejected(let body) where body.code == Self.revisionMismatch:
            await refresh()
            return SettingsCopy.changedElsewhere
        case .rejected(let body):
            return DaemonText.sentence(body.message)
        case .notSent:
            return MutationFeedback.notSent
        case .outcomeUnknown:
            await refresh()
            return MutationFeedback.outcomeUnknown
        }
    }

    // MARK: - Locations

    /// Points the default location of `kind` at `target`. The daemon checks the folder against
    /// its rules and refuses the first one it breaks; the sentence of that rule shows under the
    /// row. Both buttons of the row stay off while it runs. Afterwards the settings and the
    /// locations are read again, since the daemon's revision rose and a user count moved.
    public func setLocation(_ kind: LocationKind, to target: LocationTarget) async {
        guard savingLocation == nil, let current = settings else { return }
        locationNotices[kind] = nil
        savingLocation = kind
        defer { savingLocation = nil }
        do throws(ClientError) {
            _ = try await client.setLocation(
                kind: kind, target: target, expectedRevision: current.revision,
                idempotencyKey: newKey())
            await refresh()
        } catch {
            locationNotices[kind] = await message(for: error)
        }
    }

    /// The code of a change made against an old revision.
    static let revisionMismatch = "resource.revision_mismatch"
}
