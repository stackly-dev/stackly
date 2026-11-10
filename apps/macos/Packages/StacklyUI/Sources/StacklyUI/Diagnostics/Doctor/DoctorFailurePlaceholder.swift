import SwiftUI

extension PlaceholderContent {
    /// A failed doctor run with no report yet (spec 0034, *State seksi Doctor*): the critical
    /// symbol, "Couldn’t Run the Doctor", the daemon's message and code, and Retry. It has no
    /// Open Diagnostics, since the page is Diagnostics.
    init(doctorFailure failure: FailureContent, retry: StateAction?) {
        symbol = Tone.critical.symbol
        symbolStyle = .critical
        title = failure.title
        message = failure.message
        code = failure.code
        primaryAction = retry
        secondaryAction = nil
    }
}
