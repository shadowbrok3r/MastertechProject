use crate::DATABASE;

use super::{
    prestashop_schema::{self, Employee, Prestashop},
    ComputerData, ConnectedClient, CustomerData, ExtendedSeb, SpecialPartOrder, TaskPayload,
    TicketData, TicketPayload, User,
};
use anyhow::{Error, Result};
use async_trait::async_trait;
use log::debug;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{collections::HashMap, fmt::Debug};

/// Macro to implement GetDataFromId for structs with an 'id' field
macro_rules! _get_id {
    ($struct_name:ident) => {
        #[async_trait]
        impl GetDataFromId for $struct_name {
            async fn get_id(&self) -> &RecordId {
                &self.id
            }
        }
    };
}

/// A trait for assisting with operations involving the Employee struct
#[async_trait]
pub trait EmployeeHelper {
    /// Find a User based on Employee info -> id_employee
    async fn find_user(&mut self) -> Result<User, Error>;
    /// Pull all of my services given Employee info -> id_employee
    async fn get_my_services(&mut self) -> Result<Vec<prestashop_schema::Order>, Error>;
    /// Get all orders in my store given Employee info -> id_location
    async fn get_services_in_my_store(&mut self) -> Result<Vec<prestashop_schema::Order>, Error>;
}

/// A trait for assisting with operations involving the User struct
#[async_trait]
pub trait UserHelper {
    /// Get Employee record from User info
    async fn find_employee(&mut self) -> Result<prestashop_schema::Employee, Error>;
}

/// A trait for assisting with operations involving the ComputerData struct
#[async_trait]
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

/// A trait for assisting with operations involving Customer Records
#[async_trait]
pub trait CustomerHelper {
    async fn find_associated_addr(&mut self) -> Result<prestashop_schema::Address, Error>;
}

#[async_trait]
pub trait TaskNotePayloadHelper {}

#[async_trait]
pub trait CustomerThreadHelper {
    async fn create_task_note_payload(&mut self) -> Result<(), Error>;
}
#[async_trait]
pub trait CustomerDataHelper {
    async fn find_part_orders(&mut self) -> Result<Vec<SpecialPartOrder>, Error>;
    async fn find_prestashop_customer(&mut self) -> Result<prestashop_schema::Customer, Error>;
    async fn get_seb_data(&mut self) -> Result<ExtendedSeb, Error>;
}

#[async_trait]
pub trait OrderHelper {
    async fn convert_to_task_payload(&mut self) -> Result<TaskPayload, Error>;
    async fn convert_to_ticket_payload(&mut self) -> Result<TicketPayload, Error>;
}

#[async_trait]
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

#[async_trait]
impl EmployeeHelper for Employee {
    async fn find_user(&mut self) -> Result<User, Error> {
        DATABASE.set("email", self.email.clone()).await?;
        let usr: Option<User> = DATABASE
            .query("SELECT * FROM user WHERE email == $email")
            .await?
            .take(0)?;
        debug!("user: {:?}", usr);
        let user = usr.unwrap_or_default();
        Ok(user)
    }

    async fn get_my_services(&mut self) -> Result<Vec<prestashop_schema::Order>, Error> {
        let api_call = Prestashop::default();
        let mut query: HashMap<&str, &str> = HashMap::new();

        query.insert("filter[id_employee_sales_rep]", &self.id);
        query.insert("filter[id_store]", &self.id_store);
        query.insert("filter[id_order_type]", "2");
        query.insert("sort", "[id_DESC]");
        query.insert("limit", "0,20");
        query.insert("output_format", "JSON");

        let orders: Vec<prestashop_schema::Order> = api_call
            .request_resources_wasm("orders", query.clone())
            .await?;
        Ok(orders)
    }

    async fn get_services_in_my_store(&mut self) -> Result<Vec<prestashop_schema::Order>, Error> {
        let api_call = Prestashop::default();
        let mut query: HashMap<&str, &str> = HashMap::new();

        query.insert("filter[id_store]", &self.id_store);
        query.insert("filter[id_order_type]", "2");
        query.insert("sort", "[id_DESC]");
        query.insert("limit", "0,20");
        query.insert("output_format", "JSON");

        let orders: Vec<prestashop_schema::Order> = api_call
            .request_resources_wasm("orders", query.clone())
            .await?;
        Ok(orders)
    }
}

#[async_trait]
impl UserHelper for User {
    async fn find_employee(&mut self) -> Result<prestashop_schema::Employee, Error> {
        let api_call = Prestashop::default();
        let mut query: HashMap<&str, &str> = HashMap::new();

        query.insert("filter[email]", &self.email);
        query.insert("output_format", "JSON");

        let employee: prestashop_schema::Employee = api_call
            .find_resource_wasm("employees", query.clone())
            .await?;
        Ok(employee)
    }
}

#[async_trait]
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
