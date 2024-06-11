use serde::{de::Deserializer, Deserialize};
use serde_json::Value;
use serde::de::DeserializeOwned;


pub fn deserialize_nested<'de, D, T>(deserializer: D) -> Result<T, D::Error>
    where D: Deserializer<'de>, T: DeserializeOwned,
{
    // Deserialize the entire input as a serde_json::Value
    let value: Value = Deserialize::deserialize(deserializer)?;

    // Extract the nested value we are interested in (e.g., "employees", "orders", etc.)
    let nested_value = value
        .as_object()
        .and_then(|obj| obj.values().next())
        .ok_or_else(|| serde::de::Error::custom("Missing nested value"))?;

    // Deserialize the nested value into the target type
    let nested_str = serde_json::to_string(nested_value).map_err(serde::de::Error::custom)?;
    let nested: T = serde_json::from_str(&nested_str).map_err(serde::de::Error::custom)?;
    Ok(nested)
}