import AppKit
import Foundation
import Testing

@testable import StacklyUI

/// The window shell: sidebar structure, badges, unbuilt pages, sizes, the close notice,
/// and the window opener (spec 0013 AC-13, AC-14, AC-15).
struct ShellStructureTests {
    // MARK: - Sidebar

    @Test func sidebarGroupsTheSectionsInMockupOrder() {
        #expect(SidebarGroup.main.title == nil)
        #expect(SidebarGroup.main.sections == [.overview, .instances, .environments])
        #expect(SidebarGroup.library.title == "Library")
        #expect(SidebarGroup.library.sections == [.catalog, .runtimes])
        #expect(SidebarGroup.system.title == "System")
        #expect(SidebarGroup.system.sections == [.operations, .diagnostics])
        #expect(
            SidebarSection.allCases.map(\.title) == [
                "Overview", "Instances", "Environments", "Catalog", "Runtimes", "Operations",
                "Diagnostics",
            ]
        )
    }

    @Test func everySectionUsesItsOwnSectionStyle() {
        let styles = SidebarSection.allCases.map(\.style)
        #expect(styles == SectionStyle.allCases)
    }

    @Test func sceneStorageKeepsTheRawValuesAndDefaultsToOverview() {
        #expect(
            SidebarSection.allCases.map(\.rawValue) == [
                "overview", "instances", "environments", "catalog", "runtimes", "operations",
                "diagnostics",
            ])
        #expect(SidebarSection(rawValue: "unknown") == nil)
    }

    @Test func badgesShowNothingWithoutASnapshotOrAtZero() {
        let none = SidebarCounts()
        let zero = SidebarCounts(instances: 0, runningOperations: 0)
        for section in SidebarSection.allCases {
            #expect(section.badge(counts: none, isConnected: true) == nil)
            #expect(section.badge(counts: zero, isConnected: true) == nil)
        }
    }

    @Test func instancesBadgeStaysOfflineAsLastKnown() throws {
        let counts = SidebarCounts(instances: 3)
        let online = try #require(SidebarSection.instances.badge(counts: counts, isConnected: true))
        let offline = try #require(
            SidebarSection.instances.badge(counts: counts, isConnected: false)
        )
        let one = SidebarSection.instances.badge(
            counts: SidebarCounts(instances: 1),
            isConnected: true
        )

        #expect(online.count == 3)
        #expect(online.spokenValue == "3 instances")
        #expect(online.isProminent == false)
        #expect(offline.count == 3)
        #expect(offline.spokenValue == "3 instances, last known")
        #expect(one?.spokenValue == "1 instance")
    }

    @Test func operationsBadgeShowsOnlyWhileConnected() throws {
        let counts = SidebarCounts(runningOperations: 2)
        let online = try #require(
            SidebarSection.operations.badge(counts: counts, isConnected: true)
        )

        #expect(online.count == 2)
        #expect(online.isProminent)
        #expect(online.spokenValue == "2 running")
        #expect(SidebarSection.operations.badge(counts: counts, isConnected: false) == nil)
    }

    @Test func otherSectionsHaveNoBadge() {
        let counts = SidebarCounts(instances: 4, runningOperations: 1)
        for section in [SidebarSection.overview, .environments, .catalog, .runtimes, .diagnostics] {
            #expect(section.badge(counts: counts, isConnected: true) == nil)
        }
    }

    @Test func unbuiltPagesSayTheyComeInALaterBuild() {
        #expect(UnbuiltPage.message == "Coming in a later build.")
    }

    // MARK: - Window

    @Test func windowSizesKeepTheContentAtLeast640PointsWide() {
        #expect(AppShell.minimumSize == CGSize(width: 900, height: 600))
        #expect(AppShell.sidebarWidth.min == 200)
        #expect(AppShell.sidebarWidth.ideal == 236)
        #expect(AppShell.sidebarWidth.max == 260)
        #expect(AppShell.contentMinWidth == 640)
        #expect(AppShell.minimumSize.width - AppShell.sidebarWidth.max == AppShell.contentMinWidth)
        #expect(AppShell.sidebarWidth.ideal + AppShell.contentIdealWidth == 1280)
    }

    @Test func windowOpenerDoesNothingBeforeTheShellAppears() {
        let opener = WindowOpener()
        #expect(opener.canOpen == false)
        #expect(opener.openMain() == false)
        #expect(WindowOpener.mainWindowID == "main")
    }

    // MARK: - Close notice

    @Test func closeNoticeShowsOncePerLaunch() {
        let notice = CloseNotice(presentsPanel: false)
        #expect(notice.mainWindowWillClose(isTerminating: false, screen: nil))
        #expect(notice.hasShown)
        #expect(notice.mainWindowWillClose(isTerminating: false, screen: nil) == false)
    }

    @Test func closeNoticeNeverShowsWhileQuitting() {
        let notice = CloseNotice(presentsPanel: false)
        #expect(notice.mainWindowWillClose(isTerminating: true, screen: nil) == false)
        #expect(notice.hasShown == false)
    }

    @Test func closeNoticeCopyAndDuration() {
        #expect(CloseNotice.title == "Stackly is still running")
        #expect(
            CloseNotice.message
                == "Closing the window doesn’t stop stacklyd or your instances. Click Stackly in the Dock to reopen it."
        )
        #expect(CloseNotice.displayDuration == .seconds(4))
    }
}
