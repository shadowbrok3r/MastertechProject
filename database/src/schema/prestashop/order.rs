use crate::schema::deserializer::deserialize_to_string;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use crate::schema::prestashop::PRESTASHOP_API_URL_WASM;

#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub struct Order {
    #[serde(deserialize_with = "deserialize_to_string")]
    pub id: String,
    // #[serde(skip_serializing_if = "Option::is_none")]
    // #[serde(deserialize_with = "deserialize_to_string")]
    pub id_order_type: String,
    #[serde(default)]
    pub id_address_delivery: String, // ✔️
    #[serde(default)]
    pub id_address_invoice: String,  // ✔️
    #[serde(default)]
    pub id_customer: String,         // ✔️
    #[serde(default)]
    pub current_state: String,
    #[serde(default)]
    pub invoice_number: String,
    #[serde(default)]
    pub invoice_date: String,  
    #[serde(default)]
    pub payment: String,
    #[serde(default)]
    pub date_add: String,
    #[serde(default)]
    pub date_upd: String,
    #[serde(default)]
    pub id_employee_sales_rep: String,
    #[serde(default)]
    pub id_employee_split_rep: String,
    #[serde(default)]
    pub id_employee_editing: String,
    #[serde(default)]
    pub id_order_everest: String,
    #[serde(default)]
    pub id_store: String,   // 1 = warehouse
    #[serde(default)]
    pub total_paid: String, // ✔️
    #[serde(default)]
    pub delivery_date: String,
    #[serde(default)]
    pub total_products: String,
    #[serde(default)]
    pub total_products_wt: String,
    #[serde(default)]
    pub total_paid_tax_excl: String,
    #[serde(default)]
    pub total_discounts_tax_excl: String,
    #[serde(default)]
    pub reference: String, // what prestashop sees since order id and reference are different...
    #[serde(default)]
    pub id_order_parent: String, // no idea
    #[serde(default)]
    pub shipping_number: String, // Tracking number
    #[serde(default)]
    pub order_type: String, // Configurator / Sales Order
    // note: String,
    // #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub associations: Associations,
}

#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub struct Associations {
    #[serde(default = "new_vec")]
    pub order_rows: Vec<OrderRow>,
    #[serde(default = "new_svc_vec")]
    pub order_service: Vec<ServiceOrder>,
}

#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub enum OrderState {
    #[default]
    AcceptedByOdoo,
    Shipped,
    DeliveredToStore,
    DoneShelf,
    OrderPlaced,
    PrePulled,
    ReadyToBuild,
    QcAndBurnin,
    ShipToStore,
    OdooPendingReview,
    Returned 
}

impl OrderState {
    pub fn as_str(&self) -> &str {
        match self {
            Self::AcceptedByOdoo => "Accepted By Odoo",
            Self::Shipped => "Shipped",
            Self::DeliveredToStore => "Delivered To Store",
            Self::DoneShelf => "Done Shelf",
            Self::OrderPlaced => "Order Placed",
            Self::PrePulled => "Pre Pulled",
            Self::ReadyToBuild => "Ready To Build",
            Self::QcAndBurnin => "Qc & Burnin",
            Self::ShipToStore => "Ship To Store",
            Self::OdooPendingReview => "Odoo Pending Review",
            Self::Returned => "Returned",
        }
    }

    pub fn from_id_str(id: &str) -> &str {
        match id {
            "239" => "Accepted By Odoo",
            "4" => "Shipped",
            "238" => "Delivered To Store",
            "40" => "Done Shelf",
            "73" => "Order Placed",
            "70" => "Pre Pulled",
            "224" => "Ready To Build",
            "71" => "Qc & Burnin",
            "236" => "Ship To Store",
            "84" => "Returned",
            "30" => "In Repair",
            "29" => "Check-in Shelf",
            "242" => "Odoo Pending Review",
            _ => "Accepted By Odoo"
        }
    }

    pub fn state_from_id_str(id: &str) -> Self {
        match id {
            "239" => Self::AcceptedByOdoo,
            "4" => Self::Shipped,
            "238" => Self::DeliveredToStore,
            "40" => Self::DoneShelf,
            "73" => Self::OrderPlaced,
            "70" => Self::PrePulled,
            "224" => Self::ReadyToBuild,
            "71" => Self::QcAndBurnin,
            "236" => Self::ShipToStore,
            "84" => Self::Returned,
            "242" => Self::OdooPendingReview,
            _ => Self::AcceptedByOdoo
        }
    }

    /*84=Returned, 30=In Repair, 239=Accepted by Odoo?, 29=CheckinShelf, 40=DoneShelf, 73=Order Placed, 70=PrePulled236=ShipToStore */
    pub fn id(&self) -> i32 {
        match self {
            Self::AcceptedByOdoo => 239,
            Self::Shipped => 4,
            Self::DeliveredToStore => 238,
            Self::DoneShelf => 40,
            Self::OrderPlaced => 73,
            Self::PrePulled => 70,
            Self::ReadyToBuild => 224,
            Self::QcAndBurnin => 71,
            Self::ShipToStore => 236,
            Self::OdooPendingReview => 242,
            Self::Returned => 84,
        }
    }

    pub const VALUES: [Self; 11] = [
        Self::AcceptedByOdoo,
        Self::Shipped,
        Self::DeliveredToStore,
        Self::DoneShelf,
        Self::OrderPlaced,
        Self::PrePulled,
        Self::ReadyToBuild,
        Self::QcAndBurnin,
        Self::ShipToStore,
        Self::OdooPendingReview,
        Self::Returned,
    ];
}

fn new_vec() -> Vec<OrderRow> {
    Vec::new()
}

fn new_svc_vec() -> Vec<ServiceOrder> {
    Vec::new()
}

#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub struct OrderRow {
    #[serde(deserialize_with = "deserialize_to_string")]
    pub id: String,
    pub id_order_config: String,
    pub product_id: String,
    pub product_quantity: String,
    pub product_name: String,
    pub product_price: String,
    pub product_reference: String
}

#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub struct ServiceOrder {
    // pub id: String,
    #[serde(deserialize_with = "deserialize_to_string")]
    pub id_order_service: String,
    // pub id_order: String,
    #[serde(deserialize_with = "deserialize_to_string")]
    pub device_name: String,
    #[serde(deserialize_with = "deserialize_to_string")]
    pub device_mfg: String,
    #[serde(deserialize_with = "deserialize_to_string")]
    pub device_model: String,
    #[serde(deserialize_with = "deserialize_to_string")]
    pub device_serial: String,
    #[serde(deserialize_with = "deserialize_to_string")]
    pub device_password: String,
    // pub id_status_service: String, // This is fucky
    #[serde(deserialize_with = "deserialize_to_string")]
    pub device_power_supply: String,
    #[serde(deserialize_with = "deserialize_to_string")]
    pub other_hardware_software: String,
    #[serde(deserialize_with = "deserialize_to_string")]
    pub physical_damage: String,
    #[serde(deserialize_with = "deserialize_to_string")]
    pub check_in_notes: String,
    #[serde(deserialize_with = "deserialize_to_string")]
    pub intake_notes: String,
    // pub id_employee_qc_tech: String,
    // pub id_employee_qc_signoff: String,
}

/*
isUnsignedId
isUnsignedId
isUnsignedId
isUnsignedId
isUnsignedId
isUnsignedId
isUnsignedId
isModuleName
isGenericName
isPrice
isPrice
isFloat
*/

impl Order {
    pub async fn create_prestashop_order(&self, client: Client) ->anyhow::Result<(), anyhow::Error> {
                // Prepare the XML payload
        let payload = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
            <prestashop xmlns:xlink="http://www.w3.org/1999/xlink">
                <order>
                    <id_address_delivery required="true"></id_address_delivery>
                    <id_address_invoice required="true"></id_address_invoice>
                    <id_cart required="true"></id_cart>
                    <id_currency required="true"></id_currency>
                    <id_lang required="true"></id_lang>
                    <id_customer required="true"></id_customer>
                    <id_carrier required="true"></id_carrier>
                    <module required="true"></module>
                    <payment required="true"></payment>
                    <total_products required="true"></total_products>
                    <total_products_wt required="true"></total_products_wt>
                    <conversion_rate required="true"></conversion_rate>
                    <associations>
                        <order_rows nodeType="order_row" virtualEntity="true">
                            <order_row>
                                <id></id>
                                <id_order_config read_only="true" readOnly="true"></id_order_config>
                                <product_id xlink:href="https://pclaptops.mojo11.com/api/products/" required="true"></product_id>
                                <product_attribute_id required="true"></product_attribute_id>
                                <product_quantity required="true"></product_quantity>
                            </order_row>
                        </order_rows>
                        <order_serial nodeType="order_serial" virtualEntity="true">
                            <order_serial>
                                <id_order></id_order>
                                <id_order_detail></id_order_detail>
                                <product_reference></product_reference>
                                <serial_number></serial_number>
                                <id_order_config></id_order_config>
                            </order_serial>
                        </order_serial>
                        <order_config nodeType="order_config" virtualEntity="true">
                            <order_config>
                                <id_order_config></id_order_config>
                                <id_order></id_order>
                                <name></name>
                                <id_config></id_config>
                            </order_config>
                        </order_config>
                        <order_service nodeType="order_service" api="order_service">
                            <order_service>
                                <id_order_service></id_order_service>
                                <device_name></device_name>
                                <device_mfg></device_mfg>
                                <device_model></device_model>
                                <device_serial></device_serial>
                                <device_password></device_password>
                                <id_status_service></id_status_service>
                                <device_power_supply></device_power_supply>
                                <other_hardware_software></other_hardware_software>
                                <physical_damage></physical_damage>
                                <check_in_notes></check_in_notes>
                                <intake_notes></intake_notes>
                                <id_employee_qc_tech></id_employee_qc_tech>
                                <id_employee_qc_signoff></id_employee_qc_signoff>
                            </order_service>
                        </order_service>
                    </associations>
                </order>
            </prestashop>"#
        );

        // Send HTTP POST request with the XML payload
        log::info!("prestashop_schema -> Payload: {:?}", payload);
        let response_text = client
            .post(format!("{PRESTASHOP_API_URL_WASM}/customer_messages"))
            .header("Content-type", "application/xml")
            .body(payload)
            .send()
            .await?
            .text()
            .await?;

        log::info!("prestashop_schema -> response text: {response_text:?}");
        // Parse the XML response to extract values
        let _id = response_text
            .split("<id><![CDATA[")
            .nth(1)
            .and_then(|s| s.split("]]></id>").next())
            .ok_or_else(|| anyhow::anyhow!("Failed to parse 'id' from response"))?;

        let _date_add = response_text
            .split("<date_add><![CDATA[")
            .nth(1)
            .and_then(|s| s.split("]]></date_add>").next())
            .ok_or_else(|| anyhow::anyhow!("Failed to parse 'date_add' from response"))?;

        let _date_upd = response_text
            .split("<date_upd><![CDATA[")
            .nth(1)
            .and_then(|s| s.split("]]></date_upd>").next())
            .unwrap_or(""); // Optional field, so we handle it accordingly

        // super::helper_traits::PrestaResourceResponse {
        //     date_add: super::helper_traits::convert_date_string(date_add)?.to_string(), //,
        //     id: id.to_string(),
        //     date_upd: super::helper_traits::convert_date_string(date_upd)?.to_string(), // date_upd.to_string(),
        // }
        Ok(())
    }
}
