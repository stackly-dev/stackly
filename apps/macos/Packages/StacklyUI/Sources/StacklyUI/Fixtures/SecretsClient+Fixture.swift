#if DEBUG
    import StacklyClient

    extension SecretsClient {
        /// A secrets client that answers from `scenario.replies` and fails every other call
        /// with `ipc.not_connected`. `secret.reveal` is never answered, and no fixture holds a
        /// secret value (spec 0068).
        public static func fixture(_ scenario: FixtureScenario) -> SecretsClient {
            SecretsClient { call, _ throws(ClientError) in
                let method = call.methodName
                guard method != "secret.reveal", let reply = scenario.replies[method] else {
                    throw .notSent(ClientErrorCatalog.notConnected())
                }
                switch reply {
                case .reply(let value): return value
                case .error(let error): throw error
                }
            }
        }
    }
#endif
