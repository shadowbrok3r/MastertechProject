//! Injection state and command dispatch. The USB HID report backend lands in step 2;
//! injection commands report that until then, while arming and release work now.

use crate::protocol::{Envelope, Request, Response};
use serde_json::json;

/// Tracks whether injection is armed. Boots disarmed.
pub struct Injector {
    armed: bool,
}

impl Default for Injector {
    fn default() -> Self {
        Self { armed: false }
    }
}

impl Injector {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn armed(&self) -> bool {
        self.armed
    }

    /// Arms or disarms injection; disarming drops any held reports.
    pub fn set_armed(&mut self, armed: bool) {
        self.armed = armed;
        if !armed {
            self.release_all();
        }
    }

    /// Drops all held keys and mouse buttons. Safe while disarmed.
    pub fn release_all(&mut self) {}

    fn inject_guarded(&self) -> anyhow::Result<()> {
        if !self.armed {
            anyhow::bail!("injection disarmed; send {{\"cmd\":\"arm\"}} first");
        }
        anyhow::bail!("usb hid backend not built yet (project 1, step 2)");
    }

    fn status(&self) -> serde_json::Value {
        json!({ "armed": self.armed, "hid": "stub", "firmware": env!("CARGO_PKG_VERSION") })
    }
}

/// Runs one command against the injector and builds its reply.
pub fn dispatch(env: Envelope, inj: &mut Injector) -> Response {
    let id = env.id;
    match env.request {
        Request::Ping => Response::ok(id, json!({ "pong": true, "armed": inj.armed() })),
        Request::Status => Response::ok(id, inj.status()),
        Request::Arm => {
            inj.set_armed(true);
            Response::ok(id, json!({ "armed": true }))
        }
        Request::Disarm => {
            inj.set_armed(false);
            Response::ok(id, json!({ "armed": false }))
        }
        Request::ReleaseAll => {
            inj.release_all();
            Response::ok(id, json!({ "released": true }))
        }
        Request::Type { .. }
        | Request::Key { .. }
        | Request::MouseMove { .. }
        | Request::Click { .. }
        | Request::Combo { .. } => match inj.inject_guarded() {
            Ok(()) => Response::ok(id, json!({ "done": true })),
            Err(e) => Response::err(id, e.to_string()),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(line: &str) -> Envelope {
        serde_json::from_str(line).expect("parses")
    }

    #[test]
    fn boots_disarmed() {
        assert!(!Injector::new().armed());
    }

    #[test]
    fn ping_reports_armed_state() {
        let mut inj = Injector::new();
        let r = dispatch(env(r#"{"id":"1","cmd":"ping"}"#), &mut inj);
        assert_eq!(r.result.unwrap()["armed"], false);
        dispatch(env(r#"{"cmd":"arm"}"#), &mut inj);
        let r = dispatch(env(r#"{"cmd":"ping"}"#), &mut inj);
        assert_eq!(r.result.unwrap()["armed"], true);
    }

    #[test]
    fn injection_blocked_until_armed() {
        let mut inj = Injector::new();
        let r = dispatch(env(r#"{"cmd":"type","text":"hi"}"#), &mut inj);
        assert!(!r.ok);
        assert!(r.error.unwrap().contains("disarmed"));
    }

    #[test]
    fn armed_injection_reports_backend_pending() {
        let mut inj = Injector::new();
        dispatch(env(r#"{"cmd":"arm"}"#), &mut inj);
        let r = dispatch(env(r#"{"cmd":"key","chord":"F2"}"#), &mut inj);
        assert!(!r.ok);
        assert!(r.error.unwrap().contains("not built yet"));
    }

    #[test]
    fn disarm_takes_effect() {
        let mut inj = Injector::new();
        dispatch(env(r#"{"cmd":"arm"}"#), &mut inj);
        assert!(inj.armed());
        dispatch(env(r#"{"cmd":"disarm"}"#), &mut inj);
        assert!(!inj.armed());
    }
}
