use anyhow::{Result, Error};
use serde::de::DeserializeOwned;
use serde_json::Value;

pub trait XValue {
	fn x_take<T: DeserializeOwned>(&mut self, name: &str) -> Result<T, Error>;
}

impl XValue for Value {
	fn x_take<T: DeserializeOwned>(&mut self, name: &str) -> Result<T, Error> {
		let value = self
			.get_mut(name)
			.map(Value::take)
			.ok_or(format!("No property '{name}' found.")).unwrap();

		let value: T = serde_json::from_value(value)?;
		Ok(value)
	}
}
