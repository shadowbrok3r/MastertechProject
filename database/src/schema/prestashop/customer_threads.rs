use crate::schema::prestashop::deserialize_to_string;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub struct CustomerThread {
    #[serde(deserialize_with = "deserialize_to_string")]
    pub id: String,
    pub id_customer: String, // isUnsignedId 	❌ 		Customer ID
    pub id_order: String,    // isUnsignedId 	❌ 		Order ID
    pub date_add: String,    // isDate 	        ❌
    pub date_upd: String,    // isDate 	        ❌
    #[serde(default = "empty_association")]
    pub associations: CustMessageAssociation,
}

#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub struct CustMessageAssociation {
    pub customer_messages: Vec<CustMessage>,
}

fn empty_association() -> CustMessageAssociation {
    CustMessageAssociation { 
        customer_messages: vec![]
    }
}

#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub struct CustMessage {
    pub id: String,
}