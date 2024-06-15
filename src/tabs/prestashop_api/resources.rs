use serde::{Deserialize, Serialize};

#[derive(Serialize, Debug)]
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
    pub employees: Employees,          
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

#[derive(Serialize, Deserialize, Debug)]
pub struct Address{
    ///❌     isNullOrUnsignedId  
    pub id_customer: i32,
    ///✔️     isName  
    pub lastname: String,
    ///✔️     isName  
    pub firstname: String,
    ///✔️     isAddress   
    pub address1: String,
    ///❌     isAddress   
    pub address2: String,
    ///❌     isPostCode  
    pub postcode: String,
    ///✔️     isCityName  
    pub city: String,
    ///❌     isPhoneNumber   
    pub phone: String,
    ///❌     isPhoneNumber   
    pub phone_mobile: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Employees{
    pub employee: Vec<Employee>
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Employee{
    pub id: i32,
    /// ✔️	isName	
    pub lastname: String, 
    /// ✔️	isName	
    pub firstname: String, 
    /// ✔️	isEmail	
    pub email: String, 
    /// ❌	isBool	
    pub active: i32, 
    /// ✔️	isInt	
    pub id_profile: i32, 
    /// ❌	isUnsignedInt	
    pub id_last_order: i32, 
    /// ❌	isUnsignedInt	
    pub id_last_customer_message: i32, 
    /// ❌	isUnsignedInt	
    pub id_last_customer: i32, 
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Orders{
    orders: Vec<Order>
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Order{
    pub id_address_delivery: SubData, // ✔️
    pub id_customer: SubData, // ✔️
    pub id_cart: SubData, // ✔️
    pub invoice_number: i32, // ❌		
    pub invoice_date: String, // ❌		
    pub date_add: String, // ❌
    pub date_upd: String, // ❌
    pub id_employee_sales_rep: i32,
    pub id_employee_split_rep: i32,
    pub id_employee_editing: i32,
    pub id_order_everest: i32,
    pub id_store: i32, // 1 = warehouse
    pub total_paid: f32, // ✔️
    pub reference: String, // what prestashop sees since order id and reference are different...
    pub id_order_parent: i32, // no idea
    // #[serde(flatten)]
    pub shipping_number: Shipping, // Tracking number
    pub order_type: String, // Configurator / Sales Order
    // note: String, // ❌
    // associations: Associations
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Associations{
    pub order_rows: OrderRow
}

#[derive(Serialize, Deserialize, Debug)]
pub struct OrderRow{
    pub id: i32,
    pub id_order_config: i32,
    // pub product_id: String,
    pub product_quantity: i32,
    pub product_name: f32,
    pub product_price: String,
    // pub id_customization: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Customer { 	 	
    pub lastname: String,    //  	isCustomerName 	✔️ 	✔️ 	255
    pub firstname: String,   //  	isCustomerName 	✔️ 	✔️ 	255
    pub email: String, 	     //  	isEmail 	✔️ 	✔️ 	255
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

#[derive(Serialize, Deserialize, Debug)]
pub struct Data{
    #[serde(rename="@id")]
    pub id: Option<i32>,
    #[serde(rename="@xlink:href")]
    pub link: String
}

#[derive(Serialize, Deserialize, Debug)]
pub struct SubData{
    #[serde(rename="#text")]
    pub id: i32,
    #[serde(rename="@xlink:href")]
    pub link: String
}



#[derive(Serialize, Deserialize, Debug)]
pub struct Shipping{
    #[serde(rename="#text")]
    shipping_number: String
}