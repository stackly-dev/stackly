/// The accessibility identifiers of the shared components (spec 0067, *Accessibility
/// identifier*), in the form `<scope>.<part>[.<key>]` that feature 68 agreed. The key is
/// only a stable data key, never display text, and an identifier never changes a
/// VoiceOver label or order.
///
/// Feature 68 adds the scopes of the screens next to these: the sidebar, the daemon
/// footer and banner, and the design gallery, which its UI tests read (spec 0068).
nonisolated public enum AccessibilityID {
    /// The main window's sidebar.
    public enum Sidebar {
        /// The row of `section`, such as `sidebar.row.instances`. The key is the raw
        /// value of `SidebarSection`.
        public static func row(_ section: SidebarSection) -> String {
            "sidebar.row.\(section.rawValue)"
        }
    }

    /// The daemon footer in the sidebar.
    public enum DaemonFooter {
        /// The status label, read as "Status: <text>".
        public static let status = "daemonFooter.status"
    }

    /// The daemon banner above the pages. Its label is the banner title and its buttons
    /// are found by their titles, so only the container has an identifier.
    public enum DaemonBanner {
        /// The banner.
        public static let container = "daemonBanner.container"
    }

    /// The design gallery window.
    public enum Gallery {
        /// The sidebar row of the topic with the lowercase name `topic`, such as
        /// `gallery.topic.status`.
        public static func topic(_ topic: String) -> String {
            "gallery.topic.\(topic)"
        }
    }

    /// The feedback card.
    public enum Feedback {
        /// The card.
        public static let card = "feedback.card"
        /// The card's action button.
        public static let action = "feedback.action"
        /// The Dismiss button.
        public static let dismiss = "feedback.dismiss"
    }

    /// The sheet with a footer.
    public enum Sheet {
        /// The Cancel button.
        public static let cancel = "sheet.cancel"
        /// The primary action.
        public static let primary = "sheet.primary"
    }

    /// The inspector.
    public enum Inspector {
        /// The toolbar toggle.
        public static let toggle = "inspector.toggle"
        /// The placeholder.
        public static let placeholder = "inspector.placeholder"
    }

    /// Actions and menus.
    public enum Action {
        /// The More menu.
        public static let more = "action.more"

        /// The action button with action ID `id`.
        public static func button(_ id: String) -> String {
            "action.button.\(id)"
        }
    }

    /// The checklist.
    public enum Checklist {
        /// The row with check ID `id`.
        public static func row(_ id: String) -> String {
            "checklist.row.\(id)"
        }
    }

    /// The staged progress.
    public enum Progress {
        /// The progress view.
        public static let view = "progress.view"
    }

    /// The log view.
    public enum Log {
        /// The line list.
        public static let list = "log.list"
    }

    /// The diff view.
    public enum Diff {
        /// The line list.
        public static let list = "diff.list"
    }

    /// The copy button.
    public enum Copy {
        /// The copy button with the screen's key `key`. Set only when the screen gives
        /// a key.
        public static func button(_ key: String) -> String {
            "copy.button.\(key)"
        }
    }
}
