use serde::{Deserialize, Serialize};


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
    /// ✔️	isUnsignedInt	
    pub id_lang: String, 
    /// ❌
    pub last_passwd_gen: String, 
    /// ❌	isDate	
    pub stats_date_from: String, 
    /// ❌	isDate	
    pub stats_date_to: String, 
    /// ❌	isDate	
    pub stats_compare_from: String, 
    /// ❌	isDate	
    pub stats_compare_to: String, 
    /// ✔️	isPasswd	
    pub passwd: String, 
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
    /// ❌	isColor	
    pub bo_color: String, 
    /// ❌	isInt	
    pub default_tab: String, 
    /// ❌	isGenericName	
    pub bo_theme: String, 
    /// ❌	isGenericName	
    pub bo_css: String, 
    /// ❌	isUnsignedInt	
    pub bo_width: String, 
    /// ❌	isBool	
    pub bo_menu: String, 
    /// ❌	isUnsignedInt	
    pub stats_compare_option: String, 
    /// ❌			
    pub preselect_date_range: String, 
    /// ❌	isUnsignedInt	
    pub id_last_order: String, 
    /// ❌	isUnsignedInt	
    pub id_last_customer_message: String, 
    /// ❌	isUnsignedInt	
    pub id_last_customer: String, 
    /// ❌	isSha1	
    pub reset_password_token: String, 
    /// ❌	isDateOrNull	
    pub reset_password_validity: String, 
    /// ❌	isBool	
    pub has_enabled_gravatar: String, 
}