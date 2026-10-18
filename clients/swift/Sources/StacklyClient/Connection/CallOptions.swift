/// How long a call may take, on which clock.
public struct CallOptions: Sendable {
    /// The limit for the whole call, including the wait for a request slot.
    public var timeout: Duration
    /// The clock the limit runs on; tests inject a manual one.
    public var clock: any Clock<Duration>

    /// Options with `timeout` on `clock`.
    public init(timeout: Duration, clock: any Clock<Duration> = ContinuousClock()) {
        self.timeout = timeout
        self.clock = clock
    }

    /// 10 seconds on the continuous clock.
    public static var standard: CallOptions {
        CallOptions(timeout: ContractLimits.callTimeout)
    }
}
