#if DEBUG
    import Foundation
    import StacklyClient

    /// The scenarios of the menu bar popover and of the Settings window (spec 0032, *Fixture*).
    nonisolated extension FixtureScenario {
        // MARK: - Instances

        /// An instance of the popover scenarios: running ones are ready, the others have no
        /// readiness.
        private static func service(
            _ number: UInt8, _ name: String, _ lifecycle: InstanceLifecycle,
            provider: String = "official/postgresql", version: String = "17.6",
            port: UInt16? = 5432
        ) -> InstanceSummary {
            let running = lifecycle == .running
            return .fixture(
                number: number, name: name, provider: provider, runtimeVersion: version,
                port: port, lifecycle: lifecycle, desired: running ? .running : .stopped,
                readiness: running ? .fixture(.ready, reason: "probe_ok", latencyMs: 3) : nil,
                health: running ? .fixture(.healthy, reason: "probe_ok", latencyMs: 3) : nil)
        }

        private static let normalServices: [InstanceSummary] = [
            service(1, "pg17-dev", .running),
            service(2, "orders-db", .running, version: "18.1", port: 5433),
            service(
                3, "cache", .running, provider: "official/valkey", version: "8.1.4", port: 6379),
            service(
                4, "mysql-erp", .stopped, provider: "official/mysql", version: "8.4.6", port: 3306),
        ]

        private static let manyServices: [InstanceSummary] = (1...12).map { index in
            let lifecycles: [InstanceLifecycle] = [
                .running, .stopped, .running, .failed, .running, .stopped, .starting, .running,
                .stopped, .running, .stopping, .stopped,
            ]
            return service(
                UInt8(index), String(format: "service-%02d", index), lifecycles[index - 1],
                port: UInt16(5400 + index))
        }

        // MARK: - Stop All

        private static func stopItem(_ instance: InstanceSummary, demand: [DemandSourceKind])
            -> StopAllItem
        {
            StopAllItem(
                instanceId: instance.id, name: instance.name, lifecycle: instance.lifecycle,
                demand: demand,
                autostart: InstanceAutostart(
                    policy: demand.contains(.startup) ? .atLogin : .manual, session: .none))
        }

        private static func stopPreview(
            _ items: [StopAllItem], alreadyStopping: [InstanceRef] = [],
            skipped: [StopAllSkip] = []
        ) -> StopAllPreview {
            StopAllPreview(
                items: items, alreadyStopping: alreadyStopping, skipped: skipped,
                atLoginCount: UInt64(items.filter { $0.autostart.policy == .atLogin }.count))
        }

        // MARK: - Settings

        /// The settings the daemon answers with.
        static func settingsView(
            revision: UInt64 = 3, startup: StartupPolicyKind = .manual,
            channel: UpdateChannel = .stable, telemetry: Bool = false,
            locations: DefaultLocations = defaultLocations
        ) -> DaemonSettingsView {
            DaemonSettingsView(
                revision: revision, defaultStartup: startup, updateChannel: channel,
                telemetry: telemetry, telemetryCopyVersion: 1, locations: locations,
                logRetention: LogRetention(segmentsPerStream: 8, segmentMaxBytes: 1_048_576),
                auditRetention: nil, updatedAt: FixtureClock.daysAgo(2))
        }

        /// A storage location.
        static func location(
            _ kind: LocationKind, number: UInt8, path: String, builtIn: Bool = true,
            isDefault: Bool = true, instanceCount: UInt64 = 0, reachable: Bool = true
        ) -> StorageLocationView {
            StorageLocationView(
                id: FixtureID.uuid(kind: 0x51, number: number), kind: kind, path: path,
                builtIn: builtIn, isDefault: isDefault, instanceCount: instanceCount,
                reachable: reachable)
        }

        private static let builtInRoot = "/Users/fixture/Library/Application Support/Stackly"

        private static let builtInData = location(
            .data, number: 1, path: "\(builtInRoot)/data", instanceCount: 3)
        private static let builtInLogs = location(
            .logs, number: 2, path: "\(builtInRoot)/logs", instanceCount: 3)
        private static let builtInCache = location(
            .cache, number: 3, path: "\(builtInRoot)/cache")

        private static let defaultLocations = DefaultLocations(
            data: builtInData, logs: builtInLogs, cache: builtInCache)

        private static func settingsReplies(
            _ view: DaemonSettingsView, locations: [StorageLocationView]? = nil
        ) -> [String: FixtureReply] {
            [
                "settings.get": .reply(.settingsGet(view)),
                "storage.list_locations": .reply(
                    .storageListLocations(
                        LocationList(
                            items: locations ?? [
                                view.locations.data, view.locations.logs, view.locations.cache,
                            ]))),
            ]
        }

        /// The Storage tab with a custom folder for each kind, the built in ones still used, and
        /// one folder that cannot be reached.
        private static var customStorage: (DaemonSettingsView, [StorageLocationView]) {
            let data = location(
                .data, number: 11, path: "/Users/fixture/Developer/stackly-data", builtIn: false)
            let logs = location(
                .logs, number: 12, path: "/Users/fixture/Developer/stackly-logs", builtIn: false)
            let cache = location(
                .cache, number: 13, path: "/Users/fixture/Developer/stackly-cache",
                builtIn: false)
            let oldData = location(
                .data, number: 1, path: "\(builtInRoot)/data", isDefault: false, instanceCount: 3)
            let oldLogs = location(
                .logs, number: 2, path: "\(builtInRoot)/logs", isDefault: false, instanceCount: 2)
            let gone = location(
                .data, number: 14, path: "/Users/fixture/Archive/stackly-data", builtIn: false,
                isDefault: false, instanceCount: 1, reachable: false)
            let oldCache = location(
                .cache, number: 3, path: "\(builtInRoot)/cache", isDefault: false)
            let view = settingsView(
                revision: 6, locations: DefaultLocations(data: data, logs: logs, cache: cache))
            return (view, [data, logs, cache, oldData, oldLogs, gone, oldCache])
        }

        private static var revisionMismatch: ErrorBody {
            ErrorBody(
                code: "resource.revision_mismatch", category: .conflict,
                message: "the settings changed since they were read", retryable: false,
                resource: nil, fields: nil, hint: nil, details: nil, operationId: nil, rule: nil,
                items: nil)
        }

        // MARK: - The scenarios

        /// Builds one of the menu bar and Settings scenarios.
        static func menuBarSettings(_ id: ID) -> FixtureScenario {
            let ready = DaemonPresence.connected(health: .ready)
            func connectedWith(
                _ instances: [InstanceSummary], operations: [OperationSummary] = [],
                providers: [ProviderSummary] = [], replies: [String: FixtureReply] = [:],
                agent: FixtureAgent = .fresh, presence: DaemonPresence? = nil,
                afterSnapshot: ConnectionState? = nil,
                permission: NotificationPermission = .notDetermined, notifications: Bool = false
            ) -> FixtureScenario {
                connectedScenario(
                    id, health: .fixture(), data: (instances, operations, providers),
                    afterSnapshot: afterSnapshot, presence: presence ?? ready, replies: replies,
                    agent: agent, notificationPermission: permission,
                    notificationsEnabled: notifications)
            }
            switch id {
            case .menubarNormal:
                return connectedWith(normalServices)
            case .menubarFailed:
                return connectedWith([normalServices[0], normalServices[3], instanceCases[4]])
            case .menubarOfflineLastKnown:
                return connectedWith(
                    normalServices, presence: .unreachable(lastKnownAt: FixtureClock.now),
                    afterSnapshot: unreachable)
            case .menubarMany:
                return connectedWith(manyServices)
            case .menubarEmpty:
                return connectedWith([])
            case .menubarStopPreview:
                let running = Array(normalServices[0...2])
                let stopping = service(5, "queue", .stopping, provider: "official/nats")
                let held = service(6, "ledger", .unknown, port: 5435)
                let environment = service(7, "env-db", .running, port: 5436)
                let preview = stopPreview(
                    [
                        stopItem(running[0], demand: [.manual]),
                        stopItem(running[1], demand: [.manual, .startup]),
                        stopItem(running[2], demand: [.startup]),
                    ],
                    alreadyStopping: [InstanceRef(instanceId: stopping.id, name: stopping.name)],
                    skipped: [
                        StopAllSkip(
                            instanceId: held.id, name: held.name,
                            code: "supervisor.identity_unproven"),
                        StopAllSkip(
                            instanceId: environment.id, name: environment.name,
                            code: "supervisor.stop_requires_override"),
                    ])
                return connectedWith(
                    normalServices + [stopping, held, environment],
                    replies: ["instance.preview_stop_all": .reply(.instancePreviewStopAll(preview))]
                )
            case .menubarStopPartial:
                let services = Array(normalServices[0...2])
                let preview = stopPreview(services.map { stopItem($0, demand: [.manual]) })
                let result = StopAllResult(
                    stopped: [services[0].id],
                    failed: [
                        StopAllEntry(instanceId: services[1].id, code: "supervisor.stop_timeout"),
                        StopAllEntry(instanceId: services[2].id, code: "supervisor.stop_timeout"),
                    ], skipped: [])
                return connectedWith(
                    services,
                    replies: [
                        "instance.preview_stop_all": .reply(.instancePreviewStopAll(preview)),
                        "instance.stop_all": .reply(.instanceStopAll(result)),
                    ])
            case .settingsGeneral:
                return connectedWith(
                    normalServices, replies: settingsReplies(settingsView()), agent: .enabled)
            case .settingsInstancesDenied:
                return connectedWith(
                    normalServices, replies: settingsReplies(settingsView(startup: .atLogin)),
                    permission: .denied, notifications: true)
            case .settingsSources:
                return connectedWith(
                    normalServices, providers: providerCases,
                    replies: settingsReplies(settingsView()))
            case .settingsStorageCustom:
                let storage = customStorage
                return connectedWith(
                    normalServices,
                    replies: settingsReplies(storage.0, locations: storage.1))
            case .settingsUpdates:
                return connectedWith(
                    normalServices,
                    replies: settingsReplies(settingsView(channel: .beta, telemetry: true)))
            case .settingsOffline:
                return connectedWith(
                    normalServices, providers: providerCases,
                    replies: settingsReplies(settingsView()),
                    presence: .unreachable(lastKnownAt: FixtureClock.now),
                    afterSnapshot: unreachable)
            case .settingsConflict:
                var replies = settingsReplies(settingsView(revision: 4, telemetry: true))
                replies["settings.update"] = .error(.rejected(revisionMismatch))
                return connectedWith(normalServices, replies: replies)
            default:
                preconditionFailure("\(id.rawValue) is not a menu bar or Settings scenario")
            }
        }
    }
#endif
