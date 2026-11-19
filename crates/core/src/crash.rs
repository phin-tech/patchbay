//! What a crash report says and what it's called, so the panic hook in the app only has to write it down.

use jiff::{Timestamp, Zoned, tz::TimeZone};

use crate::version;

/// The report of a panic: when, what went wrong, where, and the backtrace, under the version that crashed.
pub fn report(at: &Zoned, message: &str, location: Option<&str>, backtrace: &str) -> String {
    let place = location
        .map(|place| format!(" at {place}"))
        .unwrap_or_default();
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    let when = at.strftime("%Y-%m-%d %H:%M:%S %Z");
    format!(
        "Version {} on {os} {arch}, {when}\n\nPanicked{place}:\n{message}\n\n{backtrace}\n",
        version()
    )
}

/// A report's file name, by the time in UTC, so sorting the names sorts the reports: `crash-20261104-151207.log`.
pub fn file_name(at: Timestamp) -> String {
    format!(
        "crash-{}.log",
        at.to_zoned(TimeZone::UTC).strftime("%Y%m%d-%H%M%S")
    )
}

/// Whether a file name is a report's.
pub fn is_report(name: &str) -> bool {
    name.starts_with("crash-") && name.ends_with(".log")
}

/// The message a panic carried, which is a `&str` or a `String` when it came from `panic!` or `expect`.
pub fn message(payload: &(dyn std::any::Any + Send)) -> &str {
    payload
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("(no message)")
}

#[cfg(test)]
mod tests {
    use super::{file_name, is_report, message, report};
    use jiff::{Timestamp, tz::TimeZone};

    #[test]
    fn a_report_says_when_what_where_and_which_version() {
        let at: Timestamp = "2026-11-04T15:12:07Z".parse().unwrap();
        let at = at.to_zoned(TimeZone::get("America/New_York").unwrap());
        let text = report(
            &at,
            "index out of range",
            Some("src/view.rs:12:5"),
            "0: main",
        );
        // The version moves with every release, and the machine with whoever runs the tests.
        insta::with_settings!({ filters => vec![
            (r"Version \S+ \(\d+\) on \w+ \w+", "Version [version] on [machine]"),
        ] }, {
            insta::assert_snapshot!(text, @r"
            Version [version] on [machine], 2026-11-04 10:12:07 EST

            Panicked at src/view.rs:12:5:
            index out of range

            0: main
            ");
        });
    }

    #[test]
    fn report_names_sort_by_time() {
        let earlier = file_name("2026-11-04T09:59:59Z".parse().unwrap());
        let later = file_name("2026-11-04T10:00:00Z".parse().unwrap());
        assert_eq!(earlier, "crash-20261104-095959.log");
        assert!(earlier < later);
        assert!(is_report(&later));
        assert!(!is_report("sample.2026-11-04.log"));
    }

    #[test]
    fn a_panics_message_is_read_whichever_kind_it_is() {
        let fixed: Box<dyn std::any::Any + Send> = Box::new("fixed");
        let formatted: Box<dyn std::any::Any + Send> = Box::new(String::from("formatted 3"));
        let other: Box<dyn std::any::Any + Send> = Box::new(7_u8);
        assert_eq!(message(&*fixed), "fixed");
        assert_eq!(message(&*formatted), "formatted 3");
        assert_eq!(message(&*other), "(no message)");
    }
}
