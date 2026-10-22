import Foundation
import StacklyClient

/// What the New Instance sheet reads from the daemon (spec 0020, *Sheet New Instance*): the
/// form of a runtime, the preflight of a draft, and the references Copy from offers.
///
/// A test passes a fake that answers each call at a time it chooses. The app passes
/// ``LiveCreateInstanceService``.
public protocol CreateInstanceService {
    /// `instance.get_create_form`.
    ///
    /// - Parameters:
    ///   - runtimeId: The installed runtime the instance will run on.
    ///   - provider: The provider to use, or `nil` for the catalog entry of the runtime.
    func getCreateForm(runtimeId: UUID, provider: String?) async throws(ClientError)
        -> CreateForm

    /// `instance.get_duplicate_form`: the form and the draft that copy the settings of an
    /// instance (spec 0024).
    ///
    /// - Parameter instanceId: The instance to copy the settings of.
    func getDuplicateForm(instanceId: UUID) async throws(ClientError) -> DuplicateForm

    /// `instance.preflight`.
    ///
    /// - Parameter draft: The draft, with the kind of every secret source and no value.
    func preflight(_ draft: PreflightCreateParams) async throws(ClientError) -> CreatePreflight

    /// `secret.list` without a filter: the references Copy from can offer. It reads the
    /// daemon's cache and never touches the Keychain (spec 0011).
    func listSecrets() async throws(ClientError) -> [SecretRefSummary]
}

/// The service of the running app: the create read methods and `secret.list` over the
/// daemon model's link.
public struct LiveCreateInstanceService: CreateInstanceService {
    private let create: CreateClient
    private let secrets: SecretsClient

    /// Creates a service over explicit clients.
    ///
    /// - Parameters:
    ///   - create: The create read methods.
    ///   - secrets: The secret methods.
    public init(create: CreateClient, secrets: SecretsClient) {
        self.create = create
        self.secrets = secrets
    }

    /// Creates a service over the link of `model`.
    ///
    /// - Parameter model: The daemon model of the shell.
    public init(model: DaemonModel) {
        self.init(
            create: CreateClient { call, options throws(ClientError) in
                try await model.read(call, options: options).get()
            },
            secrets: model.secrets)
    }

    /// `instance.get_create_form`.
    public func getCreateForm(runtimeId: UUID, provider: String?) async throws(ClientError)
        -> CreateForm
    {
        try await create.getCreateForm(runtimeId: runtimeId, provider: provider)
    }

    /// `instance.get_duplicate_form`.
    public func getDuplicateForm(instanceId: UUID) async throws(ClientError) -> DuplicateForm {
        try await create.getDuplicateForm(instanceId: instanceId)
    }

    /// `instance.preflight`.
    public func preflight(_ draft: PreflightCreateParams) async throws(ClientError)
        -> CreatePreflight
    {
        try await create.preflight(draft)
    }

    /// `secret.list`, rows only.
    public func listSecrets() async throws(ClientError) -> [SecretRefSummary] {
        try await secrets.listSecrets().items ?? []
    }
}
