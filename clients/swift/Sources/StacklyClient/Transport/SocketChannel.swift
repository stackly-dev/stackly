import Darwin
import Dispatch
import Synchronization

/// The byte stream of one connected, non blocking socket.
///
/// One read of up to ``ContractLimits/readChunk`` bytes runs at a time: the
/// caller asks for the next chunk only after it handled the last one, so a
/// fast daemon cannot fill memory. Reads and writes wait on dispatch sources
/// on a private serial queue, never on the main thread or the cooperative
/// pool. `DispatchIO` is not used for reads: its stream read reports `done`
/// only at EOF or after the full length, so a daemon that sends one small
/// frame and waits would never complete a read.
///
/// `@unchecked Sendable`: every mutable property is only touched on `queue`,
/// except the closed flag, which is a `Mutex`. ``close()`` may be called from
/// any thread.
final class SocketChannel: @unchecked Sendable {
    private let fd: Int32
    private let queue = DispatchQueue(label: "com.ojiepermana.stackly.client.channel")
    private let readSource: DispatchSourceRead
    private let writeSource: DispatchSourceWrite
    private let closed = Mutex(false)

    /// The read waiting for the socket to be readable. Queue only.
    private var readWaiter: CheckedContinuation<[UInt8]?, Never>?
    /// Writes not fully written yet, in order. Queue only.
    private var writes: [PendingWrite] = []
    /// Whether the write side failed, so later writes fail at once. Queue only.
    private var writeFailed = false
    /// Whether the channel finished: sources cancelled, waiters failed. Queue
    /// only.
    private var finished = false
    private var readSuspended = true
    private var writeSuspended = true

    /// One write and how far it got.
    private struct PendingWrite {
        var bytes: [UInt8]
        var offset: Int
        let continuation: CheckedContinuation<Bool, Never>
    }

    /// Takes over `fd`, a connected non blocking socket. The channel closes it
    /// once both sources are cancelled.
    init(fd: Int32) {
        self.fd = fd
        readSource = DispatchSource.makeReadSource(fileDescriptor: fd, queue: queue)
        writeSource = DispatchSource.makeWriteSource(fileDescriptor: fd, queue: queue)
        let remaining = CancelCount(2)
        let cancelled: @Sendable () -> Void = {
            remaining.value -= 1
            if remaining.value == 0 { Darwin.close(fd) }
        }
        readSource.setCancelHandler(handler: cancelled)
        writeSource.setCancelHandler(handler: cancelled)
        readSource.setEventHandler { [weak self] in self?.readReady() }
        writeSource.setEventHandler { [weak self] in self?.drainWrites() }
    }

    deinit {
        // Nothing else holds the channel now. A suspended source must be
        // resumed before it can be cancelled and released.
        if readSuspended { readSource.resume() }
        if writeSuspended { writeSource.resume() }
        readSource.cancel()
        writeSource.cancel()
    }

    /// Reads the next chunk: up to ``ContractLimits/readChunk`` bytes, as soon
    /// as any are available. `nil` at EOF, after a read error, or once the
    /// channel is closed.
    func read() async -> [UInt8]? {
        await withCheckedContinuation { continuation in
            queue.async { self.startRead(continuation) }
        }
    }

    /// Writes all of `bytes`, in order after earlier writes. `false` when the
    /// write side failed or the channel is closed.
    func write(_ bytes: [UInt8]) async -> Bool {
        await withCheckedContinuation { continuation in
            queue.async {
                guard self.finished == false, self.writeFailed == false else {
                    continuation.resume(returning: false)
                    return
                }
                self.writes.append(
                    PendingWrite(bytes: bytes, offset: 0, continuation: continuation))
                self.drainWrites()
            }
        }
    }

    /// Closes the channel: a waiting read returns `nil`, waiting writes fail,
    /// and the socket closes. Safe to call more than once and from any thread,
    /// so the app can close it synchronously while it terminates.
    func close() {
        let first = closed.withLock { isClosed in
            defer { isClosed = true }
            return isClosed == false
        }
        guard first else { return }
        Darwin.shutdown(fd, SHUT_RDWR)
        queue.async { self.finish() }
    }

    private func startRead(_ continuation: CheckedContinuation<[UInt8]?, Never>) {
        guard finished == false else {
            continuation.resume(returning: nil)
            return
        }
        switch readNow() {
        case .bytes(let bytes):
            continuation.resume(returning: bytes)
        case .wouldBlock:
            readWaiter = continuation
            if readSuspended {
                readSuspended = false
                readSource.resume()
            }
        case .ended:
            continuation.resume(returning: nil)
        }
    }

    private func readReady() {
        guard let waiter = readWaiter else {
            suspendRead()
            return
        }
        switch readNow() {
        case .bytes(let bytes):
            readWaiter = nil
            suspendRead()
            waiter.resume(returning: bytes)
        case .wouldBlock:
            break
        case .ended:
            readWaiter = nil
            suspendRead()
            waiter.resume(returning: nil)
        }
    }

    private enum ReadOutcome {
        case bytes([UInt8])
        case wouldBlock
        case ended
    }

    private func readNow() -> ReadOutcome {
        var buffer = [UInt8](repeating: 0, count: ContractLimits.readChunk)
        let count = buffer.withUnsafeMutableBytes { Darwin.read(fd, $0.baseAddress, $0.count) }
        if count > 0 {
            buffer.removeSubrange(count...)
            return .bytes(buffer)
        }
        if count < 0, errno == EAGAIN || errno == EINTR {
            return .wouldBlock
        }
        return .ended
    }

    private func suspendRead() {
        guard readSuspended == false else { return }
        readSuspended = true
        readSource.suspend()
    }

    private func drainWrites() {
        while var first = writes.first {
            let remaining = first.bytes.count - first.offset
            let count = first.bytes.withUnsafeBytes { buffer in
                Darwin.write(fd, buffer.baseAddress.map { $0 + first.offset }, remaining)
            }
            if count < 0 {
                if errno == EAGAIN || errno == EINTR {
                    if writeSuspended {
                        writeSuspended = false
                        writeSource.resume()
                    }
                    return
                }
                failWrites()
                return
            }
            first.offset += count
            if first.offset < first.bytes.count {
                writes[0] = first
                continue
            }
            writes.removeFirst()
            first.continuation.resume(returning: true)
        }
        if writeSuspended == false {
            writeSuspended = true
            writeSource.suspend()
        }
    }

    private func failWrites() {
        writeFailed = true
        let failed = writes
        writes.removeAll()
        for write in failed {
            write.continuation.resume(returning: false)
        }
    }

    private func finish() {
        guard finished == false else { return }
        finished = true
        readWaiter?.resume(returning: nil)
        readWaiter = nil
        failWrites()
        if readSuspended {
            readSuspended = false
            readSource.resume()
        }
        if writeSuspended {
            writeSuspended = false
            writeSource.resume()
        }
        readSource.cancel()
        writeSource.cancel()
    }
}

/// How many sources still have to be cancelled before the socket closes.
///
/// `@unchecked Sendable`: both cancel handlers run on the channel's serial
/// queue, so `value` is never touched concurrently.
private final class CancelCount: @unchecked Sendable {
    var value: Int

    init(_ value: Int) {
        self.value = value
    }
}
