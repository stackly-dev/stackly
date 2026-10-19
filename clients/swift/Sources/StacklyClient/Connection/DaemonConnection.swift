import Foundation
import os

/// One verified connection to `stacklyd` (spec 0013, *Koneksi, handshake, dan
/// request*): the handshake, calls matched by `requestId` with a per call
/// time limit, event subscriptions, and the reason the connection ended.
///
/// The actor can be entered again at every `await`, so each method checks
/// again whether the connection is still open after one.
public actor DaemonConnection {
    /// The stages of a connection.
    private enum Phase {
        /// Only the handshake may be written.
        case handshaking
        /// Calls may be written.
        case open
        /// The connection ended; nothing more is written or read.
        case closed
    }

    /// A call that was given a request slot and waits for its reply.
    private struct Pending {
        let method: String
        /// Whether its frame was handed to the socket, so the daemon may have
        /// run it.
        var written: Bool
        /// The code a time out fails it with: `ipc.call_timeout`, or
        /// `ipc.handshake_timeout` for the handshake.
        let timeoutBody: ErrorBody
        let continuation: CheckedContinuation<Result<Reply, ClientError>, Never>
    }

    /// A call waiting for a request slot.
    private struct SlotWaiter {
        let requestId: String
        let timeoutBody: ErrorBody
        let continuation: CheckedContinuation<Result<Void, ClientError>, Never>
    }

    /// The type of every event stream.
    public typealias EventStream = AsyncThrowingStream<DecodedFrame, any Error>

    private let channel: SocketChannel
    private let logger = Logger(subsystem: "com.ojiepermana.stackly", category: "client")
    private var phase = Phase.handshaking
    private var decoder = FrameDecoder(maxLength: ContractLimits.controlFrameMax)
    private var maxDepth = ContractLimits.maxJsonDepth
    private var maxOutgoing = ContractLimits.controlFrameMax
    private var slotLimit = 1
    private var inFlight = 0
    private var slotWaiters: [SlotWaiter] = []
    private var pending: [String: Pending] = [:]
    /// Request IDs given up on by a time out or a cancel, oldest first. A late
    /// reply for one is dropped quietly.
    private var abandoned: [String] = []
    /// Request IDs whose time ran out before they reached a slot.
    private var expired: Set<String> = []
    private var subscriptions: [UInt32: EventStream.Continuation] = [:]
    /// Streams made when a subscribe reply arrived, before the caller takes
    /// them, so no event can arrive without a stream.
    private var newStreams: [UInt32: EventStream] = [:]
    private var closeReason: ErrorBody?
    private let terminationContinuation: AsyncStream<ErrorBody>.Continuation

    /// Yields the reason the connection closed, once, then finishes.
    public nonisolated let termination: AsyncStream<ErrorBody>

    private init(channel: SocketChannel) {
        self.channel = channel
        (termination, terminationContinuation) = AsyncStream.makeStream(
            of: ErrorBody.self, bufferingPolicy: .bufferingNewest(1))
    }

    /// Verifies the endpoint, connects, and runs the handshake, within
    /// `options.timeout` (5 seconds by default) for both together. No frame
    /// but the handshake is written before `HandshakeAccepted` arrives.
    ///
    /// - Parameters:
    ///   - endpoint: Where the socket is.
    ///   - policy: Whose socket and server to accept.
    ///   - client: Who this client is, sent in the handshake.
    ///   - options: The time limit for connect plus handshake.
    /// - Returns: The open connection and the daemon's handshake reply.
    /// - Throws: The errors of ``UnixSocket/connectVerified(_:policy:)``;
    ///   ``ClientError/rejected(_:)`` with the daemon's error, such as
    ///   `protocol.major_mismatch`; `ipc.handshake_timeout`; or a closed
    ///   connection.
    public static func open(
        endpoint: SocketEndpoint,
        policy: ConnectPolicy,
        client: ClientInfo,
        options: CallOptions = CallOptions(timeout: ContractLimits.handshakeTimeout)
    ) async throws(ClientError) -> (DaemonConnection, HandshakeAccepted) {
        let started = ContinuousClock.now
        let fd = try await UnixSocket.connectVerified(endpoint, policy: policy)
        let remaining = max(.zero, options.timeout - (ContinuousClock.now - started))
        let connection = DaemonConnection(channel: SocketChannel(fd: fd))
        await connection.startReading()
        let accepted = try await connection.handshake(
            client: client,
            options: CallOptions(timeout: remaining, clock: options.clock)
        )
        return (connection, accepted)
    }

    /// Sends `call` and waits for its reply.
    ///
    /// At most `max(1, limits.maxInFlightRequests)` calls run at once; the
    /// rest wait for a slot within their time limit. A time out or a cancel
    /// fails only this call: ``ClientError/outcomeUnknown(_:)`` with
    /// `ipc.call_timeout` once its frame was written, ``ClientError/notSent(_:)``
    /// before.
    ///
    /// - Parameters:
    ///   - call: The method and its params.
    ///   - idempotencyKey: The key of a mutation, or `nil`.
    ///   - options: The time limit for the whole call.
    /// - Returns: The reply, tagged like `call`.
    /// - Throws: ``ClientError``: the daemon's error frame as `rejected`, a
    ///   time out, or a closed connection.
    public func call(
        _ call: Call,
        idempotencyKey: String? = nil,
        options: CallOptions = .standard
    ) async throws(ClientError) -> Reply {
        let method = call.methodName
        guard phase == .open else {
            throw .notSent(closeReason ?? ClientErrorCatalog.notConnected())
        }
        let requestId = UUID().uuidString.lowercased()
        let timeoutBody = ClientErrorCatalog.callTimeout(
            method: method, limit: options.timeout, idempotencyKey: idempotencyKey)
        let timer = startTimer(requestId, options: options)
        defer {
            timer.cancel()
            expired.remove(requestId)
        }
        let slot = await withTaskCancellationHandler {
            await acquireSlot(requestId, timeoutBody: timeoutBody)
        } onCancel: {
            Task { await self.expire(requestId) }
        }
        try slot.get()
        defer { releaseSlot() }
        let request = Request(requestId: requestId, idempotencyKey: idempotencyKey, call: call)
        let result = await send(request, method: method, timeoutBody: timeoutBody, options: options)
        return try result.get()
    }

    /// Subscribes with `params` and returns the daemon's reply plus the stream
    /// of its events, in order. The stream ends when the connection closes.
    ///
    /// - Parameters:
    ///   - params: What to subscribe to.
    ///   - options: The time limit for the subscribe call.
    /// - Throws: The errors of ``call(_:idempotencyKey:options:)``, such as
    ///   `events.too_many_subscriptions`.
    public func subscribe(
        _ params: SubscribeParams,
        options: CallOptions = .standard
    ) async throws(ClientError) -> (Subscribed, EventStream) {
        let reply = try await call(.eventsSubscribe(params), options: options)
        guard case .eventsSubscribe(let subscribed) = reply,
            let stream = newStreams.removeValue(forKey: subscribed.subscriptionId)
        else {
            throw .outcomeUnknown(closeReason ?? ClientErrorCatalog.protocolViolation())
        }
        return (subscribed, stream)
    }

    /// Closes the connection. Waiting calls fail with `ipc.connection_closed`.
    /// Safe to call more than once.
    public func close() {
        terminate(reason: ClientErrorCatalog.connectionClosed())
    }

    /// Closes the socket synchronously from any thread, for app termination.
    /// The connection then ends as if the daemon had closed it.
    public nonisolated func closeNow() {
        channel.close()
    }

    // MARK: - Handshake

    private func handshake(
        client: ClientInfo,
        options: CallOptions
    ) async throws(ClientError) -> HandshakeAccepted {
        let requestId = UUID().uuidString.lowercased()
        let call = Call.daemonHandshake(
            HandshakeParams(
                protocol: ContractInfo.protocolVersion,
                client: client,
                contractDigest: ContractInfo.digest
            ))
        let timer = startTimer(requestId, options: options)
        defer { timer.cancel() }
        let request = Request(requestId: requestId, idempotencyKey: nil, call: call)
        let result = await send(
            request,
            method: call.methodName,
            timeoutBody: ClientErrorCatalog.handshakeTimeout(),
            options: options
        )
        let reply: Reply
        switch result {
        case .success(let value):
            reply = value
        case .failure(let error):
            terminate(reason: error.body)
            throw error
        }
        guard phase == .handshaking, case .daemonHandshake(let accepted) = reply else {
            terminate(reason: ClientErrorCatalog.protocolViolation())
            throw .outcomeUnknown(ClientErrorCatalog.protocolViolation())
        }
        phase = .open
        decoder.setMaxLength(accepted.limits.maxFrameBytes)
        maxDepth = accepted.limits.maxJsonDepth
        maxOutgoing = accepted.limits.maxFrameBytes
        slotLimit = max(1, Int(accepted.limits.maxInFlightRequests))
        if accepted.contractDrift {
            logger.warning("stacklyd has the same protocol but another contract digest")
        }
        logger.info("connected to stacklyd")
        return accepted
    }

    // MARK: - Writing

    /// Registers `request` as pending, writes it, and waits for its reply.
    private func send(
        _ request: Request,
        method: String,
        timeoutBody: ErrorBody,
        options: CallOptions
    ) async -> Result<Reply, ClientError> {
        guard phase != .closed else {
            return .failure(.notSent(closeReason ?? ClientErrorCatalog.connectionClosed()))
        }
        guard expired.contains(request.requestId) == false else {
            return .failure(.notSent(timeoutBody))
        }
        let frame: [UInt8]
        do {
            let payload = try StacklyJSON.encoder().encode(request)
            frame = try FrameEncoder.encode(Array(payload), maxLength: maxOutgoing)
        } catch let error as FrameError {
            return .failure(.notSent(ClientErrorCatalog.outgoing(error)))
        } catch {
            return .failure(.notSent(ClientErrorCatalog.protocolViolation()))
        }
        let requestId = request.requestId
        return await withTaskCancellationHandler {
            await withCheckedContinuation { continuation in
                pending[requestId] = Pending(
                    method: method,
                    written: true,
                    timeoutBody: timeoutBody,
                    continuation: continuation
                )
                Task { await self.write(frame, requestId: requestId, options: options) }
            }
        } onCancel: {
            Task { await self.expire(requestId) }
        }
    }

    private func write(_ frame: [UInt8], requestId: String, options: CallOptions) async {
        guard phase != .closed else { return }
        let written = await channel.write(frame)
        guard written == false, pending[requestId] != nil else { return }
        // The daemon may have answered with an error before it closed: keep
        // reading for a moment and report that error if it comes.
        try? await options.clock.sleep(for: ContractLimits.pendingReadTimeout)
        if pending[requestId] != nil {
            terminate(reason: ClientErrorCatalog.connectionClosed())
        }
    }

    // MARK: - Slots and time limits

    private func acquireSlot(
        _ requestId: String,
        timeoutBody: ErrorBody
    ) async -> Result<Void, ClientError> {
        guard phase == .open else {
            return .failure(.notSent(closeReason ?? ClientErrorCatalog.connectionClosed()))
        }
        guard expired.contains(requestId) == false, Task.isCancelled == false else {
            return .failure(.notSent(timeoutBody))
        }
        if inFlight < slotLimit {
            inFlight += 1
            return .success(())
        }
        return await withCheckedContinuation { continuation in
            slotWaiters.append(
                SlotWaiter(
                    requestId: requestId, timeoutBody: timeoutBody, continuation: continuation))
        }
    }

    private func releaseSlot() {
        if slotWaiters.isEmpty {
            inFlight = max(0, inFlight - 1)
        } else {
            slotWaiters.removeFirst().continuation.resume(returning: .success(()))
        }
    }

    private func startTimer(_ requestId: String, options: CallOptions) -> Task<Void, Never> {
        Task {
            do {
                try await options.clock.sleep(for: options.timeout)
            } catch {
                return
            }
            self.expire(requestId)
        }
    }

    /// Fails the call `requestId` because its time ran out or its task was
    /// cancelled, wherever it is.
    private func expire(_ requestId: String) {
        if let index = slotWaiters.firstIndex(where: { $0.requestId == requestId }) {
            let waiter = slotWaiters.remove(at: index)
            waiter.continuation.resume(returning: .failure(.notSent(waiter.timeoutBody)))
            return
        }
        guard let entry = pending.removeValue(forKey: requestId) else {
            expired.insert(requestId)
            return
        }
        if entry.written {
            abandon(requestId)
            entry.continuation.resume(returning: .failure(.outcomeUnknown(entry.timeoutBody)))
        } else {
            entry.continuation.resume(returning: .failure(.notSent(entry.timeoutBody)))
        }
    }

    private func abandon(_ requestId: String) {
        abandoned.append(requestId)
        let limit = 2 * slotLimit
        if abandoned.count > limit {
            abandoned.removeFirst(abandoned.count - limit)
        }
    }

    // MARK: - Reading

    private func startReading() {
        Task { await self.readLoop() }
    }

    private func readLoop() async {
        while phase != .closed {
            guard let chunk = await channel.read() else {
                terminate(reason: ClientErrorCatalog.connectionClosed())
                return
            }
            guard phase != .closed else { return }
            var refused: FrameError?
            do {
                try decoder.push(chunk)
            } catch {
                refused = error
            }
            while phase != .closed, let payload = decoder.nextFrame() {
                handle(payload)
            }
            if let refused {
                terminate(reason: ClientErrorCatalog.frame(refused))
                return
            }
        }
    }

    private func handle(_ payload: [UInt8]) {
        let frame: DecodedFrame
        do {
            frame = try ServerFrameDecoder.decode(payload, maxDepth: maxDepth)
        } catch {
            terminate(reason: error.body)
            return
        }
        switch frame {
        case .response(let response):
            resolve(response)
        case .error(let errorFrame):
            resolve(errorFrame)
        case .event(let event):
            subscriptions[event.subscriptionId]?.yield(frame)
        case .skipped(let subscriptionId, _, _):
            subscriptions[subscriptionId]?.yield(frame)
        }
    }

    private func resolve(_ response: Response) {
        let requestId = response.requestId
        guard let entry = pending.removeValue(forKey: requestId) else {
            if let index = abandoned.firstIndex(of: requestId) {
                abandoned.remove(at: index)
            } else {
                terminate(reason: ClientErrorCatalog.protocolViolation())
            }
            return
        }
        guard response.reply.methodName == entry.method else {
            pending[requestId] = entry
            terminate(reason: ClientErrorCatalog.protocolViolation())
            return
        }
        if case .eventsSubscribe(let subscribed) = response.reply {
            let (stream, continuation) = EventStream.makeStream()
            subscriptions[subscribed.subscriptionId] = continuation
            newStreams[subscribed.subscriptionId] = stream
        }
        entry.continuation.resume(returning: .success(response.reply))
    }

    private func resolve(_ errorFrame: ErrorFrame) {
        guard let requestId = errorFrame.requestId else {
            // An error for the whole connection: before the handshake it
            // refuses the handshake, after it every waiting call.
            if phase == .handshaking {
                for (id, entry) in pending {
                    pending[id] = nil
                    entry.continuation.resume(returning: .failure(.rejected(errorFrame.error)))
                }
                terminate(reason: errorFrame.error)
            } else {
                terminate(reason: errorFrame.error, callsFailWith: errorFrame.error)
            }
            return
        }
        if let entry = pending.removeValue(forKey: requestId) {
            entry.continuation.resume(returning: .failure(.rejected(errorFrame.error)))
        } else if let index = abandoned.firstIndex(of: requestId) {
            abandoned.remove(at: index)
        } else {
            terminate(reason: ClientErrorCatalog.protocolViolation())
        }
    }

    // MARK: - Ending

    /// Ends the connection for `reason`: written calls fail as
    /// `outcomeUnknown`, unwritten ones as `notSent`, both with
    /// `callsFailWith` (`ipc.connection_closed` unless the daemon sent an
    /// error for the connection), and every event stream ends.
    private func terminate(reason: ErrorBody, callsFailWith: ErrorBody? = nil) {
        guard phase != .closed else { return }
        phase = .closed
        closeReason = reason
        channel.close()
        let body = callsFailWith ?? ClientErrorCatalog.connectionClosed()
        for entry in pending.values {
            entry.continuation.resume(
                returning: .failure(entry.written ? .outcomeUnknown(body) : .notSent(body)))
        }
        pending.removeAll()
        for waiter in slotWaiters {
            waiter.continuation.resume(returning: .failure(.notSent(body)))
        }
        slotWaiters.removeAll()
        for continuation in subscriptions.values {
            continuation.finish(throwing: ClientError.outcomeUnknown(reason))
        }
        subscriptions.removeAll()
        newStreams.removeAll()
        logger.info("connection to stacklyd closed: \(reason.code, privacy: .public)")
        terminationContinuation.yield(reason)
        terminationContinuation.finish()
    }
}

extension ClientErrorCatalog {
    /// `ipc.frame_empty` or `ipc.frame_too_large` for a request the client
    /// refused before writing anything.
    static func outgoing(_ error: FrameError) -> ErrorBody {
        switch error {
        case .empty:
            body(Spec.frameEmpty, [:], message: "the request is empty")
        case .tooLarge(let length, let max):
            body(
                Spec.frameTooLarge, ["length": String(length), "max": String(max)],
                message: "the request is above the frame size limit")
        }
    }
}
