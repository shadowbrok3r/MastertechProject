use crate::DATABASE;

use super::{
    prestashop_schema, ConnectedClient, CustomerData, ExtendedSeb, LiveTaskPayload,
    SpecialPartOrder, TaskPayload, TicketData, TicketPayload, User,
};
use anyhow::{Error, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use structdiff::StructDiff;
use surrealdb::opt::RecordId;

// Define a macro to implement GetDataFromId for structs with an 'id' field
macro_rules! get_id {
    ($struct_name:ident) => {
        #[async_trait]
        impl GetDataFromId for $struct_name {
            fn get_id(&self) -> &RecordId {
                &self.id
            }
        }
    };
}

/// A trait for assisting with operations involving the Employee struct
pub trait EmployeeHelper {
    /// Find a User based on Employee info -> id_employee
    fn find_user(&mut self) -> Result<User, Error>;
    /// Pull all of my services given Employee info -> id_employee
    fn get_my_services(&mut self) -> Result<Vec<prestashop_schema::Order>, Error>;
    /// Get all orders in my store given Employee info -> id_location
    fn get_orders_in_my_store(&mut self) -> Result<Vec<prestashop_schema::Order>, Error>;
}

/// A trait for assisting with operations involving the User struct
pub trait UserHelper {
    /// Get Employee record from User info
    fn find_employee(&mut self) -> Result<prestashop_schema::Employee, Error>;
}

/// A trait for assisting with operations involving the ComputerData struct
pub trait ComputerDataHelper {
    /// Associate a ComputerData record to a ServiceOrder
    fn associate_to_service(&mut self) -> Result<prestashop_schema::ServiceOrder, Error>;
    /// Find TicketData associated with this Computer
    fn find_associated_tickets(&mut self) -> Result<Vec<TicketData>, Error>;
    /// Find Clients associated with this Computer
    fn find_associated_client(&mut self) -> Result<ConnectedClient, Error>;
    /// Find Tasks associated to this Computer
    fn find_associated_tasks(&mut self) -> Result<Vec<TaskPayload>, Error>;
    /// Find Customer that owns this Computer
    fn find_associated_customer(&mut self) -> Result<CustomerData, Error>;
    /// Find PrestaShop Orders associated with this Computer
    fn find_associated_prestashop_orders(&mut self)
        -> Result<Vec<prestashop_schema::Order>, Error>;
    /// Find PrestaShop Customer associated with this Computer
    fn find_prestashop_customer(&mut self) -> Result<prestashop_schema::Customer, Error>;
}

/// A trait for assisting with operations involving Customer Records
pub trait CustomerHelper {
    fn find_associated_addr(&mut self) -> Result<prestashop_schema::Address, Error>;
}

pub trait TaskNotePayloadHelper {}

pub trait CustomerThreadHelper {
    fn create_task_note_payload(&mut self);
}

pub trait CustomerDataHelper {
    fn find_part_orders(&mut self) -> Result<Vec<SpecialPartOrder>, Error>;
    fn find_prestashop_customer(&mut self) -> Result<prestashop_schema::Customer, Error>;
    fn get_seb_data(&mut self) -> Result<ExtendedSeb, Error>;
}

pub trait OrderHelper {
    fn convert_to_task_payload(&mut self) -> Result<TaskPayload, Error>;
    fn convert_to_ticket_payload(&mut self) -> Result<TicketPayload, Error>;
}

#[async_trait]
pub trait GetAssociatedDataFromId<D> {
    async fn get_associated_data<T>(&mut self) -> Result<D, Error>
    where
        T: Serialize + for<'de> Deserialize<'de> + Clone,
        D: structdiff::StructDiff + Serialize + for<'de> Deserialize<'de> + Clone;
}

// impl
