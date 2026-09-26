//! PrestaShop-shaped employee lookups; a Shopify source needs an XBM email lookup and a server-side `id_prestashop` change.
#![allow(async_fn_in_trait)]

use std::collections::HashMap;

use crate::schema::{
    Store,
    prestashop_schema::{Employee, Prestashop},
};

/// PrestaShop employee fields the directory requests.
const EMPLOYEE_DISPLAY: &str = "[id,firstname,lastname,email,active,id_store]";

/// One employee as the directory reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmployeeRecord {
    pub id: u64,
    pub email: String,
    pub first_name: String,
    pub last_name: String,
    pub id_store: String,
    pub active: bool,
}

impl EmployeeRecord {
    /// The employee's store, or `None` for an unmapped store id.
    pub fn store(&self) -> Option<Store> {
        Store::try_from_presta_store_id(self.id_store.trim())
    }

    /// "First Last", trimmed.
    pub fn name(&self) -> String {
        format!("{} {}", self.first_name.trim(), self.last_name.trim())
            .trim()
            .to_string()
    }
}

impl TryFrom<Employee> for EmployeeRecord {
    type Error = anyhow::Error;

    /// Fails on a non-numeric id or an `active` flag other than "1" or "0".
    fn try_from(emp: Employee) -> anyhow::Result<Self> {
        let id = emp
            .id
            .trim()
            .parse::<u64>()
            .map_err(|e| anyhow::anyhow!("employee id {:?} is not a number: {e}", emp.id))?;
        let active = match emp.active.trim() {
            "1" => true,
            "0" => false,
            other => anyhow::bail!("employee {id} has an unreadable active flag {other:?}"),
        };
        Ok(Self {
            id,
            email: emp.email.trim().to_lowercase(),
            first_name: emp.firstname.trim().to_string(),
            last_name: emp.lastname.trim().to_string(),
            id_store: emp.id_store.trim().to_string(),
            active,
        })
    }
}

impl From<EmployeeRecord> for Employee {
    fn from(record: EmployeeRecord) -> Self {
        Employee {
            id: record.id.to_string(),
            id_store: record.id_store,
            lastname: record.last_name,
            firstname: record.first_name,
            email: record.email,
            active: if record.active { "1" } else { "0" }.to_string(),
            ..Default::default()
        }
    }
}

/// Employee lookups; `Ok(None)` is a miss and `Err` means the source was unreachable or unreadable.
pub trait EmployeeDirectory {
    async fn by_email(&self, email: &str) -> anyhow::Result<Option<EmployeeRecord>>;
    async fn by_id(&self, id: u64) -> anyhow::Result<Option<EmployeeRecord>>;
}

/// PrestaShop `employees` webservice.
#[derive(Debug, Clone, Copy, Default)]
pub struct PrestashopDirectory;

/// The directory sign-up and the sync use.
pub fn employee_directory() -> PrestashopDirectory {
    PrestashopDirectory
}

impl PrestashopDirectory {
    async fn list(&self, filter: &str, value: &str) -> anyhow::Result<Vec<Employee>> {
        if !crate::prestashop_configured() {
            return Err(crate::prestashop_unconfigured_err());
        }
        let mut api = Prestashop::default();
        api.display = EMPLOYEE_DISPLAY;
        let bracketed = format!("[{value}]");
        let mut query: HashMap<&str, &str> = HashMap::new();
        query.insert(filter, bracketed.as_str());
        query.insert("output_format", "JSON");
        api.request_resources_checked("employees", query).await
    }
}

impl EmployeeDirectory for PrestashopDirectory {
    async fn by_email(&self, email: &str) -> anyhow::Result<Option<EmployeeRecord>> {
        let email = email.trim().to_lowercase();
        if email.is_empty() {
            return Ok(None);
        }
        let rows = self.list("filter[email]", &email).await?;
        pick_employee(rows, &email)
    }

    async fn by_id(&self, id: u64) -> anyhow::Result<Option<EmployeeRecord>> {
        let wanted = id.to_string();
        let rows = self.list("filter[id]", &wanted).await?;
        rows.into_iter()
            .find(|emp| emp.id.trim() == wanted)
            .map(EmployeeRecord::try_from)
            .transpose()
    }
}

/// The row whose email matches case-insensitively, preferring an active one.
pub fn pick_employee(rows: Vec<Employee>, email: &str) -> anyhow::Result<Option<EmployeeRecord>> {
    let email = email.trim();
    let matches = rows
        .into_iter()
        .filter(|emp| emp.email.trim().eq_ignore_ascii_case(email))
        .map(EmployeeRecord::try_from)
        .collect::<anyhow::Result<Vec<_>>>()?;
    let active = matches.iter().position(|record| record.active);
    Ok(match active {
        Some(i) => matches.into_iter().nth(i),
        None => matches.into_iter().next(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn employee(id: &str, email: &str, active: &str, id_store: &str) -> Employee {
        Employee {
            id: id.into(),
            email: email.into(),
            active: active.into(),
            id_store: id_store.into(),
            firstname: " Bob ".into(),
            lastname: "Smith ".into(),
            ..Default::default()
        }
    }

    #[test]
    fn the_email_matches_case_insensitively() {
        let rows = vec![
            employee("1", "other@pclaptops.com", "1", "7"),
            employee("2", "Bob.Smith@PCLaptops.com", "1", "12"),
        ];
        let found = pick_employee(rows, "bob.smith@pclaptops.com")
            .expect("readable")
            .expect("match");
        assert_eq!(found.id, 2);
        assert_eq!(found.email, "bob.smith@pclaptops.com");
        assert_eq!(found.store(), Some(Store::SAN));
        assert_eq!(found.name(), "Bob Smith");
    }

    #[test]
    fn an_active_row_wins_over_an_inactive_one() {
        let rows = vec![
            employee("3", "bob@pclaptops.com", "0", "7"),
            employee("4", "bob@pclaptops.com", "1", "8"),
        ];
        let found = pick_employee(rows, "bob@pclaptops.com")
            .expect("readable")
            .expect("match");
        assert_eq!(found.id, 4);
        assert!(found.active);
    }

    #[test]
    fn an_inactive_only_match_is_returned_inactive() {
        let rows = vec![employee("3", "bob@pclaptops.com", "0", "7")];
        let found = pick_employee(rows, "bob@pclaptops.com")
            .expect("readable")
            .expect("match");
        assert!(!found.active);
    }

    #[test]
    fn no_matching_row_is_a_miss() {
        let rows = vec![employee("1", "other@pclaptops.com", "1", "7")];
        assert_eq!(
            pick_employee(rows, "bob@pclaptops.com").expect("readable"),
            None
        );
        assert_eq!(
            pick_employee(Vec::new(), "bob@pclaptops.com").expect("readable"),
            None
        );
    }

    #[test]
    fn a_missing_or_garbled_active_flag_is_an_error() {
        for active in ["", "yes", "true", "2"] {
            let rows = vec![employee("5", "bob@pclaptops.com", active, "7")];
            assert!(
                pick_employee(rows, "bob@pclaptops.com").is_err(),
                "active {active:?}"
            );
        }
    }

    #[test]
    fn a_non_numeric_id_is_an_error() {
        let rows = vec![employee("abc", "bob@pclaptops.com", "1", "7")];
        assert!(pick_employee(rows, "bob@pclaptops.com").is_err());
    }

    #[test]
    fn the_warehouse_and_unmapped_stores_resolve_as_expected() {
        let war =
            EmployeeRecord::try_from(employee("6", "w@pclaptops.com", "1", "1")).expect("readable");
        assert_eq!(war.store(), Some(Store::WAR));
        let unmapped = EmployeeRecord::try_from(employee("7", "u@pclaptops.com", "1", "11"))
            .expect("readable");
        assert_eq!(unmapped.store(), None);
    }

    #[test]
    fn an_employee_missing_active_in_json_fails_closed() {
        let json = r#"{"id": 9, "id_store": "7", "lastname": "Smith", "firstname": "Bob", "email": "bob@pclaptops.com"}"#;
        let emp: Employee = serde_json::from_str(json).expect("decodes without active or initials");
        assert!(EmployeeRecord::try_from(emp).is_err());
    }

    #[test]
    fn a_record_converts_back_to_an_employee() {
        let record = EmployeeRecord::try_from(employee("8", "bob@pclaptops.com", "0", "14"))
            .expect("readable");
        let emp = Employee::from(record.clone());
        assert_eq!(EmployeeRecord::try_from(emp).expect("round trip"), record);
    }
}
