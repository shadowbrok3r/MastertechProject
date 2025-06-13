use database::schema::prestashop::{Address, Customer, Order, ServiceOrder};



pub struct PrestashopOrderForm {
    order: Order,
    customer: Customer,
    address: Address,
    service_order: ServiceOrder,
    
}