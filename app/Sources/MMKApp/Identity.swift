import Foundation

/// The app's name and identifiers: the ONE place to change them. `scripts/make-app.sh` reads the
/// three `static let` lines below (keep each on one line, double-quoted) for Info.plist, the
/// bundle and executable names. The Application Support folder is named after `name` too.
///
/// "MMK" is a neutral working name until the owner picks one (DECISIONS.md, "App shell").
enum AppIdentity {
    static let name = "MMK"
    static let bundleID = "dev.mmk.engine"
    static let version = "0.1.0"
}
