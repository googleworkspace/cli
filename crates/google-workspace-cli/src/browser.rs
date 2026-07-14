//! Best-effort opening of a URL in the user's default browser.
//!
//! Used by `gws auth login` to spare the user from copy-pasting the OAuth
//! URL. Every failure path is silent by design: callers always print the
//! URL first, so the previous copy-paste behavior remains the fallback.

use std::process::{Command, Stdio};

/// Returns the platform-specific program and arguments that open `url` in
/// the default browser, or `None` when the platform has no known opener.
fn opener_for_os(os: &str, url: &str) -> Option<(&'static str, Vec<String>)> {
    match os {
        "macos" => Some(("open", vec![url.to_string()])),
        "linux" => Some(("xdg-open", vec![url.to_string()])),
        // `start` is a cmd.exe built-in that re-parses `&` inside URLs, so
        // use rundll32 which receives the URL as a plain argument instead.
        "windows" => Some((
            "rundll32",
            vec!["url.dll,FileProtocolHandler".to_string(), url.to_string()],
        )),
        _ => None,
    }
}

/// Validates that `url` is a plain https URL that is safe to hand to an
/// external opener program. Rejects control characters, whitespace, and
/// dangerous Unicode (zero-width chars, bidi overrides) that
/// `char::is_control()` does not cover (`Cf` category).
fn is_openable_url(url: &str) -> bool {
    url.starts_with("https://")
        && !url.chars().any(|c| {
            c.is_control()
                || c.is_whitespace()
                || google_workspace::validate::is_dangerous_unicode(c)
        })
}

/// Spawns `program` detached from this process. Returns `true` when the
/// process was spawned successfully (e.g. `false` when the program is not
/// installed). The child is reaped on a background thread so a slow opener
/// never blocks the OAuth callback accept loop.
fn spawn_detached(program: &str, args: &[String]) -> bool {
    match Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(mut child) => {
            std::thread::spawn(move || {
                let _ = child.wait();
            });
            true
        }
        Err(_) => false,
    }
}

/// Attempts to open `url` in the default browser. Returns `true` when an
/// opener process was launched; `false` means the caller's printed URL is
/// the only way for the user to proceed.
pub(crate) fn try_open_browser(url: &str) -> bool {
    if !is_openable_url(url) {
        return false;
    }
    match opener_for_os(std::env::consts::OS, url) {
        Some((program, args)) => spawn_detached(program, &args),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opener_for_macos_uses_open() {
        let (program, args) = opener_for_os("macos", "https://example.com").unwrap();
        assert_eq!(program, "open");
        assert_eq!(args, vec!["https://example.com".to_string()]);
    }

    #[test]
    fn opener_for_linux_uses_xdg_open() {
        let (program, args) = opener_for_os("linux", "https://example.com").unwrap();
        assert_eq!(program, "xdg-open");
        assert_eq!(args, vec!["https://example.com".to_string()]);
    }

    #[test]
    fn opener_for_windows_uses_rundll32() {
        let (program, args) = opener_for_os("windows", "https://example.com").unwrap();
        assert_eq!(program, "rundll32");
        assert_eq!(
            args,
            vec![
                "url.dll,FileProtocolHandler".to_string(),
                "https://example.com".to_string()
            ]
        );
    }

    #[test]
    fn opener_for_unknown_os_is_none() {
        assert!(opener_for_os("freebsd", "https://example.com").is_none());
    }

    #[test]
    fn openable_url_accepts_https() {
        assert!(is_openable_url(
            "https://accounts.google.com/o/oauth2/auth?scope=x&client_id=y"
        ));
    }

    #[test]
    fn openable_url_rejects_non_https_schemes() {
        assert!(!is_openable_url("http://example.com"));
        assert!(!is_openable_url("javascript:alert(1)"));
        assert!(!is_openable_url("file:///etc/passwd"));
    }

    #[test]
    fn openable_url_rejects_whitespace_and_control_chars() {
        assert!(!is_openable_url("https://example.com/a b"));
        assert!(!is_openable_url("https://example.com/\n"));
    }

    #[test]
    fn openable_url_rejects_dangerous_unicode() {
        // RLO (bidi override, category Cf — not caught by is_control)
        assert!(!is_openable_url("https://example.com/\u{202E}evil"));
        // ZWSP (zero-width space)
        assert!(!is_openable_url("https://example.com/a\u{200B}b"));
    }

    #[test]
    fn try_open_browser_rejects_unsafe_url() {
        assert!(!try_open_browser("javascript:alert(1)"));
    }

    #[cfg(unix)]
    #[test]
    fn spawn_detached_returns_true_for_existing_program() {
        assert!(spawn_detached("true", &[]));
    }

    #[test]
    fn spawn_detached_returns_false_for_missing_program() {
        assert!(!spawn_detached(
            "gws-test-nonexistent-program-9f2c",
            &["https://example.com".to_string()]
        ));
    }
}
