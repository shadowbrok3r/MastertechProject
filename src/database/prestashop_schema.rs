use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct Address{
    pub id: i32,
    pub id_customer: String,         // ❌     isNullOrUnsignedId  
    pub lastname: String,             // ✔️     isName  
    pub firstname: String,            // ✔️     isName  
    pub address1: String,             // ✔️     isAddress   
    pub address2: String,             // ❌     isAddress   
    pub postcode: String,                // ❌     isPostCode  
    pub city: String,                 // ✔️     isCityName  
    pub phone: String,                   // ❌     isPhoneNumber   
    pub phone_mobile: String, // ❌     isPhoneNumber   
}

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct Employee{
    pub id: i32,
    /// ✔️	isName	
    pub lastname: String, 
    /// ✔️	isName	
    pub firstname: String, 
    /// ✔️	isEmail	
    pub email: String, 
    /// ❌	isBool	
    pub active: String, 
    /// ✔️	isInt	
    pub id_profile: String, 
    /// ❌	isUnsignedInt	
    pub id_last_order: String, 
    /// ❌	isUnsignedInt	
    pub id_last_customer_message: String, 
    /// ❌	isUnsignedInt	
    pub id_last_customer: String, 
}


#[derive(Serialize, Deserialize, Debug, Default)]
pub struct Order{
    pub order_type_name: String,
    pub id_address_delivery: String, // ✔️
    pub id_customer: String, // ✔️
    pub id_cart: String, // ✔️
    pub invoice_number: String, // ❌		
    pub invoice_date: String, // ❌		
    pub date_add: String, // ❌
    pub date_upd: String, // ❌
    pub id_employee_sales_rep: String,
    pub id_employee_split_rep: String,
    pub id_employee_editing: String,
    pub id_order_everest: String,
    pub id_store: String, // 1 = warehouse
    pub total_paid: String, // ✔️
    pub reference: String, // what prestashop sees since order id and reference are different...
    pub id_order_parent: String, // no idea
    // #[serde(flatten)]
    pub shipping_number: String, // Tracking number
    pub order_type: String, // Configurator / Sales Order
    // note: String, // ❌
    pub associations: Associations
}

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct Associations{
    pub order_rows: Vec<OrderRow>,
    pub order_service: Option<Vec<ServiceOrder>>
}

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct OrderRow{
    pub id: String,
    pub id_order_config: String,
    pub product_id: String,
    pub product_quantity: String,
    pub product_name: String,
    pub product_price: String,
}


#[derive(Serialize, Deserialize, Debug, Default)]
pub struct Customer {
    pub lastname: String,    //  	isCustomerName 	✔️ 	✔️ 	255
    pub firstname: String,   //  	isCustomerName 	✔️ 	✔️ 	255
    pub email: String, 	     //  	isEmail 	✔️ 	✔️ 	255
}

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct CustomerMessage { 	 	
    pub id_employee: String,        //  isUnsignedId   ❌ 		Employee ID
    pub id_customer_thread: String, //	               ❌ 		Customer Thread ID
    pub ip_address: String,         //  isIp2Long      ❌ 	    15 	
    pub message: String,            //  isCleanHtml    ✔️ 	     16777216 	
    pub file_name: String,          //		           ❌ 		
    pub user_agent: String,         //	               ❌ 		
    pub private: String,            //  isBool 	       ❌ 		
    pub date_add: String,           // 	isDate 	       ❌ 		
    pub date_upd: String,           // 	isDate 	       ❌ 		
    pub read: String,           
}

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct CustomerThread { 	
    pub id: i32,     
    pub id_customer: String,    // isUnsignedId 	❌ 		Customer ID
    pub id_order: String,    	// isUnsignedId 	❌ 		Order ID
    pub date_add: String,    	// isDate 	        ❌ 		
    pub date_upd: String,    	// isDate 	        ❌ 		
    pub associations: CustMessageAssociation,    	     
}

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct CustMessageAssociation{
    pub customer_messages: Vec<CustMessage>
}

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct CustMessage{
    pub id: String
}

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct ServiceOrder{
    // pub id: i32,
    pub id_order_service: String,
    pub id_cart: String,
    pub id_order: String,
    pub device_name: String,
    pub device_mfg: String,
    pub device_model: String,
    pub device_serial: String,
    pub device_password: String,
    pub id_status_service: String,
    pub device_power_supply: String,
    pub other_hardware_software: String,
    pub physical_damage: String,
    pub check_in_notes: String,
    pub intake_notes: String,
    pub id_employee_qc_tech: String,
    pub id_employee_qc_signoff: String,
}

#[derive(Serialize, Debug, Default)]
pub struct Resources {
    /// 	The Customer, Manufacturer and Customer addresses
    pub addresses: Address,          
    /// 	The product Attachments
    pub attachments: String,            
    /// 	The product Attachments files
    pub attachments_file: String,            
    /// 	Customer’s carts
    pub carts: String,          
    /// 	The product categories
    pub categories: String,             
    /// 	The product combinations
    pub combinations: String,
    /// 	Customer services messages
    pub customer_messages: String,          
    /// 	Customer services threads
    pub customer_threads: String,           
    /// 	The e-shop’s customers
    pub customers: String,
    /// 	The Employees
    pub employees: Employee,          
    /// 	The guests (customers not logged in)
    pub guests: String,             
    /// 	The product manufacturers
    pub manufacturers: String,          
    /// 	The customers messages
    pub messages: String,           
    /// 	The order carriers
    pub order_carriers: String,             
    /// 	Details of an order
    pub order_details: String,          
    /// 	The Order histories
    pub order_histories: String,            
    /// 	The Order invoices
    pub order_invoices: String,             
    /// 	The Order payments
    pub order_payments: String,   
    /// 	The Order states (Waiting for transfer, Payment accepted, …)
    pub order_states: String,           
    /// 	The Customers orders
    pub orders: String,    
    /// 	The Product customization fields
    pub product_customization_fields: String,           
    /// 	The product feature values (Ceramic, Polyester, … - Removable cover, Short sleeves, …)
    pub product_feature_values: String,             
    /// 	The product features (Composition, Property, …)
    pub product_features: String,           
    /// 	The product options value (S, M, L, … - White, Camel, …)
    pub product_option_values: String,          
    /// 	The product options (Size, Color, …)
    pub product_options: String,            
    /// 	Product Suppliers
    pub product_suppliers: String,          
    /// 	The products
    pub products: String,           
    /// 	Search
    pub search: String,             
    /// 	Available quantities of products
    pub stock_availables: String,  
    /// 	Stocks for products
    pub stocks: String,             
    /// 	The stores
    pub stores: String,
    /// 	The Products tags
    pub tags: String,           
}

pub trait SubResource {
    fn get_subresource(&self, field: &str) -> Option<String>;
    fn get_name(&self) -> String;
    fn get_resource_name(&self) -> String;
}

impl SubResource for Employee {
    fn get_subresource(&self, field: &str) -> Option<String> {
        match field {
            "id" => Some(self.id.to_string()),
            "lastname" => Some(self.lastname.clone()),
            "firstname" => Some(self.firstname.clone()),
            "email" => Some(self.email.clone()),
            "active" => Some(self.active.to_string()),
            "id_profile" => Some(self.id_profile.to_string()),
            "id_last_order" => Some(self.id_last_order.to_string()),
            "id_last_customer_message" => Some(self.id_last_customer_message.to_string()),
            "id_last_customer" => Some(self.id_last_customer.to_string()),
            _ => None,
        }
    }

    fn get_resource_name(&self) -> String {
        "employees".to_string()
    }

    fn get_name(&self) -> String {
        "employee".to_string()
    }
}



// impl SubResource for Order{
//     fn get_subresource(&self, field: &str) -> Option<String> {
//         match field {
//             "id_address_delivery" => Some(self.id_address_delivery.to_string()),
//             "id_cart" => Some(self.id_cart.to_string()),
//             // "id_customer" => Some(self.id_customer.to_string()),
//             // "current_state" => Some(self.current_state.to_string()),
//             // "module" => Some(self.module.to_string()),
//             // "invoice_number" => Some(self.invoice_number.to_string()),
//             // "invoice_date" => Some(self.invoice_date.to_string()),
//             // "date_add" => Some(self.date_add.to_string()),
//             // "date_upd" => Some(self.date_upd.to_string()),
//             // "shipping_number" => Some(self.shipping_number.to_string()),
//             // "note" => Some(self.note.to_string()),
//             // "total_paid" => Some(self.total_paid.to_string()),
//             _ => None,
//         }
//     }
// }
