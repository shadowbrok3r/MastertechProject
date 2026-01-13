use crate::schema::deserializer::deserialize_to_string;
use crate::schema::prestashop::{PRESTASHOP_API_URL_WASM, OrderType, Prestashop};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use log::info;

/// Comprehensive specs extracted from an order
#[derive(Debug, Default, Clone)]
pub struct ExtractedOrderSpecs {
    pub cpu: String,
    pub gpu: String,
    pub ram: String,
    pub device_serial: String,
    pub device_mfg: String,
}

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
    #[serde(default = "new_order_serial_vec")]
    pub order_serial: Vec<OrderSerial>,
}

#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub struct OrderSerial {
    pub id_order: String,
    pub id_order_detail: String,
    pub serial_number: String,
    pub product_reference: String,
}

#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub struct OrderDetail {
    #[serde(deserialize_with = "deserialize_to_string")]
    pub id: String,
    pub id_order: String,
    pub detail_notes: String,
    pub product_id: String,
    pub product_name: String,
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
    Returned,
    InRepair,
    CheckinShelf,
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
            Self::InRepair => "In Repair",
            Self::CheckinShelf => "Checkin Shelf",
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
            "30" => Self::InRepair,
            "29" => Self::CheckinShelf,
            _ => Self::AcceptedByOdoo
        }
    }

    /*84=Returned, 30=In Repair, 239=Accepted by Odoo?, 29=CheckinShelf, 40=DoneShelf, 73=Order Placed, 70=PrePulled236=ShipToStore */
    pub fn to_id(&self) -> i32 {
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
            Self::InRepair => 30,
            Self::CheckinShelf => 29,
        }
    }

    pub fn to_id_str(&self) -> &str {
        match self {
            Self::AcceptedByOdoo => "239",
            Self::Shipped => "4",
            Self::DeliveredToStore => "238",
            Self::DoneShelf => "40",
            Self::OrderPlaced => "73",
            Self::PrePulled => "70",
            Self::ReadyToBuild => "224",
            Self::QcAndBurnin => "71",
            Self::ShipToStore => "236",
            Self::OdooPendingReview => "242",
            Self::Returned => "84",
            Self::InRepair => "30",
            Self::CheckinShelf => "29",
        }
    }

    pub const VALUES: [Self; 13] = [
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
        Self::InRepair,
        Self::CheckinShelf,
    ];
}

fn new_vec() -> Vec<OrderRow> {
    Vec::new()
}

fn new_svc_vec() -> Vec<ServiceOrder> {
    Vec::new()
}

fn new_order_serial_vec() -> Vec<OrderSerial> {
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
    
    /// Extract CPU, GPU, RAM, device_serial, and device_mfg from order rows
    /// This checks serialized products, RCI order details, and parses laptop product names
    pub async fn extract_specs(&self) -> ExtractedOrderSpecs {
        let mut specs = ExtractedOrderSpecs::default();
        
        // Step 1: Extract device_serial and device_mfg from main system product (LAP/ or CASE/)
        // Also check for explicit CPU/GPU/RAM serialized parts
        for row in self.associations.order_rows.iter() {
            let r = row.product_reference.to_lowercase();
            let name = &row.product_name;
            
            // Main system product - extract manufacturer from product name
            if r.starts_with("lap/") || (r.starts_with("case/") && !r.starts_with("case/15") && !r.starts_with("case/17")) {
                // Extract manufacturer from product name (first word or brand identifier)
                specs.device_mfg = Self::extract_manufacturer_from_name(name);
                
                // Find the serial number for this product in order_serial
                for serial in self.associations.order_serial.iter() {
                    if serial.product_reference.to_lowercase() == r && !serial.serial_number.is_empty() {
                        specs.device_serial = serial.serial_number.clone();
                        break;
                    }
                }
            }
            
            // CPU detection from serialized parts
            if r.starts_with("cpu/") {
                specs.cpu = name.clone();
            }
            
            // GPU detection from serialized parts
            if r.starts_with("gpu/") || r.starts_with("vid/") {
                specs.gpu = name.clone();
            }
            
            // RAM detection from serialized parts
            if r.starts_with("ddr5/") || r.starts_with("ddr4/") || r.starts_with("ram/") || r.starts_with("mem/") 
                || r.starts_with("lap/ddr") {
                specs.ram = name.clone();
            }
        }
        
        // Step 2: For RCI systems, fetch OrderDetail and parse specs from detail_notes
        let is_rci = self.id_order_type == OrderType::Rci.to_id().to_string();
        if is_rci && (specs.cpu.is_empty() || specs.gpu.is_empty() || specs.ram.is_empty()) {
            info!("RCI order {} - looking for U/DESKTOP, U/LAPTOPS, or RCI/ in {} order_serials", 
                  self.id, self.associations.order_serial.len());
            
            // Find U/DESKTOP, U/LAPTOPS, or RCI-prefixed product in order_serial to get id_order_detail
            for serial in self.associations.order_serial.iter() {
                let ref_lower = serial.product_reference.to_lowercase();
                info!("  Checking serial: product_ref='{}', id_order_detail='{}'", 
                      serial.product_reference, serial.id_order_detail);
                
                // Match U/DESKTOP, U/LAPTOPS, or any RCI/ prefixed products
                let is_rci_product = ref_lower == "u/desktop" 
                    || ref_lower == "u/laptops" 
                    || ref_lower == "u/laptop"
                    || ref_lower.starts_with("rci/");
                
                if is_rci_product && !serial.id_order_detail.is_empty() && serial.id_order_detail != "0" {
                    info!("  Found matching serial, fetching OrderDetail id={}", serial.id_order_detail);
                    
                    // Also capture serial number for RCI products
                    if specs.device_serial.is_empty() && !serial.serial_number.is_empty() {
                        specs.device_serial = serial.serial_number.clone();
                    }
                    
                    // Fetch OrderDetail using id_order_detail
                    let presta = Prestashop::default();
                    match presta.request_subresources_by_id_wasm::<OrderDetail>(
                        "order_details", 
                        "order_detail", 
                        &serial.id_order_detail
                    ).await {
                        Ok(detail) => {
                            info!("  OrderDetail fetched, detail_notes length={}", detail.detail_notes.len());
                            if !detail.detail_notes.is_empty() {
                                info!("  detail_notes: {:?}", &detail.detail_notes[..detail.detail_notes.len().min(200)]);
                            }
                            
                            // Parse detail_notes format: "Brand: DELL\r\nCPU: i7-10610U\r\nRAM: 16GB\r\n..."
                            let (parsed_cpu, parsed_gpu, parsed_ram, parsed_brand) = Self::parse_detail_notes(&detail.detail_notes);
                            info!("  Parsed specs: cpu='{}', gpu='{}', ram='{}', brand='{}'", parsed_cpu, parsed_gpu, parsed_ram, parsed_brand);
                            
                            if specs.cpu.is_empty() && !parsed_cpu.is_empty() {
                                specs.cpu = parsed_cpu;
                            }
                            if specs.gpu.is_empty() && !parsed_gpu.is_empty() {
                                specs.gpu = parsed_gpu;
                            }
                            if specs.ram.is_empty() && !parsed_ram.is_empty() {
                                specs.ram = parsed_ram;
                            }
                            if specs.device_mfg.is_empty() && !parsed_brand.is_empty() {
                                specs.device_mfg = parsed_brand;
                            }
                            
                            // If we found specs, break out
                            if !specs.cpu.is_empty() || !specs.gpu.is_empty() || !specs.ram.is_empty() {
                                break;
                            }
                        }
                        Err(e) => {
                            info!("  Failed to fetch OrderDetail: {:?}", e);
                        }
                    }
                }
            }
            
            // Fallback: If still missing specs, check ALL order_serial entries for detail_notes
            if specs.cpu.is_empty() || specs.gpu.is_empty() || specs.ram.is_empty() {
                info!("RCI order {} - fallback: checking all order_serial entries for detail_notes", self.id);
                for serial in self.associations.order_serial.iter() {
                    if serial.id_order_detail.is_empty() || serial.id_order_detail == "0" {
                        continue;
                    }
                    
                    let presta = Prestashop::default();
                    match presta.request_subresources_by_id_wasm::<OrderDetail>(
                        "order_details", 
                        "order_detail", 
                        &serial.id_order_detail
                    ).await {
                        Ok(detail) => {
                            if !detail.detail_notes.is_empty() {
                                info!("  Found detail_notes in serial '{}': {:?}", 
                                      serial.product_reference, 
                                      &detail.detail_notes[..detail.detail_notes.len().min(200)]);
                                
                                let (parsed_cpu, parsed_gpu, parsed_ram, parsed_brand) = Self::parse_detail_notes(&detail.detail_notes);
                                
                                if specs.cpu.is_empty() && !parsed_cpu.is_empty() {
                                    specs.cpu = parsed_cpu;
                                }
                                if specs.gpu.is_empty() && !parsed_gpu.is_empty() {
                                    specs.gpu = parsed_gpu;
                                }
                                if specs.ram.is_empty() && !parsed_ram.is_empty() {
                                    specs.ram = parsed_ram;
                                }
                                if specs.device_mfg.is_empty() && !parsed_brand.is_empty() {
                                    specs.device_mfg = parsed_brand;
                                }
                                
                                if !specs.cpu.is_empty() && !specs.gpu.is_empty() && !specs.ram.is_empty() {
                                    break;
                                }
                            }
                        }
                        Err(e) => {
                            info!("  Failed to fetch OrderDetail {}: {:?}", serial.id_order_detail, e);
                        }
                    }
                }
            }
        }
        
        // Step 3: For non-RCI laptops, parse specs from LAP/ product name
        if specs.cpu.is_empty() || specs.gpu.is_empty() {
            for row in self.associations.order_rows.iter() {
                let r = row.product_reference.to_lowercase();
                if r.starts_with("lap/") {
                    // Parse CPU and GPU from laptop product name
                    // Examples: "SM-5 15" RTX 5060 Core Ultra 7 275HX", "SM3 14" RYZEN 7 255"
                    let (parsed_cpu, parsed_gpu) = Self::parse_laptop_product_name(&row.product_name);
                    info!("  Parsed laptop '{}': cpu='{}', gpu='{}'", row.product_name, parsed_cpu, parsed_gpu);
                    if specs.cpu.is_empty() && !parsed_cpu.is_empty() {
                        specs.cpu = parsed_cpu;
                    }
                    if specs.gpu.is_empty() && !parsed_gpu.is_empty() {
                        specs.gpu = parsed_gpu;
                    }
                    break;
                }
            }
        }
        
        info!("Order {} extracted specs: cpu='{}', gpu='{}', ram='{}', serial='{}', mfg='{}'",
              self.id, specs.cpu, specs.gpu, specs.ram, specs.device_serial, specs.device_mfg);
        
        specs
    }
    
    /// Extract the main model name from order rows
    /// For RCI orders: Returns the product_name directly
    /// For non-RCI orders: Returns product_name for the main system product
    /// Note: For fetching order_config name, use extract_model_with_config which makes async API calls
    pub fn extract_model(&self) -> String {
        // Find the main system product row
        let main_row = self.associations.order_rows.iter().find(|row| {
            let r = row.product_reference.to_lowercase();
            r.starts_with("lap/") 
                || (r.starts_with("case/") && !r.starts_with("case/15") && !r.starts_with("case/17"))
                || r.starts_with("bsd/")
                || r.starts_with("rci/")
                || r.starts_with("r2r/")
                || r.starts_with("rtr/")
        });
        
        if let Some(row) = main_row {
            return row.product_name.clone();
        }
        
        // Fallback to first product
        self.associations.order_rows.first()
            .map(|r| r.product_name.clone())
            .unwrap_or_default()
    }
    
    /// Extract drives info from order rows
    pub fn extract_drives(&self) -> Vec<(String, String)> {
        let mut drives = Vec::new();
        
        for row in self.associations.order_rows.iter() {
            let r = row.product_reference.to_lowercase();
            
            if r.starts_with("m.2/") || r.starts_with("ssd/") {
                drives.push((row.product_name.clone(), "SSD".to_string()));
            } else if r.starts_with("hdd/") {
                drives.push((row.product_name.clone(), "HDD".to_string()));
            }
        }
        
        drives
    }
    
    /// Extract motherboard from order rows
    pub fn extract_motherboard(&self) -> Option<String> {
        for row in self.associations.order_rows.iter() {
            let r = row.product_reference.to_lowercase();
            if r.starts_with("mb/") {
                return Some(row.product_name.clone());
            }
        }
        None
    }
    
    /// Extract OS from order rows
    pub fn extract_os(&self) -> Option<String> {
        for row in self.associations.order_rows.iter() {
            let r = row.product_reference.to_lowercase();
            if r.starts_with("sw/win") {
                if r.contains("11") {
                    return Some("Windows 11".to_string());
                } else if r.contains("10") {
                    return Some("Windows 10".to_string());
                }
            }
        }
        None
    }
    
    /// Parse detail_notes from RCI order_serial
    /// Format: "Brand: DELL\r\nCPU: i7-10610U\r\nRAM: 16GB\r\nGPU: INTEGRATED\r\n..."
    /// Returns (cpu, gpu, ram, brand)
    fn parse_detail_notes(notes: &str) -> (String, String, String, String) {
        let mut cpu = String::new();
        let mut gpu = String::new();
        let mut ram = String::new();
        let mut brand = String::new();
        
        for line in notes.split(|c| c == '\n' || c == '\r') {
            let line = line.trim();
            if line.is_empty() { continue; }
            
            if let Some(value) = line.strip_prefix("CPU:") {
                cpu = value.trim().to_string();
            } else if let Some(value) = line.strip_prefix("GPU:") {
                let val = value.trim();
                // Skip "INTEGRATED" as it's not useful
                if val.to_lowercase() != "integrated" {
                    gpu = val.to_string();
                }
            } else if let Some(value) = line.strip_prefix("RAM:") {
                ram = value.trim().to_string();
            } else if let Some(value) = line.strip_prefix("Brand:") {
                brand = value.trim().to_string();
            } else if let Some(value) = line.strip_prefix("Model:") {
                // If brand is empty but model has brand info, use it
                if brand.is_empty() {
                    brand = value.trim().to_string();
                }
            }
        }
        
        (cpu, gpu, ram, brand)
    }
    
    /// Extract manufacturer/brand from product name
    /// Looks for common brand identifiers
    fn extract_manufacturer_from_name(name: &str) -> String {
        let name_upper = name.to_uppercase();
        
        // Common laptop/computer brands to look for
        let brands = [
            "DELL", "HP", "LENOVO", "ASUS", "ACER", "MSI", "GIGABYTE", 
            "RAZER", "ALIENWARE", "APPLE", "SAMSUNG", "LG", "TOSHIBA",
            "MICROSOFT", "GATEWAY", "CLEVO", "TONGFANG", "SYSTEM76",
            "FRAMEWORK", "METABOX", "SCHENKER", "XMG", "ELUKTRONICS"
        ];
        
        for brand in brands {
            if name_upper.contains(brand) {
                return brand.to_string();
            }
        }
        
        // For PC Laptops branded systems (SM-5, SM3, X-Series, etc.), use "PC LAPTOPS"
        if name_upper.starts_with("SM") || name_upper.starts_with("X-") || name_upper.starts_with("X ") 
            || name_upper.contains("XIDAX") || name_upper.contains("XENDATA") {
            return "PC LAPTOPS".to_string();
        }
        
        // If no brand found, try to get the first word as a potential brand
        if let Some(first_word) = name.split_whitespace().next() {
            // Only use if it's not a model number pattern
            if !first_word.chars().any(|c| c.is_numeric()) || first_word.len() > 3 {
                return first_word.to_uppercase();
            }
        }
        
        String::new()
    }
    
    /// Parse CPU and GPU from laptop product name
    /// Examples:
    /// - "SM-5 15" RTX 5060 Core Ultra 7 275HX" -> CPU: "Core Ultra 7 275HX", GPU: "RTX 5060"
    /// - "SM3 14" RYZEN 7 255" -> CPU: "RYZEN 7 255", GPU: ""
    fn parse_laptop_product_name(name: &str) -> (String, String) {
        let cpu = Self::extract_cpu_from_laptop_name(name);
        let gpu = Self::extract_gpu_from_laptop_name(name);
        (cpu, gpu)
    }
    
    /// Extract CPU model from laptop product name
    fn extract_cpu_from_laptop_name(name: &str) -> String {
        let name_upper = name.to_uppercase();
        
        // Check for "Core Ultra X" pattern (e.g., "Core Ultra 7 275HX")
        if let Some(idx) = name_upper.find("CORE ULTRA") {
            let after = &name[idx..];
            return Self::extract_until_resolution(after, 25);
        }
        
        // Check for "Core X" pattern without Ultra (e.g., "Core 7 250H", "Core 5 120U")
        if let Some(idx) = name_upper.find("CORE ") {
            // Make sure it's not "Core Ultra" (already handled above)
            let after = &name_upper[idx..];
            if !after.starts_with("CORE ULTRA") {
                let after_orig = &name[idx..];
                return Self::extract_until_resolution(after_orig, 20);
            }
        }
        
        // Check for "U5/U7/U9" shorthand pattern (e.g., "U9 275HX" -> "Ultra 9 275HX")
        for (short, full) in [("U9 ", "Ultra 9"), ("U7 ", "Ultra 7"), ("U5 ", "Ultra 5")] {
            if let Some(idx) = name_upper.find(short) {
                // Get the model number after UX
                let after_prefix = &name[idx + 3..];
                let model = Self::extract_until_resolution(after_prefix, 10);
                if !model.is_empty() {
                    return format!("{} {}", full, model);
                }
            }
        }
        
        // Check for RYZEN patterns (e.g., "RYZEN 7 7435HS", "RYZEN AI 7 350")
        if let Some(idx) = name_upper.find("RYZEN") {
            let after_ryzen = &name[idx..];
            return Self::extract_until_resolution(after_ryzen, 25);
        }
        
        // Check for Intel i5/i7/i9 patterns
        for prefix in ["I9 ", "I9-", "I7 ", "I7-", "I5 ", "I5-"] {
            if let Some(idx) = name_upper.find(prefix) {
                let after_prefix = &name[idx..];
                return Self::extract_until_resolution(after_prefix, 15);
            }
        }
        
        String::new()
    }
    
    /// Extract CPU/model string until we hit a resolution indicator or end
    fn extract_until_resolution(s: &str, max_len: usize) -> String {
        let end_idx = s.len().min(max_len);
        let spec = &s[..end_idx];
        
        // Split by common resolution/end markers
        let end_markers = ["1080", "2K", "4K", "FHD", "QHD", "LAPTOP", "Ready", "SOLD"];
        
        let mut result = spec.to_string();
        for marker in end_markers {
            if let Some(pos) = result.to_uppercase().find(marker) {
                result = result[..pos].to_string();
            }
        }
        
        result.trim().to_string()
    }
    
    /// Extract GPU model from laptop product name  
    fn extract_gpu_from_laptop_name(name: &str) -> String {
        let name_upper = name.to_uppercase();
        
        // Look for RTX patterns (e.g., RTX 5070, RTX 4060, RTX 3080Ti)
        if let Some(idx) = name_upper.find("RTX") {
            let after_rtx = &name[idx..];
            // Extract "RTX XXXX" or "RTX XXXXTi"
            let parts: Vec<&str> = after_rtx.split_whitespace().take(2).collect();
            if parts.len() >= 2 {
                let model = parts[1];
                // Check if next part is "Ti" suffix
                if after_rtx.to_uppercase().contains(&format!("RTX {}TI", model.to_uppercase())) {
                    return format!("RTX {}Ti", model);
                }
                return format!("RTX {}", model);
            }
        }
        
        // Look for GTX patterns
        if let Some(idx) = name_upper.find("GTX") {
            let after_gtx = &name[idx..];
            let parts: Vec<&str> = after_gtx.split_whitespace().take(2).collect();
            if parts.len() >= 2 {
                return format!("GTX {}", parts[1]);
            }
        }
        
        String::new()
    }
}
