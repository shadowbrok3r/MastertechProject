use serde::{Deserialize, Serialize};
// use crate::tabs::prestashop_api::deserializer::deserialize_nested;

#[derive(Serialize, Debug)]
pub struct Resources {
    /// 	The Customer, Manufacturer and Customer addresses
    pub addresses: Addresses,          
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
pub struct Addresses{
    ///❌     isNullOrUnsignedId  
    pub id_customer: String,
    ///❌     isNullOrUnsignedId  
    pub id_manufacturer: String,
    ///❌     isNullOrUnsignedId  
    pub id_supplier: String,
    ///❌     isNullOrUnsignedId  
    pub id_warehouse: String,
    ///✔️     isUnsignedId    
    pub id_country: String,
    ///❌     isNullOrUnsignedId  
    pub id_state: String,
    ///✔️     isGenericName   
    pub alias: String,
    ///❌     isGenericName   
    pub company: String,
    ///✔️     isName  
    pub lastname: String,
    ///✔️     isName  
    pub firstname: String,
    ///❌     isGenericName   
    pub vat_number: String,
    ///✔️     isAddress   
    pub address1: String,
    ///❌     isAddress   
    pub address2: String,
    ///❌     isPostCode  
    pub postcode: String,
    ///✔️     isCityName  
    pub city: String,
    ///❌     isMessage   
    pub other: String,
    ///❌     isPhoneNumber   
    pub phone: String,
    ///❌     isPhoneNumber   
    pub phone_mobile: String,
    ///❌     isDniLite   
    pub dni: String,
    ///❌     isBool  
    pub deleted: String,
    ///❌     isDate  
    pub date_add: String,
    ///❌	  isDate
    pub date_upd: String, 
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
    /// ✔️
    id_address_delivery: String, 
    /// ✔️
    id_cart: String, 
    /// ✔️
    id_customer: String, 
    /// ❌
    current_state: String, 
    /// ✔️
    module: String, 
    /// ❌		
    invoice_number: String, 
    /// ❌		
    invoice_date: String, 
    /// ❌		
    valid: String, 
    /// ❌
    date_add: String, 
    /// ❌
    date_upd: String, 
    /// ❌
    shipping_number: String, 
    /// ❌
    note: String, 
    /// ❌
    id_shop_group: String, 
    /// ❌
    id_shop: String, 
    /// ❌
    total_discounts: String,
    /// ✔️
    total_paid: String, 
}