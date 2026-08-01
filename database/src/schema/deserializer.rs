use serde::{de::{self, Deserializer, MapAccess, Visitor}, Deserialize, Serialize, Serializer};
use std::fmt;

use crate::schema::{prestashop::{Associations, Order}, Qc, RecordId, RecordIdExt};

pub fn deserialize_to_string<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    struct StringOrIntVisitor;

    impl<'de> de::Visitor<'de> for StringOrIntVisitor {
        type Value = String;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a string or an integer")
        }

        fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(value.to_owned())
        }

        fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(value.to_string())
        }

        fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(value.to_string())
        }

        fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(value.to_string())
        }

        fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(value.to_string())
        }

        // fn visit_none<E>(self) -> Result<Self::Value, E>
        //     where
        //         E: de::Error, 
        // {
        //     Ok(String::new())
        // }

        fn visit_unit<E>(self) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            // Handle `null` as an empty string
            Ok(String::new())
        }
    }

    deserializer.deserialize_any(StringOrIntVisitor)
}


// Manual Serialize implementation for Qc
impl Serialize for Qc {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use serde::ser::SerializeMap;

        let mut map = serializer.serialize_map(None)?;

        // Serialize task as RecordId
        map.serialize_entry("task", &self.task)?;

        // Serialize order.id as RecordId
        let record_id = RecordId::new("qc", self.order.id.as_str());
        map.serialize_entry("id", &record_id)?;

        // Serialize remaining Order fields
        map.serialize_entry("id_order_type", &self.order.id_order_type)?;
        map.serialize_entry("id_address_delivery", &self.order.id_address_delivery)?;
        map.serialize_entry("id_address_invoice", &self.order.id_address_invoice)?;
        map.serialize_entry("id_customer", &self.order.id_customer)?;
        map.serialize_entry("current_state", &self.order.current_state)?;
        map.serialize_entry("invoice_number", &self.order.invoice_number)?;
        map.serialize_entry("invoice_date", &self.order.invoice_date)?;
        map.serialize_entry("payment", &self.order.payment)?;
        map.serialize_entry("date_add", &self.order.date_add)?;
        map.serialize_entry("date_upd", &self.order.date_upd)?;
        map.serialize_entry("id_employee_sales_rep", &self.order.id_employee_sales_rep)?;
        map.serialize_entry("id_employee_split_rep", &self.order.id_employee_split_rep)?;
        map.serialize_entry("id_employee_editing", &self.order.id_employee_editing)?;
        map.serialize_entry("id_order_everest", &self.order.id_order_everest)?;
        map.serialize_entry("id_store", &self.order.id_store)?;
        map.serialize_entry("total_paid", &self.order.total_paid)?;
        map.serialize_entry("delivery_date", &self.order.delivery_date)?;
        map.serialize_entry("total_products_wt", &self.order.total_products_wt)?;
        map.serialize_entry("total_paid_tax_excl", &self.order.total_paid_tax_excl)?;
        map.serialize_entry("reference", &self.order.reference)?;
        map.serialize_entry("id_order_parent", &self.order.id_order_parent)?;
        map.serialize_entry("shipping_number", &self.order.shipping_number)?;
        map.serialize_entry("order_type", &self.order.order_type)?;
        map.serialize_entry("associations", &self.order.associations)?;

        map.end()
    }
}

// Manual Deserialize implementation for Qc
impl<'de> Deserialize<'de> for Qc {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct QcVisitor;

        impl<'de> Visitor<'de> for QcVisitor {
            type Value = Qc;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a Qc struct with task and flattened Order fields")
            }

            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                let mut task: Option<RecordId> = None;
                let mut id: Option<String> = None;
                let mut id_order_type: Option<String> = None;
                let mut id_address_delivery: Option<String> = None;
                let mut id_address_invoice: Option<String> = None;
                let mut id_customer: Option<String> = None;
                let mut current_state: Option<String> = None;
                let mut invoice_number: Option<String> = None;
                let mut invoice_date: Option<String> = None;
                let mut payment: Option<String> = None;
                let mut date_add: Option<String> = None;
                let mut date_upd: Option<String> = None;
                let mut id_employee_sales_rep: Option<String> = None;
                let mut id_employee_split_rep: Option<String> = None;
                let mut id_employee_editing: Option<String> = None;
                let mut id_order_everest: Option<String> = None;
                let mut id_store: Option<String> = None;
                let mut total_paid: Option<String> = None;
                let mut delivery_date: Option<String> = None;
                let mut total_products_wt: Option<String> = None;
                let mut total_paid_tax_excl: Option<String> = None;
                let mut reference: Option<String> = None;
                let mut id_order_parent: Option<String> = None;
                let mut shipping_number: Option<String> = None;
                let mut order_type: Option<String> = None;
                let mut associations: Option<Associations> = None;
                let mut total_discounts_tax_excl: Option<String> = None;
                let mut total_products: Option<String> = None;
                
                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "task" => task = Some(map.next_value()?),
                        "id" => {
                            // Handle RecordId (Thing) or string
                            let thing: RecordId = map.next_value()?;
                            id = Some(thing.key_string());
                        }
                        "id_order_type" => id_order_type = Some(map.next_value()?),
                        "id_address_delivery" => id_address_delivery = Some(map.next_value()?),
                        "id_address_invoice" => id_address_invoice = Some(map.next_value()?),
                        "id_customer" => id_customer = Some(map.next_value()?),
                        "current_state" => current_state = Some(map.next_value()?),
                        "invoice_number" => invoice_number = Some(map.next_value()?),
                        "invoice_date" => invoice_date = Some(map.next_value()?),
                        "payment" => payment = Some(map.next_value()?),
                        "date_add" => date_add = Some(map.next_value()?),
                        "date_upd" => date_upd = Some(map.next_value()?),
                        "id_employee_sales_rep" => id_employee_sales_rep = Some(map.next_value()?),
                        "id_employee_split_rep" => id_employee_split_rep = Some(map.next_value()?),
                        "id_employee_editing" => id_employee_editing = Some(map.next_value()?),
                        "id_order_everest" => id_order_everest = Some(map.next_value()?),
                        "id_store" => id_store = Some(map.next_value()?),
                        "total_paid" => total_paid = Some(map.next_value()?),
                        "delivery_date" => delivery_date = Some(map.next_value()?),
                        "total_products" => total_products = Some(map.next_value()?),
                        "total_products_wt" => total_products_wt = Some(map.next_value()?),
                        "total_paid_tax_excl" => total_paid_tax_excl = Some(map.next_value()?),
                        "reference" => reference = Some(map.next_value()?),
                        "id_order_parent" => id_order_parent = Some(map.next_value()?),
                        "shipping_number" => shipping_number = Some(map.next_value()?),
                        "order_type" => order_type = Some(map.next_value()?),
                        "associations" => associations = Some(map.next_value()?),
                        "total_discounts_tax_excl" => total_discounts_tax_excl = Some(map.next_value()?),
                        _ => {
                            // Ignore unknown fields
                            let _ = map.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }

                let task = task.ok_or_else(|| de::Error::missing_field("task"))?;
                let id = id.ok_or_else(|| de::Error::missing_field("id"))?;
                let id_order_type =
                    id_order_type.ok_or_else(|| de::Error::missing_field("id_order_type"))?;
                let associations =
                    associations.ok_or_else(|| de::Error::missing_field("associations"))?;

                Ok(Qc {
                    task,
                    order: Order {
                        id,
                        id_order_type,
                        id_address_delivery: id_address_delivery.unwrap_or_default(),
                        id_address_invoice: id_address_invoice.unwrap_or_default(),
                        id_customer: id_customer.unwrap_or_default(),
                        current_state: current_state.unwrap_or_default(),
                        invoice_number: invoice_number.unwrap_or_default(),
                        invoice_date: invoice_date.unwrap_or_default(),
                        payment: payment.unwrap_or_default(),
                        date_add: date_add.unwrap_or_default(),
                        date_upd: date_upd.unwrap_or_default(),
                        id_employee_sales_rep: id_employee_sales_rep.unwrap_or_default(),
                        id_employee_split_rep: id_employee_split_rep.unwrap_or_default(),
                        id_employee_editing: id_employee_editing.unwrap_or_default(),
                        id_order_everest: id_order_everest.unwrap_or_default(),
                        id_store: id_store.unwrap_or_default(),
                        total_paid: total_paid.unwrap_or_default(),
                        delivery_date: delivery_date.unwrap_or_default(),
                        total_products_wt: total_products_wt.unwrap_or_default(),
                        total_paid_tax_excl: total_paid_tax_excl.unwrap_or_default(),
                        reference: reference.unwrap_or_default(),
                        id_order_parent: id_order_parent.unwrap_or_default(),
                        shipping_number: shipping_number.unwrap_or_default(),
                        order_type: order_type.unwrap_or_default(),
                        associations,
                        total_discounts_tax_excl: total_discounts_tax_excl.unwrap_or_default(),
                        total_products: total_products.unwrap_or_default(),
                    },
                })
            }
        }

        deserializer.deserialize_map(QcVisitor)
    }
}