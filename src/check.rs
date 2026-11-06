//! `mk check`: ask whether it is safe to act, so unattended input can refuse to
//! run instead of landing in the wrong window. Anything the system cannot report
//! counts as a failed condition: an unknown answer must never mean "go ahead".

use anyhow::Result;
use serde::Serialize;
use std::time::Duration;

#[derive(Debug, Default)]
pub struct CheckRequest {
    /// Minimum time since the user last touched keyboard or mouse.
    pub min_idle: Option<Duration>,
    /// Case-insensitive substring of the active window's title.
    pub active_window: Option<String>,
    /// Case-insensitive substring of the focused control's name.
    pub focused: Option<String>,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct CheckReport {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub idle_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_window: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub focused: Option<String>,
    pub failed: Vec<String>,
}

/// What the system answered. Each field is only read if a condition needs it.
#[derive(Debug, Default)]
struct Observed {
    idle: Option<Result<Duration, String>>,
    active_window: Option<Result<String, String>>,
    focused: Option<Result<Option<String>, String>>,
}

fn contains_ignoring_case(haystack: &str, needle: &str) -> bool {
    haystack.to_lowercase().contains(&needle.to_lowercase())
}

/// Pure: decide from what was observed.
fn evaluate(request: &CheckRequest, observed: Observed) -> CheckReport {
    let mut failed = Vec::new();

    let idle = observed.idle.and_then(|answer| answer.map_err(|e| failed.push(e)).ok());
    if let Some(minimum) = request.min_idle {
        match idle {
            Some(current) if current >= minimum => {}
            Some(current) => failed.push(format!(
                "idle {}ms is below the required {}ms",
                current.as_millis(),
                minimum.as_millis()
            )),
            None if failed.is_empty() => failed.push("idle time unavailable".into()),
            None => {}
        }
    }

    let active = observed.active_window.and_then(|answer| answer.map_err(|e| failed.push(e)).ok());
    if let (Some(wanted), Some(title)) = (&request.active_window, &active)
        && !contains_ignoring_case(title, wanted)
    {
        failed.push(format!("active window is '{title}', wanted '{wanted}'"));
    }

    let focused = observed
        .focused
        .and_then(|answer| answer.map_err(|e| failed.push(e)).ok())
        .flatten();
    if let Some(wanted) = &request.focused {
        match &focused {
            Some(name) if contains_ignoring_case(name, wanted) => {}
            Some(name) => failed.push(format!("focused control is '{name}', wanted '{wanted}'")),
            None => failed.push(format!("no focused control, wanted '{wanted}'")),
        }
    }

    CheckReport {
        ok: failed.is_empty(),
        idle_ms: idle.map(|value| value.as_millis() as u64),
        active_window: active,
        focused,
        failed,
    }
}

#[cfg(target_os = "linux")]
fn read_idle() -> Result<Duration> {
    use anyhow::Context;
    let connection = zbus::blocking::Connection::session().context("no session D-Bus")?;
    let milliseconds: u64 = connection
        .call_method(
            Some("org.gnome.Mutter.IdleMonitor"),
            "/org/gnome/Mutter/IdleMonitor/Core",
            Some("org.gnome.Mutter.IdleMonitor"),
            "GetIdletime",
            &(),
        )
        .context("idle time is only available on GNOME for now")?
        .body()
        .deserialize()
        .context("unexpected idle time reply")?;
    Ok(Duration::from_millis(milliseconds))
}

#[cfg(not(target_os = "linux"))]
fn read_idle() -> Result<Duration> {
    anyhow::bail!("idle time is not available on this OS yet")
}

#[cfg(target_os = "linux")]
fn read_focused() -> Result<Option<String>> {
    crate::accessibility::linux_atspi::focused_control_name()
}

#[cfg(not(target_os = "linux"))]
fn read_focused() -> Result<Option<String>> {
    anyhow::bail!("reading the focused control is not available on this OS yet")
}

pub fn run(request: &CheckRequest) -> CheckReport {
    let describe = |error: anyhow::Error| format!("{error:#}");
    let observed = Observed {
        idle: request.min_idle.map(|_| read_idle().map_err(describe)),
        active_window: request.active_window.as_ref().map(|_| {
            crate::windows::active_window()
                .map(|window| window.title)
                .map_err(describe)
        }),
        focused: request.focused.as_ref().map(|_| read_focused().map_err(describe)),
    };
    evaluate(request, observed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seconds(value: u64) -> Duration {
        Duration::from_secs(value)
    }

    #[test]
    fn passes_when_every_condition_holds() {
        let request = CheckRequest {
            min_idle: Some(seconds(60)),
            active_window: Some("visual studio".into()),
            focused: Some("message".into()),
        };
        let report = evaluate(
            &request,
            Observed {
                idle: Some(Ok(seconds(61))),
                active_window: Some(Ok("Claude - Visual Studio Code".into())),
                focused: Some(Ok(Some("Message input".into()))),
            },
        );
        assert!(report.ok, "{:?}", report.failed);
        assert_eq!(report.idle_ms, Some(61_000));
    }

    #[test]
    fn user_activity_fails_the_idle_condition() {
        let request = CheckRequest { min_idle: Some(seconds(60)), ..Default::default() };
        let report = evaluate(&request, Observed { idle: Some(Ok(seconds(5))), ..Default::default() });
        assert!(!report.ok);
        assert!(report.failed[0].contains("below"));
    }

    #[test]
    fn wrong_window_and_wrong_focus_fail() {
        let request = CheckRequest {
            active_window: Some("code".into()),
            focused: Some("message".into()),
            ..Default::default()
        };
        let report = evaluate(
            &request,
            Observed {
                active_window: Some(Ok("Firefox".into())),
                focused: Some(Ok(Some("Search".into()))),
                ..Default::default()
            },
        );
        assert_eq!(report.failed.len(), 2);
    }

    #[test]
    fn an_unknown_answer_never_means_go_ahead() {
        let request = CheckRequest {
            min_idle: Some(seconds(60)),
            active_window: Some("code".into()),
            focused: Some("message".into()),
        };
        let report = evaluate(
            &request,
            Observed {
                idle: Some(Err("no idle monitor".into())),
                active_window: Some(Err("no active window".into())),
                focused: Some(Ok(None)),
            },
        );
        assert!(!report.ok);
        assert_eq!(report.failed.len(), 3, "{:?}", report.failed);
    }

    #[test]
    fn no_conditions_just_reports_ok() {
        assert!(evaluate(&CheckRequest::default(), Observed::default()).ok);
    }
}
