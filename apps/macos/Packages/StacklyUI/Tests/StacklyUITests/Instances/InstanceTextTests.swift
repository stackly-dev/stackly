import Foundation
import StacklyClient
import Testing

@testable import StacklyUI

/// The port text, the uptime, and the age (spec 0022, *Halaman Instances*).
struct InstanceTextTests {
    private func endpoint(
        _ name: String = "main", transport: EndpointTransport = .tcp, host: String? = "127.0.0.1",
        port: UInt16? = 5432, socketPath: String? = nil
    ) -> EndpointStatus {
        EndpointStatus(
            name: name, transport: transport, host: host, port: port,
            portMode: port == nil ? .auto : .fixed, bound: true, socketPath: socketPath)
    }

    @Test func portsAreWrittenPerEndpointInTheOrderOfTheDaemon() {
        #expect(InstanceText.ports([endpoint(port: 4222), endpoint(port: 8222)]) == ":4222 :8222")
        #expect(InstanceText.ports([endpoint(port: nil)]) == "auto")
        #expect(InstanceText.ports([endpoint(transport: .unix, host: nil, port: nil)]) == "socket")
        #expect(InstanceText.ports([]) == "—")
        #expect(InstanceText.ports(nil) == "—")
    }

    @Test func theFirstPortIsANumberOnlyForAFixedOrBoundPort() {
        #expect(InstanceText.firstPort([endpoint(port: 5432)]) == 5432)
        #expect(InstanceText.firstPort([endpoint(port: nil)]) == nil)
        #expect(InstanceText.firstPort([endpoint(transport: .unix, host: nil, port: nil)]) == nil)
    }

    @Test func eachEndpointHasItsInspectorLine() {
        #expect(
            InstanceText.endpointLine(endpoint("pg", port: 5432)) == "pg  127.0.0.1:5432")
        #expect(
            InstanceText.endpointLine(endpoint("pg", host: "::1", port: 5432)) == "pg  [::1]:5432")
        #expect(
            InstanceText.endpointLine(endpoint("pg", port: nil))
                == "pg  127.0.0.1, port assigned at start")
        #expect(
            InstanceText.endpointLine(
                endpoint("sock", transport: .unix, host: nil, port: nil, socketPath: "/tmp/s"))
                == "sock  /tmp/s")
        #expect(
            InstanceText.endpointLine(endpoint("sock", transport: .unix, host: nil, port: nil))
                == "sock  socket, created at start")
    }

    @Test func uptimeKeepsTheTwoLargestUnitsWithAPaddedSecondOne() {
        let now = Date(timeIntervalSince1970: 1_000_000)
        func text(_ seconds: Double) -> String {
            UptimeText.text(since: now.addingTimeInterval(-seconds), now: now)
        }
        #expect(text(0) == "0s")
        #expect(text(12) == "12s")
        #expect(text(184) == "3m04s")
        #expect(text(3_720) == "1h02m")
        #expect(text(2 * 86_400 + 5 * 3_600) == "2d05h")
        #expect(text(-30) == "0s")
    }

    @Test func ageUsesTheLargestWholeUnit() {
        let now = Date(timeIntervalSince1970: 1_000_000)
        func text(_ seconds: Double) -> String {
            RelativeAge.text(of: now.addingTimeInterval(-seconds), now: now)
        }
        #expect(text(0) == "now")
        #expect(text(-5) == "now")
        #expect(text(1) == "1s ago")
        #expect(text(59) == "59s ago")
        #expect(text(60) == "1m ago")
        #expect(text(3_600) == "1h ago")
        #expect(text(86_400 * 3) == "3d ago")
    }
}
