import Foundation

/// The app's name and identifiers: the ONE place to change them. `scripts/make-app.sh` reads the
/// three `static let` lines below (keep each on one line, double-quoted) for Info.plist, the
/// bundle and executable names. The Application Support folder is named after `name` too.
///
/// The project is called SMCraft (owner's choice, DECISIONS.md "Name"). The old working name
/// "MMK" lives on only as the internal prefix of crates, the CLI and environment variables.
enum AppIdentity {
    static let name = "SMCraft"
    static let bundleID = "dev.smcraft.app"
    static let version = "0.1.0"
}
