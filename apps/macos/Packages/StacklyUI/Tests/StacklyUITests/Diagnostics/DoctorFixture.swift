import Foundation
import StacklyClient

@testable import StacklyUI

/// The doctor reports of the tests: the `doctor.run` contract fixtures in
/// `protocols/fixtures/ipc/`, and small reports built from JSON for the model's rules.
enum DoctorFixture {
    /// The instance `pg-main` of the contract fixture.
    static var pgMain: UUID {
        get throws { try id("0192f3a4-0000-7000-8000-0000000000b2") }
    }
    /// The instance `mysql-legacy` of the contract fixture.
    static var mysqlLegacy: UUID {
        get throws { try id("0192f3a4-0000-7000-8000-0000000000b1") }
    }

    private static func id(_ text: String) throws -> UUID {
        guard let id = UUID(uuidString: text) else {
            throw ContractFixture.Mismatch(description: "\(text) is not a UUID")
        }
        return id
    }

    /// The mixed report of `frame-response-doctor-run.json`: 3 errors and 5 warnings.
    static func mixed() throws -> DoctorReport {
        try report("frame-response-doctor-run")
    }

    /// The report of `frame-response-doctor-run-unknown-values.json`: values of a newer
    /// daemon, in recovery.
    static func unknownValues() throws -> DoctorReport {
        try report("frame-response-doctor-run-unknown-values")
    }

    /// A report with one passing check, finished at `finishedAt`, and a `disk_space` check
    /// whose capacity was checked at `checkedAt` when one is given.
    static func clean(finishedAt: Date, checkedAt: Date? = nil) throws -> DoctorReport {
        let finished = ISO8601DateFormatter().string(from: finishedAt)
        var checks = [
            #"{"id": "daemon", "severity": "ok", "title": "stacklyd", "summary": "stacklyd 0.9.0", "findings": [], "omitted": 0, "capacity": null}"#
        ]
        if let checkedAt {
            let checked = ISO8601DateFormatter().string(from: checkedAt)
            checks.append(
                #"{"id": "disk_space", "severity": "ok", "title": "Disk space", "summary": "40% used", "findings": [], "omitted": 0, "capacity": {"checkedAt": ""#
                    + checked
                    + #"", "totalBytes": 1000, "availableBytes": 600, "usedPercent": 40, "level": "ok"}}"#
            )
        }
        let json =
            #"{"runId": "0192f3a7-0000-7000-8000-0000000000e9", "startedAt": ""# + finished
            + #"", "finishedAt": ""# + finished
            + #"", "recovery": false, "scope": null, "errors": 0, "warnings": 0, "checks": ["#
            + checks.joined(separator: ", ") + "]}"
        return try StacklyJSON.decoder().decode(DoctorReport.self, from: Data(json.utf8))
    }

    private static func report(_ name: String) throws -> DoctorReport {
        guard case .doctorRun(let report) = try ContractFixture.reply(name) else {
            throw ContractFixture.Mismatch(description: "\(name) is not a doctor.run reply")
        }
        return report
    }
}
