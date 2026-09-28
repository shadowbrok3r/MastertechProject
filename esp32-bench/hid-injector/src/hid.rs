//! Injection state and command dispatch, routing to a [`Hid`] backend.

use crate::protocol::{Button, Envelope, Request, Response, Step};
use serde_json::json;

/// Emits USB HID reports. The device backend is TinyUSB; tests use a mock.
pub trait Hid {
    fn type_text(&mut self, text: &str) -> anyhow::Result<()>;
    fn key(&mut self, chord: &str) -> anyhow::Result<()>;
    fn mouse_move(&mut self, x: i32, y: i32) -> anyhow::Result<()>;
    fn click(&mut self, button: Button) -> anyhow::Result<()>;
    fn release_all(&mut self);
    fn delay(&mut self, ms: u32);
    fn ready(&self) -> bool;
}

/// Arms injection and forwards commands to the backend. Boots disarmed.
pub struct Injector<H: Hid> {
    armed: bool,
    hid: H,
}

impl<H: Hid> Injector<H> {
    pub fn new(hid: H) -> Self {
        Self { armed: false, hid }
    }

    #[allow(dead_code)]
    pub fn armed(&self) -> bool {
        self.armed
    }

    /// Arms or disarms injection; disarming drops any held reports.
    pub fn set_armed(&mut self, armed: bool) {
        self.armed = armed;
        if !armed {
            self.hid.release_all();
        }
    }

    fn guard(&self) -> anyhow::Result<()> {
        if !self.armed {
            anyhow::bail!("injection disarmed; send {{\"cmd\":\"arm\"}} first");
        }
        if !self.hid.ready() {
            anyhow::bail!("usb host not connected");
        }
        Ok(())
    }

    fn status(&self) -> serde_json::Value {
        json!({
            "armed": self.armed,
            "usb": if self.hid.ready() { "ready" } else { "not_connected" },
            "firmware": env!("CARGO_PKG_VERSION"),
        })
    }

    fn run_step(&mut self, step: Step) -> anyhow::Result<()> {
        match step {
            Step::Type { text } => self.hid.type_text(&text),
            Step::Key { chord } => self.hid.key(&chord),
            Step::MouseMove { x, y } => self.hid.mouse_move(x, y),
            Step::Click { button } => self.hid.click(button),
            Step::Delay { ms } => {
                self.hid.delay(ms);
                Ok(())
            }
            Step::ReleaseAll => {
                self.hid.release_all();
                Ok(())
            }
        }
    }

    /// Runs one command and builds its reply.
    pub fn dispatch(&mut self, env: Envelope) -> Response {
        let id = env.id;
        match env.request {
            Request::Ping => Response::ok(id, json!({ "pong": true, "armed": self.armed })),
            Request::Status => Response::ok(id, self.status()),
            Request::Arm => {
                self.set_armed(true);
                Response::ok(id, json!({ "armed": true }))
            }
            Request::Disarm => {
                self.set_armed(false);
                Response::ok(id, json!({ "armed": false }))
            }
            Request::ReleaseAll => {
                self.hid.release_all();
                Response::ok(id, json!({ "released": true }))
            }
            Request::Type { text } => self.gated(id, |s| s.hid.type_text(&text)),
            Request::Key { chord } => self.gated(id, |s| s.hid.key(&chord)),
            Request::MouseMove { x, y } => self.gated(id, |s| s.hid.mouse_move(x, y)),
            Request::Click { button } => self.gated(id, |s| s.hid.click(button)),
            Request::Combo { steps } => self.gated(id, |s| {
                for step in steps {
                    s.run_step(step)?;
                }
                Ok(())
            }),
        }
    }

    fn gated<F>(&mut self, id: Option<String>, f: F) -> Response
    where
        F: FnOnce(&mut Self) -> anyhow::Result<()>,
    {
        if let Err(e) = self.guard() {
            return Response::err(id, e.to_string());
        }
        match f(self) {
            Ok(()) => Response::ok(id, json!({ "done": true })),
            Err(e) => Response::err(id, e.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct MockHid {
        ready: bool,
        log: Vec<String>,
    }

    impl Hid for MockHid {
        fn type_text(&mut self, text: &str) -> anyhow::Result<()> {
            self.log.push(format!("type:{text}"));
            Ok(())
        }
        fn key(&mut self, chord: &str) -> anyhow::Result<()> {
            self.log.push(format!("key:{chord}"));
            Ok(())
        }
        fn mouse_move(&mut self, x: i32, y: i32) -> anyhow::Result<()> {
            self.log.push(format!("move:{x},{y}"));
            Ok(())
        }
        fn click(&mut self, button: Button) -> anyhow::Result<()> {
            self.log.push(format!("click:{button:?}"));
            Ok(())
        }
        fn release_all(&mut self) {
            self.log.push("release".into());
        }
        fn delay(&mut self, ms: u32) {
            self.log.push(format!("delay:{ms}"));
        }
        fn ready(&self) -> bool {
            self.ready
        }
    }

    fn env(line: &str) -> Envelope {
        serde_json::from_str(line).expect("parses")
    }

    fn armed_ready() -> Injector<MockHid> {
        let mut inj = Injector::new(MockHid { ready: true, log: Vec::new() });
        inj.set_armed(true);
        inj
    }

    #[test]
    fn boots_disarmed() {
        assert!(!Injector::new(MockHid::default()).armed());
    }

    #[test]
    fn injection_blocked_until_armed() {
        let mut inj = Injector::new(MockHid { ready: true, log: Vec::new() });
        let r = inj.dispatch(env(r#"{"cmd":"type","text":"hi"}"#));
        assert!(!r.ok);
        assert!(r.error.unwrap().contains("disarmed"));
    }

    #[test]
    fn armed_but_no_usb_host_is_refused() {
        let mut inj = Injector::new(MockHid { ready: false, log: Vec::new() });
        inj.set_armed(true);
        let r = inj.dispatch(env(r#"{"cmd":"type","text":"hi"}"#));
        assert!(!r.ok);
        assert!(r.error.unwrap().contains("usb host"));
    }

    #[test]
    fn armed_type_and_key_reach_the_backend() {
        let mut inj = armed_ready();
        assert!(inj.dispatch(env(r#"{"cmd":"type","text":"hello"}"#)).ok);
        assert!(inj.dispatch(env(r#"{"cmd":"key","chord":"ctrl+alt+del"}"#)).ok);
        assert_eq!(inj.hid.log, vec!["type:hello", "key:ctrl+alt+del"]);
    }

    #[test]
    fn combo_runs_steps_in_order() {
        let mut inj = armed_ready();
        let r = inj.dispatch(env(
            r#"{"cmd":"combo","steps":[{"op":"key","chord":"F2"},{"op":"delay","ms":0},{"op":"type","text":"BIOS"},{"op":"release_all"}]}"#,
        ));
        assert!(r.ok);
        assert_eq!(inj.hid.log, vec!["key:F2", "delay:0", "type:BIOS", "release"]);
    }

    #[test]
    fn disarm_releases_and_blocks() {
        let mut inj = armed_ready();
        inj.dispatch(env(r#"{"cmd":"disarm"}"#));
        assert!(!inj.armed());
        assert_eq!(inj.hid.log, vec!["release"]);
        assert!(!inj.dispatch(env(r#"{"cmd":"click"}"#)).ok);
    }

    #[test]
    fn release_all_runs_while_disarmed() {
        let mut inj = Injector::new(MockHid { ready: true, log: Vec::new() });
        assert!(inj.dispatch(env(r#"{"cmd":"release_all"}"#)).ok);
        assert_eq!(inj.hid.log, vec!["release"]);
    }

    #[test]
    fn status_and_ping_report_state() {
        let inj = armed_ready();
        let mut inj = inj;
        let r = inj.dispatch(env(r#"{"cmd":"status"}"#));
        let v = r.result.unwrap();
        assert_eq!(v["armed"], true);
        assert_eq!(v["usb"], "ready");
    }
}
