#if DEBUG
    import Foundation
    import StacklyClient

    /// Doctor and bundle models in each state of the Diagnostics fixtures (spec 0034, AC-21),
    /// for previews and snapshots. The daemon is a closure: a reply, an error, or a call that
    /// never answers, so a state holds still.
    @MainActor
    enum FixtureDoctorModels {
        /// A state of the Doctor section.
        enum Doctor {
            /// No run yet.
            case neverRun
            /// The first run is going.
            case running
            /// The last run gave `report`.
            case report(DoctorReport)
            /// The first run failed.
            case failed
            /// The daemon does not know `doctor.run`.
            case unsupported
        }

        /// A state of the bundle sheet.
        enum Bundle {
            /// The daemon is building the preview.
            case loading
            /// The preview is there.
            case previewed
            /// The preview failed.
            case failed
        }

        /// A doctor model in `state`.
        static func doctor(_ state: Doctor) async -> DoctorModel {
            let read: @MainActor (Call) async throws(ClientError) -> Reply
            switch state {
            case .neverRun, .running:
                read = { _ throws(ClientError) in
                    try? await Task.sleep(for: .seconds(3600))
                    throw .notSent(ClientErrorCatalog.notConnected())
                }
            case .report(let report):
                read = { _ throws(ClientError) in .doctorRun(report) }
            case .failed:
                read = { _ throws(ClientError) in throw .outcomeUnknown(FixtureDoctor.runFailed) }
            case .unsupported:
                read = { _ throws(ClientError) in
                    throw .rejected(
                        ErrorBody(
                            code: "request.unknown_method", category: .unsupported,
                            message: "stacklyd does not know doctor.run", retryable: false,
                            resource: nil, fields: nil, hint: nil, details: nil,
                            operationId: nil, rule: nil, items: nil))
                }
            }
            let model = DoctorModel(read: read, now: { FixtureClock.now })
            switch state {
            case .neverRun:
                break
            case .running:
                Task { await model.request(.user) }
                while model.running == false { await Task.yield() }
            case .report, .failed, .unsupported:
                await model.request(.user)
            }
            return model
        }

        /// A bundle model in `state`.
        static func bundle(_ state: Bundle) async -> BundleModel {
            let read: @MainActor (Call) async throws(ClientError) -> Reply
            switch state {
            case .loading:
                read = { _ throws(ClientError) in
                    try? await Task.sleep(for: .seconds(3600))
                    throw .notSent(ClientErrorCatalog.notConnected())
                }
            case .previewed:
                read = { _ throws(ClientError) in .diagnosticsPreviewBundle(FixtureDoctor.preview) }
            case .failed:
                read = { _ throws(ClientError) in throw .rejected(FixtureDoctor.bundleFailed) }
            }
            let model = BundleModel(read: read)
            switch state {
            case .loading:
                Task { await model.load() }
                while model.loading == false { await Task.yield() }
            case .previewed, .failed:
                await model.load()
            }
            return model
        }
    }
#endif
