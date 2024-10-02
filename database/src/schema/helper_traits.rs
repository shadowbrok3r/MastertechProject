use crate::DATABASE;

use super::{
    prestashop_schema::{self, Employee, Prestashop},
    ComputerData, ConnectedClient, CustomerData, ExtendedSeb, SpecialPartOrder, Store, TaskPayload,
    TicketData, TicketPayload, User,
};
use anyhow::{Error, Result};
use async_trait::async_trait;
use log::{debug, info};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{collections::HashMap, fmt::Debug};

/// Macro to implement GetDataFromId for structs with an 'id' field
macro_rules! _get_id {
    ($struct_name:ident) => {
        #[async_trait(?Send)]
        impl GetDataFromId for $struct_name {
            async fn get_id(&self) -> &RecordId {
                &self.id
            }
        }
    };
}

/// A trait for assisting with operations involving the Employee struct
#[async_trait(?Send)]
pub trait EmployeeHelper {
    /// Find a User based on Employee info -> id_employee
    async fn find_user(&mut self) -> Result<User, Error>;
    /// Pull all of my services given Employee info -> id_employee
    async fn get_my_services(&mut self) -> Result<Vec<prestashop_schema::Order>, Error>;
    /// Get all orders in my store given Employee info -> id_location
    async fn get_services_in_my_store(&mut self) -> Result<Vec<prestashop_schema::Order>, Error>;
    /// Get all Orders of which are my Return For Service's
    async fn get_my_return_for_services(&mut self) -> Result<Vec<prestashop_schema::Order>, Error>;
    /// Get all Return For Service's in my store
    async fn get_my_store_return_for_services(
        &mut self,
    ) -> Result<Vec<prestashop_schema::Order>, Error>;
}

/// A trait for assisting with operations involving the User struct
#[async_trait(?Send)]
pub trait UserHelper {
    /// Get Employee record from User info
    async fn find_employee(&mut self) -> Result<prestashop_schema::Employee, Error>;

    async fn save_user_settings(&mut self) -> Result<(), Error>;

    fn get_odoo_store_number(&mut self) -> Result<u64, Error>;

    fn get_store_from_odoo_id(&mut self) -> Result<Store, Error>;
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

/// A trait for assisting with operations involving Customer Records
#[async_trait(?Send)]
pub trait CustomerHelper {
    async fn find_associated_addr(&mut self) -> Result<prestashop_schema::Address, Error>;
}

#[async_trait(?Send)]
pub trait TaskNotePayloadHelper {}

#[async_trait(?Send)]
pub trait CustomerThreadHelper {
    async fn create_task_note_payload(&mut self) -> Result<(), Error>;
}

#[async_trait(?Send)]
pub trait CustomerDataHelper {
    async fn find_part_orders(&mut self) -> Result<Vec<SpecialPartOrder>, Error>;
    async fn find_prestashop_customer(&mut self) -> Result<prestashop_schema::Customer, Error>;
    async fn get_seb_data(&mut self) -> Result<ExtendedSeb, Error>;
}

#[async_trait(?Send)]
pub trait OrderHelper {
    async fn convert_to_task_payload(&mut self) -> Result<TaskPayload, Error>;
    async fn convert_to_ticket_payload(&mut self) -> Result<TicketPayload, Error>;
    async fn get_all_return_for_services(&mut self)
        -> Result<Vec<prestashop_schema::Order>, Error>;
}

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

#[async_trait(?Send)]
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

    async fn get_my_return_for_services(&mut self) -> Result<Vec<prestashop_schema::Order>, Error> {
        let mut api_call = Prestashop::default();
        api_call.display = "";
        let mut query: HashMap<&str, &str> = HashMap::new();

        query.insert("filter[product_reference]", "SRVC/RETURN");
        query.insert("sort", "[id_DESC]");
        query.insert("limit", "5");
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

    async fn get_my_store_return_for_services(
        &mut self,
    ) -> Result<Vec<prestashop_schema::Order>, Error> {
        let api_call = Prestashop::default();
        let mut query: HashMap<&str, &str> = HashMap::new();

        query.insert("filter[product_reference]", "SRVC/RETURN");
        query.insert("sort", "[id_DESC]");
        query.insert("limit", "5");
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
                order_query.insert("filter[id_store]", &self.id_store);
                order_query.insert("filter[id_order_type]", "2");

                let order: prestashop_schema::Order = api_call
                    .request_subresources_by_id_wasm("orders", "order", &id)
                    .await?;

                orders_vec.push(order);
            }
        }

        Ok(orders_vec)
    }
}

#[async_trait(?Send)]
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
    async fn save_user_settings(&mut self) -> Result<(), Error> {
        let user_settings = serde_json::to_value(self.user_settings.clone())?;

        info!(
            "User Settings to apply: {user_settings:?}\nTo User: {:?}",
            self.id.clone()
        );

        match DATABASE
            .query("UPDATE user SET user_settings = $settings WHERE id == $user")
            .bind(("settings", user_settings))
            .bind(("user", self.id.clone()))
            .await
        {
            Ok(res) => {
                info!("Result: {res:?}");
            }
            Err(e) => info!("Error updating User Settings: {e:?}"),
        }
        Ok(())
    }

    fn get_odoo_store_number(&mut self) -> Result<u64, Error> {
        let store = match self.store {
            Store::RIV => 76,
            Store::LTN => 73,
            Store::MUR => 74,
            Store::AF => 72,
            Store::WJ => 78,
            Store::ORE => 75,
            Store::SAN => 77,
        };
        Ok(store)
    }

    fn get_store_from_odoo_id(&mut self) -> Result<Store, Error> {
        let store = match self.get_odoo_store_number()? {
            76 => Store::RIV,
            73 => Store::LTN,
            74 => Store::MUR,
            78 => Store::WJ,
            75 => Store::ORE,
            72 => Store::AF,
            77 => Store::SAN,
            _ => Store::RIV,
        };
        Ok(store)
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
