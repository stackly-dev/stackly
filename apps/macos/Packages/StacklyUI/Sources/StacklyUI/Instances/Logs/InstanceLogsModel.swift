import Foundation
import Observation
import StacklyClient

/// The Logs tab of one instance (spec 0027, *Layar Logs*): the loaded rows, the source and
/// filter, Follow, the cursors, and the active follow.
///
/// Opening reads the newest page and, with Follow on, follows after `later ?? head`, so
/// nothing is missed or shown twice between the two. A change of source or filter reads
/// again from the newest page. The model keeps at most ``maxLines`` rows: Load Earlier past
/// it drops the newest rows and turns Follow off, a follow past it drops the oldest. When
/// the connection closes the rows stay and the follow waits for ``reconnected()``, which
/// follows again from ``resumeAfter``.
@Observable
final class InstanceLogsModel {
    /// Which streams the tab shows.
    enum Source: String, CaseIterable, Identifiable, Sendable {
        case all
        case stdout
        case stderr

        var id: String { rawValue }

        /// The title in the segmented control.
        var title: String {
            switch self {
            case .all: "All"
            case .stdout: "stdout"
            case .stderr: "stderr"
            }
        }

        /// The `streams` of a read or follow; `nil` is both.
        var streams: [LogStream]? {
            switch self {
            case .all: nil
            case .stdout: [.stdout]
            case .stderr: [.stderr]
            }
        }
    }

    /// Where the tab stands.
    enum Phase: Equatable, Sendable {
        /// The first page has not arrived.
        case loading
        /// A page arrived; the rows may be empty.
        case ready
        /// The daemon could not read the store, or refused the read, with why.
        case unreadable(String)
    }

    /// The most rows the tab holds.
    static let maxLines = 10_000
    /// The entries of one page.
    static let pageLimit: UInt32 = 200
    /// The pause after a change of the filter text before it reads again.
    static let queryDebounce = Duration.milliseconds(300)
    /// The longest filter the daemon takes, in characters (`LOG_FILTER_MAX_CHARS`).
    static let filterMaxCharacters = 256

    /// The instance.
    let instanceId: UUID

    /// The loaded rows, oldest first.
    private(set) var rows: [ShownLog] = []
    /// The streams shown.
    private(set) var source = Source.all
    /// The filter text as typed.
    private(set) var query = ""
    /// Whether new entries are followed.
    private(set) var follow = true
    /// Where the tab stands.
    private(set) var phase = Phase.loading
    /// The cursor Load Earlier reads before, or `nil` when nothing older is left.
    private(set) var earlier: LogCursor?
    /// Whether the last read stopped at its scan budget.
    private(set) var scanLimited = false
    /// How much the store keeps, from the last page.
    private(set) var retention: LogRetention?
    /// Whether the connection closed under the tab; the rows stay.
    private(set) var isOffline = false
    /// Whether a Load Earlier read runs.
    private(set) var isLoadingEarlier = false
    /// Why the last follow was refused, such as too many subscriptions, or `nil`.
    private(set) var followFailure: String?

    /// Where a follow or reconnect continues: `later ?? head` of the last page, or the `seq`
    /// of the last frame. With a filter it differs from the newest row.
    @ObservationIgnored private(set) var resumeAfter: LogCursor?

    @ObservationIgnored private let service: InstanceLogsService
    @ObservationIgnored private let sleep: @Sendable (Duration) async throws -> Void
    /// The zone rows are mapped in from now on; the tab sets it from its environment, so a
    /// change of the system zone applies to the rows loaded after it.
    @ObservationIgnored var timeZone: TimeZone
    /// The locale of the day separators; the tab sets it from its environment.
    @ObservationIgnored var locale: Locale
    @ObservationIgnored private var mapper: LogLineMapper?
    @ObservationIgnored private var storeId: UUID?
    @ObservationIgnored private var generation = 0
    /// How many views show the tab now; the follow stops when the last one leaves.
    @ObservationIgnored private var viewers = 0
    @ObservationIgnored private var load: Task<Void, Never>?
    @ObservationIgnored private var earlierLoad: Task<Void, Never>?
    @ObservationIgnored private var following: Task<Void, Never>?
    @ObservationIgnored private var debounce: Task<Void, Never>?
    @ObservationIgnored private var active: LogFollowSession?

    /// Creates the model of one instance's Logs tab.
    /// - Parameters:
    ///   - instanceId: The instance.
    ///   - service: The reads and follows.
    ///   - source: The streams shown first.
    ///   - query: The filter text first.
    ///   - timeZone: The zone rows are mapped in first.
    ///   - locale: The locale of the day separators.
    ///   - sleep: Waits for a duration and throws when cancelled.
    init(
        instanceId: UUID,
        service: InstanceLogsService,
        source: Source = .all,
        query: String = "",
        timeZone: TimeZone = .current,
        locale: Locale = .current,
        sleep: @escaping @Sendable (Duration) async throws -> Void = {
            try await Task.sleep(for: $0)
        }
    ) {
        self.instanceId = instanceId
        self.service = service
        self.source = source
        self.query = query
        self.timeZone = timeZone
        self.locale = locale
        self.sleep = sleep
    }

    // MARK: - Derived

    /// The rows as the log view draws them, with the day separators.
    var lines: [LogLine] { LogLineMapper.lines(rows) }

    /// Whether a source or a filter narrows what is shown.
    var isFiltered: Bool { source != .all || filterText != nil }

    /// The text Copy Shown Lines writes: every loaded row, without day separators.
    var shownText: String { rows.map(\.copyText).joined(separator: "\n") }

    /// What Copy Line writes for a drawn line, `nil` for a day separator.
    func copyText(of line: LogLine) -> String? {
        rows.last { $0.line.id == line.id }?.copyText
    }

    /// The `seq` of the newest loaded entry, for the footer.
    var newestSeq: UInt64? { rows.last { $0.seq != nil }?.seq }

    /// The filter the daemon gets: trimmed, without line breaks, at most 256 characters, or
    /// `nil` when empty.
    var filterText: String? {
        let flat = query.split(whereSeparator: \.isNewline).joined(separator: " ")
        let trimmed = flat.trimmingCharacters(in: .whitespaces)
        return trimmed.isEmpty ? nil : String(trimmed.prefix(Self.filterMaxCharacters))
    }

    // MARK: - Intents

    /// A view shows the tab: reads the newest page and follows when Follow is on.
    func open() {
        viewers += 1
        reload()
    }

    /// A view stopped showing the tab. When none is left, every read and the follow stop; the
    /// rows stay. A view that appears before the last one goes keeps them running.
    func close() {
        viewers = max(0, viewers - 1)
        guard viewers == 0 else { return }
        stop()
    }

    /// Stops every read and the follow whoever shows the tab, for an instance that is gone.
    func stop() {
        viewers = 0
        generation += 1
        load?.cancel()
        earlierLoad?.cancel()
        debounce?.cancel()
        stopFollowing()
    }

    /// Shows other streams: reads again from the newest page.
    func setSource(_ source: Source) {
        guard source != self.source else { return }
        self.source = source
        reload()
    }

    /// Changes the filter text: reads again after ``queryDebounce``.
    func setQuery(_ query: String) {
        guard query != self.query else { return }
        self.query = query
        debounce?.cancel()
        let sleep = self.sleep
        debounce = Task { [weak self] in
            do { try await sleep(Self.queryDebounce) } catch { return }
            self?.reload()
        }
    }

    /// Turns Follow on (reads the newest page, then follows) or off (stops the follow and
    /// keeps the rows).
    func setFollow(_ on: Bool) {
        guard on != follow else { return }
        follow = on
        followFailure = nil
        if on {
            reload()
        } else {
            stopFollowing()
        }
    }

    /// Reads the page before the oldest loaded row.
    func loadEarlier() {
        guard let earlier, isLoadingEarlier == false else { return }
        isLoadingEarlier = true
        let gen = generation
        let params = readParams(.before(LogAnchor(cursor: earlier)))
        let service = self.service
        earlierLoad = Task { [weak self] in
            let result: Result<LogPage, ClientError>
            do throws(ClientError) { result = .success(try await service.read(params)) } catch {
                result = .failure(error)
            }
            guard let self, gen == self.generation else { return }
            self.isLoadingEarlier = false
            switch result {
            case .success(let page): self.prepend(page)
            case .failure(let error): self.failed(error)
            }
        }
    }

    /// Reads again after a failure ("Try Again").
    func retry() {
        reload()
    }

    /// The shell caught up after a reconnect: a tab whose connection closed follows again
    /// from ``resumeAfter``, or reads again when it has no page yet.
    func reconnected() {
        guard isOffline else { return }
        isOffline = false
        if phase != .ready || resumeAfter == nil {
            reload()
        } else if follow {
            startFollowing()
        }
    }

    // MARK: - Reads

    /// Drops the rows and reads the newest page, then follows when Follow is on.
    private func reload() {
        generation += 1
        load?.cancel()
        earlierLoad?.cancel()
        debounce?.cancel()
        stopFollowing()
        rows = []
        earlier = nil
        resumeAfter = nil
        scanLimited = false
        isLoadingEarlier = false
        isOffline = false
        phase = .loading
        let gen = generation
        let params = readParams(.newest)
        let service = self.service
        load = Task { [weak self] in
            let result: Result<LogPage, ClientError>
            do throws(ClientError) { result = .success(try await service.read(params)) } catch {
                result = .failure(error)
            }
            guard let self, gen == self.generation else { return }
            switch result {
            case .success(let page):
                self.receiveNewest(page)
                if self.follow { self.startFollowing() }
            case .failure(let error):
                self.failed(error)
            }
        }
    }

    private func readParams(_ position: LogPosition) -> LogsReadParams {
        LogsReadParams(
            instance: instanceId, streams: source.streams, contains: filterText,
            position: position, limit: Self.pageLimit)
    }

    private func receiveNewest(_ page: LogPage) {
        storeId = page.storeId
        retention = page.retention
        rows = currentMapper().shown(
            entries: page.entries ?? [], gap: page.gap, head: page.head?.seq)
        earlier = page.earlier
        resumeAfter = page.later ?? page.head
        scanLimited = page.scanLimited ?? false
        phase = .ready
    }

    private func prepend(_ page: LogPage) {
        retention = page.retention
        let older = currentMapper().shown(
            entries: page.entries ?? [], gap: page.gap, head: rows.first?.seq)
        rows = older + rows
        earlier = page.earlier
        scanLimited = page.scanLimited ?? false
        if rows.count > Self.maxLines {
            rows.removeLast(rows.count - Self.maxLines)
            follow = false
            stopFollowing()
            resumeAfter = nil
        }
    }

    private func failed(_ error: ClientError) {
        switch error {
        case .notSent, .outcomeUnknown:
            isOffline = true
            if phase == .loading { phase = .ready }
        case .rejected(let body):
            phase = .unreadable(body.detail("reason") ?? body.message)
        }
    }

    // MARK: - Follow

    private func startFollowing() {
        stopFollowing()
        let gen = generation
        let params = LogsFollowParams(
            instance: instanceId, streams: source.streams, contains: filterText,
            after: resumeAfter)
        let service = self.service
        following = Task { [weak self] in
            let follow: LogFollowSession
            do throws(ClientError) {
                follow = try await service.follow(params)
            } catch {
                self?.followRefused(error, generation: gen)
                return
            }
            guard let self, gen == self.generation, Task.isCancelled == false else {
                await follow.unfollow()
                return
            }
            self.active = follow
            self.followFailure = nil
            if self.resumeAfter == nil { self.resumeAfter = follow.following.head }
            do {
                for try await frame in follow.frames {
                    guard gen == self.generation else { break }
                    if case .log(let log) = frame, self.receive(log) == false { break }
                }
            } catch {
                // The connection closed: the rows stay until the shell catches up again.
                if gen == self.generation, Task.isCancelled == false {
                    self.active = nil
                    self.isOffline = true
                }
            }
        }
    }

    private func followRefused(_ error: ClientError, generation gen: Int) {
        guard gen == generation else { return }
        switch error {
        case .notSent, .outcomeUnknown:
            isOffline = true
        case .rejected(let body):
            followFailure = body.message
            follow = false
        }
    }

    /// Takes one frame; `false` when the follow ended.
    private func receive(_ frame: LogFrame) -> Bool {
        if frame.ended != nil {
            active = nil
            return false
        }
        if let id = frame.storeId { storeId = id }
        let entries = frame.entries ?? []
        let added = currentMapper().shown(
            entries: entries, gap: frame.gap, head: resumeAfter?.seq)
        if let last = entries.last, let store = storeId {
            resumeAfter = LogCursor(storeId: store, seq: last.seq)
        }
        guard added.isEmpty == false else { return true }
        rows.append(contentsOf: added)
        if rows.count > Self.maxLines {
            rows.removeFirst(rows.count - Self.maxLines)
            if let oldest = rows.first(where: { $0.seq != nil })?.seq, let store = storeId {
                earlier = LogCursor(storeId: store, seq: oldest)
            }
        }
        return true
    }

    private func stopFollowing() {
        following?.cancel()
        following = nil
        if let active {
            self.active = nil
            Task { await active.unfollow() }
        }
    }

    /// The mapper of the current time zone, made again when the zone changed.
    private func currentMapper() -> LogLineMapper {
        if let mapper, mapper.timeZone == timeZone, mapper.locale == locale { return mapper }
        let made = LogLineMapper(timeZone: timeZone, locale: locale)
        mapper = made
        return made
    }
}
