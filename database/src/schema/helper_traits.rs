#![allow(async_fn_in_trait)]
use super::{
    prestashop_schema::{self, Employee, Prestashop, PrestashopPayload}, ComputerData, ConnectedClient, CustomerData, Store, TaskNotePayload, TaskPayload, TicketData, TicketPayload, User, TASK_NOTE_TABLE
};
use crate::{schema::{parse_msg_date, prestashop::PrestashopId, CUSTOMER_TABLE, TASK_TABLE, TICKET_TABLE}, PlatformSpawner, Spawner, DATABASE};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{collections::HashMap, fmt::Debug};
use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};
use anyhow::{Context, Error, Result};
use async_trait::async_trait;
use surrealdb::RecordId;
use log::{debug, info};

/// Get the associated data tied to an ID
#[async_trait(?Send)]
pub trait GetAssociatedDataFromId<D> {
    async fn get_associated_data<T>(&mut self) -> Result<D, Error>
    where
        T: Serialize + for<'de> Deserialize<'de> + Clone,
        D: structdiff::StructDiff
            + DeserializeOwned
            + Serialize
            + 'static
            + Debug
            + std::marker::Unpin
            + for<'de> Deserialize<'de>;
}

/// A trait for assisting with operations involving the Employee struct
pub trait EmployeeHelper {
    /// Find a User based on Employee info -> id_employee
    async fn find_user(&mut self) -> Result<Option<User>, Error>;
    /// Pull all of my services given Employee info -> id_employee
    async fn get_my_services_in_repair(&mut self) -> Result<Vec<PrestashopId>, Error>;
    async fn get_all_my_services(&mut self) -> Result<Vec<PrestashopId>, Error>;
    /// Get all orders in my store given Employee info -> id_location
    async fn get_services_in_my_store(&mut self, start_idx: i32, offset: i32) -> Result<Vec<PrestashopId>, Error>;
    /// Get all Orders of which are my Return For Service's
    async fn get_my_return_for_services(&mut self, start_idx: i32, offset: i32) -> Result<Vec<prestashop_schema::Order>, Error>;
    /// Get all Orders of which are In the given status
    async fn get_services_by_status(&mut self, status: &str, start_idx: i32, offset: i32, id_store: &str) -> Result<Vec<PrestashopId>, Error>;
    /// Get all services in my store
    async fn get_all_services_in_my_store(&mut self, start_idx: i32, offset: i32) -> Result<Vec<PrestashopId>, Error>;
    /// Get all Return For Service's in my store
    async fn get_my_store_return_for_services(&mut self, start_idx: i32, offset: i32) -> Result<Vec<prestashop_schema::Order>, Error>;
    /// Get Employee from ID
    async fn get_employee_from_id(&mut self, id_employee: &str) -> Result<Employee, Error>;
    /// Convert an order into a PrestashopPayload
    async fn to_prestashop_payload(service_number: &str) -> Result<prestashop_schema::PrestashopPayload, Error>;
    /// get employees in store
    async fn get_employees_in_store(id_store: &str) -> Result<Vec<Employee>, Error>;
}


/// A trait for assisting with operations involving the ComputerData struct
#[async_trait(?Send)]
pub trait ComputerDataHelper {
    /// Associate a ComputerData record to a ServiceOrder
    async fn associate_to_service(&mut self) -> Result<prestashop_schema::ServiceOrder, Error>;
    /// Find TicketData associated with this Computer
    async fn find_associated_tickets(&mut self) -> Result<Vec<TicketData>, Error>;
    /// Find Clients associated with this Computer
    async fn find_associated_client(&mut self) -> Result<ConnectedClient, Error>;
    /// Find Tasks associated to this Computer
    async fn find_associated_tasks(&mut self) -> Result<Vec<TaskPayload>, Error>;
    /// Find Customer that owns this Computer
    async fn find_associated_customer(&mut self) -> Result<CustomerData, Error>;
    /// Find PrestaShop Orders associated with this Computer
    async fn find_associated_prestashop_orders(
        &mut self,
    ) -> Result<Vec<prestashop_schema::Order>, Error>;
    /// Find PrestaShop Customer associated with this Computer
    async fn find_prestashop_customer(&mut self) -> Result<prestashop_schema::Customer, Error>;
}

/// A trait for assisting with operations involving orders.
#[async_trait(?Send)]
pub trait OrderHelper {
    /// Converts an order to a task payload structure.
    ///
    /// # Returns
    /// - `Ok(TaskPayload)` on success.
    /// - `Err(Error)` if an error occurs during conversion.
    async fn convert_to_task_payload(&mut self) -> Result<TaskPayload, Error>;

    /// Converts an order to a ticket payload structure.
    ///
    /// # Returns
    /// - `Ok(TicketPayload)` on success.
    /// - `Err(Error)` if an error occurs during conversion.
    async fn convert_to_ticket_payload(&mut self) -> Result<TicketPayload, Error>;

    /// Retrieves all return orders associated with services.
    ///
    /// # Returns
    /// - `Ok(Vec<Order>)` containing a list of return orders.
    /// - `Err(Error)` if an error occurs during retrieval.
    async fn get_all_return_for_services(&mut self)
        -> Result<Vec<prestashop_schema::Order>, Error>;
}

impl EmployeeHelper for Employee {
    async fn find_user(&mut self) -> Result<Option<User>, Error> {
        log::warn!("EmployeeHelper -> find_user");
        DATABASE.set("email", self.email.clone()).await?;
        let usr: Option<User> = DATABASE
            .query("SELECT * FROM user WHERE email == $email")
            .await?
            .take(0)?;
        if usr.is_none() {
            log::warn!("EmployeeHelper -> find_user -> Could not find user");
        }
        debug!("user: {:?}", usr);
        Ok(usr)
    }

    async fn get_employee_from_id(&mut self, id_employee: &str) -> Result<Employee, Error> {
        let api_call = Prestashop::default();

        if id_employee != "0" && id_employee != "" {
            let employee_res: anyhow::Result<Employee, anyhow::Error> = api_call
                .request_subresources_by_id_wasm("employees", "employee", &id_employee)
                .await;

            match employee_res {
                Ok(employee) => Ok(Employee {
                    email: employee.email.clone(),
                    initials: employee.initials.clone(),
                    firstname: employee.firstname.clone(),
                    id_store: employee.id_store.clone(),
                    lastname: employee.lastname.clone(),
                    id: employee.id.clone(),
                    ..Default::default() // ..self.clone()
                }),
                Err(e) => { return Err(anyhow::anyhow!("Error getting employee: {e:?}")); }
            }
        } else if !self.email.is_empty() {
            Ok(User::default().set_email(&self.email).find_employee_by_email().await?)
        } else {
            Ok(Employee::default())
        }
    }

    async fn get_my_return_for_services(&mut self, start_idx: i32, offset: i32) -> Result<Vec<prestashop_schema::Order>, Error> {
        let mut api_call = Prestashop::default();
        api_call.display = "";
        let mut query: HashMap<&str, &str> = HashMap::new();
        let pagination = format!("{},{}",start_idx.clone(), offset);
        query.insert("filter[product_reference]", "SRVC/RETURN");
        query.insert("sort", "[id_DESC]");
        query.insert("limit", &pagination);
        query.insert("output_format", "JSON");

        let order_details: Vec<prestashop_schema::OrderDetails> = api_call
            .request_resources_wasm("order_details", query.clone())
            .await?;

        let mut orders_vec = Vec::new();

        for order in order_details.iter() {
            let order_details: prestashop_schema::OrderDetails = api_call
                .request_subresources_by_id_wasm("order_details", "order_detail", &order.id)
                .await?;

            if let Some(id) = order_details.id_order {
                let mut order_query: HashMap<&str, &str> = HashMap::new();

                order_query.insert("sort", "[id_DESC]");
                order_query.insert("output_format", "JSON");

                let order: prestashop_schema::Order = api_call
                    .request_subresources_by_id_wasm("orders", "order", &id)
                    .await?;

                orders_vec.push(order);
            }
        }

        Ok(orders_vec)
    }

    async fn get_my_store_return_for_services(&mut self, start_idx: i32, offset: i32) -> Result<Vec<prestashop_schema::Order>, Error> {
        let api_call = Prestashop::default();
        let mut query: HashMap<&str, &str> = HashMap::new();
        let pagination = format!("{},{}",start_idx.clone(), offset);
        query.insert("filter[product_reference]", "SRVC/RETURN");
        query.insert("sort", "[id_DESC]");
        query.insert("limit", &pagination);
        query.insert("output_format", "JSON");

        let order_details: Vec<prestashop_schema::OrderDetails> = api_call
            .request_resources_wasm("order_details", query.clone())
            .await?;

        let mut orders_vec = Vec::new();

        for order in order_details.iter() {
            let order_details: prestashop_schema::OrderDetails = api_call
                .request_subresources_by_id_wasm("order_details", "order_detail", &order.id)
                .await?;

            if let Some(id) = order_details.id_order {
                let mut order_query: HashMap<&str, &str> = HashMap::new();

                order_query.insert("sort", "[id_DESC]");
                order_query.insert("output_format", "JSON");
                order_query.insert("filter[id_store]", &mut self.id_store);
                order_query.insert("filter[id_order_type]", "2");

                let order: prestashop_schema::Order = api_call
                    .request_subresources_by_id_wasm("orders", "order", &id)
                    .await?;

                orders_vec.push(order);
            }
        }

        Ok(orders_vec)
    }

    async fn get_my_services_in_repair(&mut self) -> Result<Vec<PrestashopId>, Error> {
        let mut api_call = Prestashop::default();
        let mut query: HashMap<&str, &str> = HashMap::new();
        query.insert("filter[id_employee_sales_rep]", &self.id);
        query.insert("filter[id_store]", &self.id_store);
        query.insert("filter[id_order_type]", "2");
        query.insert("filter[current_state]", "30");
        query.insert("sort", "[id_DESC]");
        query.insert("output_format", "JSON");
        api_call.display = "[id]";

        let orders: Vec<PrestashopId> = api_call
            .request_resources_wasm("orders", query.clone())
            .await?;
        info!("helper_traits -> Orders list: {orders:?}");
        Ok(orders)
    }

    async fn get_all_my_services(&mut self) -> Result<Vec<PrestashopId>, Error> {
        let mut api_call = Prestashop::default();
        let mut query: HashMap<&str, &str> = HashMap::new();
        query.insert("filter[id_employee_sales_rep]", &self.id);
        query.insert("filter[id_store]", &self.id_store);
        query.insert("filter[id_order_type]", "2");
        query.insert("filter[current_state]", "30");
        query.insert("sort", "[id_DESC]");
        query.insert("limit", "20");
        query.insert("output_format", "JSON");
        api_call.display = "[id]";

        let orders: Vec<PrestashopId> = api_call
            .request_resources_wasm("orders", query.clone())
            .await?;
        info!("helper_traits -> Orders list: {orders:?}");
        Ok(orders)
    }

    async fn get_services_in_my_store(&mut self, start_idx: i32, offset: i32) -> Result<Vec<PrestashopId>, Error> {
        let mut api_call = Prestashop::default();
        let mut query: HashMap<&str, &str> = HashMap::new();
        let pagination = format!("{},{}",start_idx.clone(), offset);
        query.insert("filter[id_store]", &self.id_store);
        query.insert("filter[id_order_type]", "2");
        query.insert("sort", "[id_DESC]");
        query.insert("limit", &pagination);
        query.insert("output_format", "JSON");
        api_call.display = "[id]";

        let orders: Vec<PrestashopId> = api_call
            .request_resources_wasm("orders", query.clone())
            .await?;
        Ok(orders)
    }

    async fn get_all_services_in_my_store(&mut self, start_idx: i32, offset: i32) -> Result<Vec<PrestashopId>, Error> {
        let mut api_call = Prestashop::default();
        let mut query: HashMap<&str, &str> = HashMap::new();
        let pagination = format!("{},{}",start_idx.clone(), offset);
        query.insert("filter[id_store]", &self.id_store);
        query.insert("filter[id_order_type]", "2");
        query.insert("sort", "[id_DESC]");
        query.insert("limit", &pagination);
        query.insert("output_format", "JSON");
        api_call.display = "[id]";

        let orders: Vec<PrestashopId> = api_call
            .request_resources_wasm("orders", query.clone())
            .await?;
        Ok(orders)
    }
 
    async fn get_services_by_status(&mut self, status: &str, start_idx: i32, offset: i32, id_store: &str) -> Result<Vec<PrestashopId>, Error> {
        let mut api_call = Prestashop::default();
        let mut query: HashMap<&str, &str> = HashMap::new();
        let pagination = format!("{},{}",start_idx.clone(), offset);

        info!("helper_traits -> Pagination: {pagination}");

        query.insert("filter[id_store]", id_store);
        query.insert("filter[id_order_type]", "2");
        query.insert("filter[current_state]", status);
        query.insert("sort", "[id_DESC]");
        query.insert("limit", &pagination);
        query.insert("output_format", "JSON");
        api_call.display = "[id]";

        let orders: Vec<PrestashopId> = api_call
            .request_resources_wasm("orders", query.clone())
            .await.context("Pulling orders list")?;

        info!("helper_traits -> Orders list: {orders:?}");
        Ok(orders)
    }

    async fn to_prestashop_payload(service_number: &str) -> Result<prestashop_schema::PrestashopPayload, Error> {
        let mut api_call = Prestashop::default();
        let mut query = HashMap::new();
        let task_notes = &mut vec![];
        info!("helper_traits -> Pulling order {service_number}");
        query.insert("filter[id]", service_number);
        query.insert("output_format", "JSON");
        // api_call.display = "[id,id_address_invoice,id_customer,current_state,date_add,id_employee_sales_rep,id_employee_split_rep,id_store,associations]";
    
        let customer_threads: Vec<prestashop_schema::CustomerThread> = api_call
            .request_resources_wasm("customer_threads", query.clone())
            .await?;
    
        let mut customer_messages: Vec<prestashop_schema::CustomerMessage> = Vec::new();
    
        if !customer_threads.is_empty() {
            for thread in customer_threads.iter() {
                for msg in thread.associations.customer_messages.iter() {
                    let msg: prestashop_schema::CustomerMessage =  api_call
                        .request_subresources_by_id_wasm(
                            "customer_messages",
                            "customer_message",
                            msg.id.as_str(),
                        )
                        .await?;
                    task_notes.push(msg.into_task_note(service_number).await?);
                    customer_messages.push(msg);
                }
            }
        }
    
        let order: prestashop_schema::Order = api_call
            .find_resource_wasm("orders", query.clone())
            .await.context("Pulling order")?;

        api_call.display = "full";
        if order.id_customer.is_empty() 
        {
            return Err(anyhow::anyhow!("order.id_customer is empty")).into();
        }

        api_call.display = "[id,id_store,lastname,firstname,email,initials]";

        let sales_rep: Option<Employee>  = if !order.id_employee_sales_rep.eq("checkinshelf") && !order.id_employee_sales_rep.eq("0"){
            let mut new_query = query.clone();
            new_query.clear();
            new_query.insert("filter[id]", &order.id_employee_sales_rep);
            new_query.insert("output_format", "JSON");
            Some(
                api_call
                .find_resource_wasm(
                    "employees",
                    new_query
                )
                .await.context("Pulling employee")?
            )
        } else {
            let mut emp = Employee::default();
            emp.firstname = "CheckInShelf".to_string();
            Some(emp)
        };

        let split_rep: Option<Employee> = if !order.id_employee_split_rep.eq("0") {
            let mut new_query = query.clone();
            new_query.clear();
            new_query.insert("filter[id]", &order.id_employee_split_rep);
            new_query.insert("output_format", "JSON");
            let employee_2: Employee = api_call
                .find_resource_wasm(
                    "employees",
                    new_query
                )
                .await
                .context("Pulling split rep")?;

            info!("helper_traits -> employee: {sales_rep:#?}");
            Some(employee_2)
        } else {
            None
        };

        let cust: prestashop_schema::Customer = if order.id_employee_sales_rep.eq("0") {
            let mut cust = prestashop_schema::Customer::default();
            cust.firstname = "Checkin".to_string();
            cust.lastname = "Shelf".to_string();
            cust
        } else {
            api_call.display = "[lastname,firstname,email]";
            let mut new_query = query.clone();
            new_query.clear();
            new_query.insert("filter[id]", &order.id_customer);
            new_query.insert("output_format", "JSON");
            api_call
                .find_resource_wasm(
                    "customers", 
                    new_query
                )
                .await.context("Pulling customer")?
        };
        
        api_call.display = "full";
        let address: prestashop_schema::Address = api_call
            .request_subresources_by_id_wasm("addresses", "address", &order.id_address_invoice)
            .await?;

        let customer = CustomerData {
            id: RecordId::from((
                CUSTOMER_TABLE.to_string(),
                order.id_customer.clone(),
            )),
            cust_code: order.id_customer.clone(),
            name: format!("{} {}", &cust.firstname, &cust.lastname),
            phone_number: address.phone.clone().to_string(),
            email: cust.email,
            ..Default::default()
        };

        Ok(
            prestashop_schema::PrestashopPayload {
                customer,
                order,
                sales_rep,
                split_rep,
                customer_threads,
                customer_messages,
                task_notes: task_notes.clone(),
                address
            }
        )
    }

    async fn get_employees_in_store(id_store: &str) -> Result<Vec<Employee>, Error> {
        let api_call = Prestashop::default();
        let mut query: HashMap<&str, &str> = HashMap::new();
        query.insert("filter[id_store]", id_store);
        query.insert("filter[active]", "1");

        let employees: Vec<PrestashopId> = api_call
            .request_resources_wasm("employees", query.clone())
            .await?;

        let mut employees_vec = Vec::new();

        for employee in employees.iter() {
            let employee: prestashop_schema::Employee = api_call
                .request_subresources_by_id_wasm("employees", "employee", &employee.id)
                .await?;
            
            employees_vec.push(employee);
        }

        Ok(employees_vec)
    }
}

#[async_trait(?Send)]
impl ComputerDataHelper for ComputerData {
    async fn associate_to_service(&mut self) -> Result<prestashop_schema::ServiceOrder, Error> {
        todo!()
    }

    async fn find_associated_tickets(&mut self) -> Result<Vec<TicketData>, Error> {
        todo!()
    }

    async fn find_associated_client(&mut self) -> Result<ConnectedClient, Error> {
        todo!()
    }

    async fn find_associated_tasks(&mut self) -> Result<Vec<TaskPayload>, Error> {
        todo!()
    }

    async fn find_associated_customer(&mut self) -> Result<CustomerData, Error> {
        DATABASE
            .set("cust_id", self.customer.clone().unwrap())
            .await?;
        let customer: Option<CustomerData> = DATABASE
            .query("SELECT * FROM customer WHERE id == $cust_id")
            .await?
            .take(0)?;
        debug!("customer: {:?}", customer);
        Ok(customer.unwrap())
    }

    async fn find_associated_prestashop_orders(
        &mut self,
    ) -> Result<Vec<prestashop_schema::Order>, Error> {
        todo!()
    }

    async fn find_prestashop_customer(&mut self) -> Result<prestashop_schema::Customer, Error> {
        let cust = self.find_associated_customer().await?;
        let api_call = Prestashop::default();
        let mut query: HashMap<&str, &str> = HashMap::new();

        query.insert("filter[email]", &cust.email);
        query.insert("output_format", "JSON");

        let customer: prestashop_schema::Customer = api_call
            .find_resource_wasm("customers", query.clone())
            .await?;
        Ok(customer)
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PrestaResourceResponse {
    pub date_add: String,
    pub id: String,
    pub date_upd: String,
    // pub service_number: String,
}

#[async_trait(?Send)]
pub trait PrestashopPayloadHelper<'a>: Send + Sync  {
    async fn get_prestashop_payload(&mut self, service_number: &str) -> Result<PrestashopPayload, Error>;
    async fn get_customer_threads(&mut self, service_number: &str, prestashop_api: &Prestashop) -> Result<(), Error>;
    async fn get_customer_messages(&mut self, prestashop_api: &Prestashop) -> Result<(), Error>;
    async fn get_order(&mut self, service_number: &str, prestashop_api: &Prestashop) -> Result<(), Error>;
    async fn get_employee(&mut self, prestashop_api: &Prestashop) -> Result<(), Error>;
    async fn get_customer(&mut self, prestashop_api: &Prestashop) -> Result<(), Error>;
}

#[async_trait(?Send)]
impl <'a>PrestashopPayloadHelper<'a> for PrestashopPayload {
    async fn get_prestashop_payload(&mut self, service_number: &str) -> Result<Self, Error> {
        let prestashop_api = Prestashop::default();
        self.get_customer_threads(&service_number, &prestashop_api).await?;
        self.get_customer_messages(&prestashop_api).await?;
        self.get_order(&service_number, &prestashop_api).await?;
        self.get_employee(&prestashop_api).await?;
        self.get_customer(&prestashop_api).await?;

        Ok(self.clone())
    }
    async fn get_customer_threads(&mut self, service_number: &str, prestashop_api: &Prestashop) -> Result<(), Error> {
        if !self.customer_threads.is_empty() {
            let mut query = HashMap::new();
            query.insert("filter[id_order]", service_number);
            query.insert("output_format", "JSON");
            self.customer_threads = prestashop_api
                .request_resources_wasm("customer_threads", query.clone())
                .await?;
        }
        Ok(())
    }
    async fn get_customer_messages(&mut self, prestashop_api: &Prestashop) -> Result<(), Error> {
        for thread in self.customer_threads.iter() {
            for msg in thread.associations.customer_messages.iter() {
                let msg =  prestashop_api
                    .request_subresources_by_id_wasm(
                        "customer_messages",
                        "customer_message",
                        &msg.id,
                    )
                    .await?;
                self.customer_messages.push(msg)
            }
        }
        Ok(())
    }
    async fn get_order(&mut self, service_number: &str, prestashop_api: &Prestashop) -> Result<(), Error> {
        self.order = prestashop_api.request_subresources_by_id_wasm("orders", "order", service_number).await?;
        Ok(())
    }
    async fn get_employee(&mut self, prestashop_api: &Prestashop) -> Result<(), Error> {
        let sales_rep: Option<Employee> = if !self.order.id_employee_sales_rep.eq("checkinshelf") && !self.order.id_employee_sales_rep.eq("0") {
            //|| order.id_employee_sales_rep.len() != 0{
            let employee: Employee = prestashop_api
                .request_subresources_by_id_wasm(
                    "employees",
                    "employee",
                    &self.order.id_employee_sales_rep,
                )
                .await?;
    
            info!("helper_traits -> employee: {employee:#?}");
            Some(employee)
        } else {
            None
        };
    
        self.sales_rep = sales_rep;

        let split_rep: Option<Employee> = if !self.order.id_employee_split_rep.eq("0") {
            let employee_2: Employee = prestashop_api
                .request_subresources_by_id_wasm(
                    "employees",
                    "employee",
                    &self.order.id_employee_split_rep,
                )
                .await?;
    
            info!("helper_traits -> employee: {employee_2:#?}");
            Some(employee_2)
        } else {
            None
        };
        self.split_rep = split_rep;

        Ok(())
    }
    async fn get_customer(&mut self, prestashop_api: &Prestashop) -> Result<(), Error> {
        let cust: prestashop_schema::Customer = if self.order.id_employee_sales_rep.eq("0") {
            let mut cust = prestashop_schema::Customer::default();
            cust.firstname = "Checkin".to_string();
            cust.lastname = "Shelf".to_string();
            cust
        } else {
            prestashop_api
                .request_subresources_by_id_wasm("customers", "customer", &self.order.id_customer)
                .await.context("Pulling customer")?
        };

        let address: prestashop_schema::Address = prestashop_api
            .request_subresources_by_id_wasm("addresses", "address", &self.order.id_address_invoice)
            .await?;

        self.customer = CustomerData {
            id: RecordId::from((
                CUSTOMER_TABLE.to_string(),
                self.order.id_customer.clone(),
            )),
            cust_code: self.order.id_customer.clone(),
            name: format!("{} {}", &cust.firstname, &cust.lastname),
            phone_number: address.phone.clone().to_string(),
            // phone_number_2: address.phone_mobile.clone().unwrap_or(0).to_string(),
            email: cust.email,
            ..Default::default()
        };

        Ok(())
    }
}


impl From<TaskPayload> for PrestashopPayload {
    fn from(_value: TaskPayload) -> Self {
        todo!()
    }
}

impl From<PrestashopPayload> for TaskPayload {
    fn from(value: PrestashopPayload) -> Self {
        let customer = &mut CustomerData::default();
        let ticket = &mut TicketPayload::default();
        let task = &mut TaskPayload::default();
        let mut task_notes = Vec::new();

        let service_details = value.order.associations.order_service.clone();
        let mut services: Vec<RecordId> = Vec::new();

        let sales_rep = value.sales_rep.clone().unwrap_or_default();
        let split_rep = value.split_rep.clone().unwrap_or_default();
        let email = parse_email_user(&sales_rep.email);
        let email_split_rep = parse_email_user(&split_rep.email);

        customer.id = value.customer.id.clone();
        customer.cust_code = value.customer.cust_code.clone();
        customer.email = value.customer.email.clone();
        customer.name = value.customer.name.clone();
        customer.phone_number = value.customer.phone_number.clone();
        ticket.salesman = email_split_rep.to_string();
        ticket.sales_rep = email.to_string();
        ticket.tech = email.to_string();
        info!(
            "Salesman: {:?}\nTech: {:?}",
            ticket.salesman.clone(),
            ticket.tech.clone()
        );
        ticket.customer = Some(customer.clone());
        ticket.checkin_rep = email.to_string();
        ticket.terms = value.order.payment.clone();
        ticket.ticket_total = value.order.total_products_wt.clone();
        ticket.doc_alias = value.order.order_type.clone();
        ticket.service_number = value.order.id.clone();
        ticket.id = RecordId::from((
            TICKET_TABLE.to_string(),
            ticket.service_number.clone(),
        ));
        task.id = RecordId::from((
            TASK_TABLE.to_string(),
            ticket.service_number.clone(),
        ));

        let (tx, rx) = crossbeam::channel::bounded::<User>(1);
        
        let employees = value
            .customer_messages
            .iter()
            .map(|msg| msg.id_employee.clone())
            .collect::<Vec<String>>();

        PlatformSpawner::spawn(async move {
            let res = async {
                for emp in employees.iter() {
                    let employee = Employee::default().get_employee_from_id(emp).await?;
                    let emp = User::default().set_email(&employee.email).find_employee_by_email().await?;
                    tx.try_send(User::query_user_from_email(emp.email.clone()).await?)?;
                }
                
                Ok::<(), anyhow::Error>(())
            }.await;
            log::info!("Res: {res:?}");
        });

        let user = &mut User::default();

        if let Ok(usr) = rx.try_recv() { 
            *user = usr;
        }

        for msg in value.customer_messages.iter() {
            task_notes.push(TaskNotePayload {
                note: msg.message.clone(),
                id: RecordId::from((TASK_NOTE_TABLE, msg.id.clone())),
                task_id: Some(task.id.clone()),
                created_at: if let Ok(date) = DateTime::parse_from_rfc3339(&msg.date_add) {
                    date.with_timezone(&Utc).into()
                } else {
                    parse_msg_date(&msg.date_add).unwrap_or(Utc::now().into())
                },
                id_customer_thread: Some(msg.id_customer_thread.clone()),
                id_customer_message: Some(msg.id.clone()),
                id_employee: Some(msg.id_employee.clone()),
                username: user.get_username().to_string(),
                user: user.get_id().clone(),
                service_number: Some(ticket.service_number.clone()),
                private: false
                // ..Default::default()
            })
        }
        task.task_note = task_notes;
        services.push(ticket.id.clone());
        
        if !service_details.is_empty() {
            if service_details.len() == 1 {
                let svc = service_details.get(0);
                if let Some(service) = svc {
                    ticket.checkin_notes = service.check_in_notes.clone();
                }
            } else {
                info!("helper_traits -> Theres a couple.... {:?}", service_details);
            }
        }

        task.service_ticket = Some(ticket.clone());

        task.task_name = format!(
            "{} - {}",
            &customer.name,
            ticket.service_number.clone()
        );
        task.clone()
    }
}

impl From<u64> for Store {
    fn from(value: u64) -> Self {
        match value {
            76 => Store::RIV,
            73 => Store::LTN,
            74 => Store::MUR,
            78 => Store::WJ,
            75 => Store::ORE,
            72 => Store::AF,
            77 => Store::SAN,
            _ => Store::RIV,
        }
    }
}

impl From<Store> for u64 {
    fn from(value: Store) -> Self {
        match value {
            Store::RIV => 76,
            Store::LTN => 73,
            Store::MUR => 74,
            Store::WJ => 78,
            Store::ORE => 75,
            Store::AF => 72,
            Store::SAN => 77
        }
    }
}

pub fn convert_date_string(input: &str) -> Result<String, chrono::ParseError> {
    // Define the input format as per the provided string.
    let format = "%Y-%m-%d %H:%M:%S";

    // Parse the input string into a NaiveDateTime (which doesn't include timezone information).
    let naive_dt = NaiveDateTime::parse_from_str(input, format)?;

    // Convert the NaiveDateTime to a DateTime<Utc> with the assumption that it is in UTC.
    let datetime_utc = Utc.from_utc_datetime(&naive_dt);

    // Format the DateTime<Utc> to the desired ISO 8601 string with milliseconds.
    let result = datetime_utc.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);

    Ok(result)
}

/// Parses the username from an email address
pub fn parse_email_user(email: &str) -> &str {
    email.split('@').next().unwrap_or(email)
}
