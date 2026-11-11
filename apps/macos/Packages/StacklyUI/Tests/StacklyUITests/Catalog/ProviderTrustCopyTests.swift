import Foundation
import StacklyClient
import Testing

@testable import StacklyUI

/// The words of the local provider screens (spec 0037, *Copy*, *Tone dan label*, AC-11,
/// AC-16): the approval statement against the shared fixture, the phrases the copy must
/// never say, the labels, and the approval banner of every state.
struct ProviderTrustCopyTests {
    /// The phrases no copy of this feature may say (AC-11): no sandbox, no safe or
    /// verified program or publisher, and no revocation list that is current.
    static let forbidden = [
        "sandboxed", "safe to run", "verified publisher", "trusted publisher", "malware",
        "up to date", "latest",
    ]

    private static let style = Date.FormatStyle(
        date: .abbreviated, time: .omitted, locale: Locale(identifier: "en_US"),
        calendar: Calendar(identifier: .gregorian), timeZone: TimeZone(identifier: "UTC")!)

    @Test(
        .evidence(
            covers: ["REQ-032", "BR-020", "AC-061"],
            expected:
                "The approve sheet says the approval statement of the shared fixture word for word"
        ))
    func theApprovalStatementIsTheFixtureWordForWord() throws {
        #expect(ProviderTrustCopy.approvalStatement == (try ProviderFixture.approvalStatement()))
    }

    @Test(
        .evidence(
            covers: ["AC-061", "BR-020"],
            expected:
                "No copy of the local provider screens claims a sandbox, safety, a verified publisher, or a current revocation list"
        ))
    func noCopyOfThisFeatureSaysAForbiddenPhrase() throws {
        var texts = ProviderTrustCopy.allSamples
        texts += ProviderTrustCopy.permissionChanges.map(ProviderTrustCopy.changeLabel)
        texts.append(CatalogCopy.trustText(.local))
        texts.append(CatalogCopy.trustTitle(.local))
        texts.append(CatalogCopy.reasonText(.approvalRequired))
        texts.append(CatalogCopy.reasonText(.trustRevoked))
        texts.append(OnboardingCopy.catalogSubtitle)
        texts.append(OnboardingCopy.catalogImportNote)
        for name in ["pending", "approved", "revoked"] {
            let detail = try ProviderFixture.detail(name)
            if let banner = ProviderTrustCopy.banner(
                detail: detail, catalog: nil, style: Self.style)
            {
                texts += [banner.title, banner.message]
            }
        }
        for detail in [
            try ProviderFixture.changedDetail(), try ProviderFixture.catalogRevokedDetail(),
        ] {
            if let banner = ProviderTrustCopy.banner(
                detail: detail, catalog: nil, style: Self.style)
            {
                texts += [banner.title, banner.message]
            }
        }
        for text in texts {
            for phrase in Self.forbidden {
                #expect(
                    text.localizedCaseInsensitiveContains(phrase) == false, "“\(phrase)” in \(text)"
                )
            }
        }
    }

    @Test func theLabelsAreTheTableOfTheSpec() {
        #expect(CatalogCopy.trustText(.local) == "Local, not verified")
        #expect(CatalogCopy.reasonText(.approvalRequired) == "Needs your approval.")
        #expect(CatalogCopy.reasonText(.trustRevoked) == "Approval revoked.")
        #expect(ProviderTrustCopy.approvalLabel(.pending) == "Needs Approval")
        #expect(ProviderTrustCopy.approvalLabel(.approved) == "Approved")
        #expect(ProviderTrustCopy.approvalLabel(.revoked) == "Revoked")
        #expect(ProviderTrustCopy.approvalLabel(.unrecognized) == "Unrecognized State")
        #expect(
            ProviderTrustCopy.executableUses.map(ProviderTrustCopy.useLabel) == [
                "starts the server", "runs during setup", "runs on stop", "runs on reload",
                "runs health checks", "runs once at install", "opens in Terminal", "Unknown",
            ])
        #expect(
            ProviderTrustCopy.permissionChanges.map(ProviderTrustCopy.changeLabel) == [
                "revision", "programs it runs", "files it downloads", "download hosts",
                "features", "folders it writes", "secrets it receives", "endpoints",
                "apps it opens", "configuration content", "Unknown",
            ])
    }

    @Test func theFixedSentencesAreTheTableOfTheSpec() {
        #expect(
            ProviderTrustCopy.identityLine(provider: "acme/demo", providerRevision: "1.0.0")
                == "acme/demo · revision 1.0.0 · from acme (not verified)")
        #expect(
            ProviderTrustCopy.approveFootnote
                == "This approval covers exactly this version of the provider. Any change needs a new approval."
        )
        #expect(
            ProviderTrustCopy.importFootnote
                == "Importing doesn’t run anything. The provider stays blocked until you approve it."
        )
        #expect(
            ProviderTrustCopy.importChanges([.revision, .downloads])
                == "Differs from the version you decided on: revision, files it downloads.")
        #expect(
            ProviderTrustCopy.replaces(revision: "1.0.0")
                == "Replaces imported revision 1.0.0. The new version needs approval before it can install or run anything."
        )
        #expect(
            ProviderTrustCopy.revokeTitle(displayName: "Acme Demo")
                == "Revoke approval for “Acme Demo”?")
        #expect(
            ProviderTrustCopy.removeTitle(displayName: "Acme Demo")
                == "Remove “Acme Demo” from the catalog?")
        #expect(
            ProviderTrustCopy.imported(displayName: "Acme Demo")
                == "Imported Acme Demo. Approve it before you install anything.")
        #expect(ProviderTrustCopy.publisher(provider: "acme/demo") == "acme (not verified)")
        #expect(ProviderTrustCopy.revokedRevisions(2) == "2 revoked revisions in this catalog")
    }

    @Test func theRevokeAndRemoveConfirmationsFollowTheCopyRules() {
        let rules = [
            (
                ProviderTrustCopy.revokeTitle(displayName: "Acme Demo"),
                ProviderTrustCopy.revokeMessage, ProviderTrustCopy.revokeAction
            ),
            (
                ProviderTrustCopy.removeTitle(displayName: "Acme Demo"),
                ProviderTrustCopy.removeMessage, ProviderTrustCopy.removeAction
            ),
        ]
        for (title, message, action) in rules {
            let request = ConfirmationRequest(
                title: title, message: message, actionTitle: action, role: .destructive,
                perform: {})
            #expect(request.copyProblems.isEmpty, "\(title)")
        }
    }

    // MARK: - Banner

    @Test func aPendingEntryWithoutADecisionAsksForApproval() throws {
        let banner = try #require(
            ProviderTrustCopy.banner(
                detail: try ProviderFixture.detail("pending"), catalog: nil, style: Self.style))
        #expect(banner.title == "Not approved yet")
        #expect(
            banner.message
                == "Stackly won’t install or run anything from this provider until you approve it."
        )
        #expect(banner.action == .approve)
        #expect(banner.tone == .caution)
    }

    @Test func aPendingEntryWithAnEarlierApprovalNamesTheChanges() throws {
        let banner = try #require(
            ProviderTrustCopy.banner(
                detail: try ProviderFixture.changedDetail(), catalog: nil, style: Self.style))
        #expect(banner.title == "Changed since your approval")
        #expect(
            banner.message
                == "This version differs from revision 0.9.0 that you decided on Nov 1, 2026: revision, files it downloads, download hosts."
        )
        #expect(banner.action == .approve)
    }

    @Test func aRevokedEntryOffersToApproveAgain() throws {
        let banner = try #require(
            ProviderTrustCopy.banner(
                detail: try ProviderFixture.detail("revoked"), catalog: nil, style: Self.style))
        #expect(banner.title == "Approval revoked")
        #expect(banner.message.hasPrefix("Revoked on Nov 11, 2026. "))
        #expect(banner.message.hasSuffix("can still start."))
        #expect(banner.action == .approveAgain)
    }

    @Test func anEntryTheCatalogRevokesSaysTheCatalogRevisionAndDateAndOffersNothing() throws {
        let banner = try #require(
            ProviderTrustCopy.banner(
                detail: try ProviderFixture.catalogRevokedDetail(), catalog: CatalogInfo.fixture,
                style: Self.style))
        #expect(banner.title == "Revoked by the catalog")
        #expect(banner.message.contains("(revision 7, published "))
        #expect(
            banner.message.contains(
                "revokes this revision since Nov 5, 2026: Starts with an insecure default; use revision 1.0.1 or later."
            ))
        #expect(banner.action == nil)
    }

    @Test func anApprovedEntryHasNoBanner() throws {
        #expect(
            ProviderTrustCopy.banner(
                detail: try ProviderFixture.detail("approved"), catalog: nil, style: Self.style)
                == nil)
    }

    // MARK: - Appearance

    @Test func approvedIsNeutralNeverPositive() {
        #expect(CatalogAppearance.appearance(of: ApprovalState.approved).tone == .neutral)
        #expect(CatalogAppearance.appearance(of: ApprovalState.pending).tone == .caution)
        #expect(CatalogAppearance.appearance(of: ApprovalState.revoked).tone == .caution)
        #expect(
            CatalogAppearance.appearance(of: ApprovalState.unrecognized).label
                == "Unrecognized State")
    }

    @Test func theBadgeOfALocalEntryFollowsItsApproval() throws {
        let pending = try ProviderFixture.detail("pending").summary
        let approved = try ProviderFixture.detail("approved").summary
        let revoked = try ProviderFixture.detail("revoked").summary
        let catalogRevoked = try ProviderFixture.catalogRevokedDetail().summary
        #expect(CatalogAppearance.badge(of: pending).label == "Needs Approval")
        #expect(CatalogAppearance.badge(of: approved).label == "Available")
        #expect(CatalogAppearance.badge(of: revoked).label == "Revoked")
        #expect(CatalogAppearance.badge(of: catalogRevoked).label == "Revoked")
        #expect(CatalogCopy.metaLine(of: pending) == "Database · Local")
    }
}
