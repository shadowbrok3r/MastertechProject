//! Named payloads: ordered macro scripts staged on the device and run by name.

use crate::protocol::Step;

const MAX_PAYLOADS: usize = 16;
const MAX_STEPS: usize = 512;
const MAX_NAME_LEN: usize = 64;

/// One stored payload: a name and its ordered steps.
#[derive(Debug, Clone, PartialEq)]
pub struct Payload {
    pub name: String,
    pub steps: Vec<Step>,
}

/// Name and step count of one stored payload.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct PayloadMeta {
    pub name: String,
    pub steps: usize,
}

/// In-RAM payload store, bounded in count, steps per payload, and name length.
#[derive(Default)]
pub struct PayloadStore {
    payloads: Vec<Payload>,
}

impl PayloadStore {
    pub fn new() -> Self {
        Self { payloads: Vec::new() }
    }

    fn position(&self, name: &str) -> Option<usize> {
        self.payloads.iter().position(|p| p.name == name)
    }

    /// Stores a payload, replacing any with the same name. Errors on bounds.
    pub fn put(&mut self, name: String, steps: Vec<Step>) -> Result<(), String> {
        if name.is_empty() || name.len() > MAX_NAME_LEN {
            return Err(format!("payload name must be 1..={MAX_NAME_LEN} chars"));
        }
        if steps.len() > MAX_STEPS {
            return Err(format!("payload has {} steps; max {MAX_STEPS}", steps.len()));
        }
        match self.position(&name) {
            Some(i) => self.payloads[i].steps = steps,
            None => {
                if self.payloads.len() >= MAX_PAYLOADS {
                    return Err(format!("payload store full ({MAX_PAYLOADS}); delete one first"));
                }
                self.payloads.push(Payload { name, steps });
            }
        }
        Ok(())
    }

    /// Returns a clone of a payload's steps, or `None` if the name is unknown.
    pub fn steps(&self, name: &str) -> Option<Vec<Step>> {
        self.position(name).map(|i| self.payloads[i].steps.clone())
    }

    /// Removes a payload, returning whether one was present.
    pub fn delete(&mut self, name: &str) -> bool {
        match self.position(name) {
            Some(i) => {
                self.payloads.remove(i);
                true
            }
            None => false,
        }
    }

    /// Name and step count of every stored payload.
    pub fn list(&self) -> Vec<PayloadMeta> {
        self.payloads
            .iter()
            .map(|p| PayloadMeta { name: p.name.clone(), steps: p.steps.len() })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn steps(n: usize) -> Vec<Step> {
        (0..n).map(|_| Step::ReleaseAll).collect()
    }

    #[test]
    fn put_get_list_delete() {
        let mut s = PayloadStore::new();
        s.put("bios".into(), steps(3)).unwrap();
        s.put("oobe".into(), steps(1)).unwrap();
        assert_eq!(s.steps("bios").unwrap().len(), 3);
        assert_eq!(s.list().len(), 2);
        assert!(s.delete("bios"));
        assert!(!s.delete("bios"));
        assert!(s.steps("bios").is_none());
        assert_eq!(s.list(), vec![PayloadMeta { name: "oobe".into(), steps: 1 }]);
    }

    #[test]
    fn put_replaces_same_name_without_growing() {
        let mut s = PayloadStore::new();
        s.put("p".into(), steps(1)).unwrap();
        s.put("p".into(), steps(5)).unwrap();
        assert_eq!(s.list().len(), 1);
        assert_eq!(s.steps("p").unwrap().len(), 5);
    }

    #[test]
    fn rejects_bad_name_and_overflow() {
        let mut s = PayloadStore::new();
        assert!(s.put("".into(), steps(1)).is_err());
        assert!(s.put("x".repeat(MAX_NAME_LEN + 1), steps(1)).is_err());
        assert!(s.put("big".into(), steps(MAX_STEPS + 1)).is_err());
        for i in 0..MAX_PAYLOADS {
            s.put(format!("p{i}"), steps(1)).unwrap();
        }
        assert!(s.put("one_too_many".into(), steps(1)).is_err());
    }
}
