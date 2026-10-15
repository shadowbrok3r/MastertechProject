//! Build identity stamped as `tool_version` on every stress row this process writes.

use std::sync::OnceLock;

static HOST_BUILD: OnceLock<String> = OnceLock::new();

/// Records the host app and its build, e.g. `("MasterTech", "5.0.1+ec3ea6b.e8a3c1")`, and publishes the process stamp; the first call wins.
pub fn set_host_build(app: &str, build: &str) {
    register(&HOST_BUILD, app, build);
    database::schema::register_process_build(&compose(&host_build(), stress_kit::BUILD_ID));
}

fn register(cell: &OnceLock<String>, app: &str, build: &str) {
    let _ = cell.set(format!("{app} {build}"));
}

/// The registered host build, or [`database::schema::UNREGISTERED_BUILD`] when no host registered one.
pub fn host_build() -> String {
    HOST_BUILD
        .get()
        .cloned()
        .unwrap_or_else(|| database::schema::UNREGISTERED_BUILD.to_owned())
}

/// `tool_version` for rows this process writes: the published `"{host} {build}; stress-kit {build}"`.
pub fn tool_version() -> String {
    database::schema::process_build().to_owned()
}

/// `tool_version` for rows this process records but did not produce: `"{supplied or backfill}; recorded by {host}"`.
pub fn recorded_tool_version(supplied: Option<&str>) -> String {
    recorded(supplied, &host_build())
}

fn compose(host: &str, kit: &str) -> String {
    format!("{host}; stress-kit {kit}")
}

fn recorded(supplied: Option<&str>, recorder: &str) -> String {
    let origin = supplied
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .unwrap_or("backfill");
    format!("{origin}; recorded by {recorder}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn composes_host_and_kit_builds() {
        assert_eq!(
            compose("MasterTech 5.0.1+abc1234.000001", "0.1.0+abc1234.000002"),
            "MasterTech 5.0.1+abc1234.000001; stress-kit 0.1.0+abc1234.000002"
        );
    }

    #[test]
    fn tool_version_is_the_published_process_stamp() {
        assert_eq!(tool_version(), database::schema::process_build());
    }

    #[test]
    fn first_registered_host_wins() {
        let cell = OnceLock::new();
        register(&cell, "first-app", "1.0.0+aaaaaaa");
        register(&cell, "second-app", "2.0.0+bbbbbbb");
        assert_eq!(
            cell.get().map(String::as_str),
            Some("first-app 1.0.0+aaaaaaa")
        );
    }

    #[test]
    fn recorded_rows_name_their_origin_and_recorder() {
        let recorder = "admin-agent 0.0.0+0000000.000000";
        assert_eq!(
            recorded(Some(" OCCT 13.1.4 "), recorder),
            "OCCT 13.1.4; recorded by admin-agent 0.0.0+0000000.000000"
        );
        assert_eq!(
            recorded(Some("  "), recorder),
            "backfill; recorded by admin-agent 0.0.0+0000000.000000"
        );
        assert_eq!(recorded(None, recorder), recorded(Some(""), recorder));
        assert_eq!(
            recorded_tool_version(None),
            format!("backfill; recorded by {}", host_build())
        );
    }
}
