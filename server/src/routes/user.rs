use axum::Extension;
use axum::{
    response::IntoResponse,
    Json,
};
use log::{info, debug};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use surrealdb::opt::auth::{Scope, Jwt};
use surrealdb::sql::Thing;
use tower_cookies::{Cookies, Cookie};
use std::borrow::Cow;
use std::future::Future;
use std::pin::Pin;
use database::Database;
use database::schema::{NotificationId, Record, UserId, DB, NS, USER_SCOPE, USER_TABLE};
use crate::middlewares::auth_middleware::JWT_KEY;
use crate::middlewares::context::Ctx;
use crate::utils::error::Error;
use crate::utils::error::{ApiResult, ApiError};


#[derive(Debug, Serialize, Deserialize)]
pub struct ReturnedStoreUsers {
    pub name: String,
    pub everest_initials: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct OriginStore {
    pub origin_store: String,
}

#[derive(Debug, Serialize)]
pub struct LogoutSuccess {
    pub success: bool,
}

#[derive(Debug, Serialize)]
pub struct LogoutResult {
    pub result: LogoutSuccess,
}

// Custom error handling + returning JWT token
#[derive(Serialize)]
pub struct ActionResult {
    pub result: ApiResult<ActionSuccess>,
    pub token: Option<Jwt>,
}

// Credentials input for actions that contains username and password.
#[derive(Debug, Serialize, Deserialize)]
pub struct CredentialsInput {
    pub name: String,
    pub store: String,
    pub email: String,
    pub password: String, 
    pub everest_initials: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SigninInput {
    pub email: String,
    pub password: String, 
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct LoginResponse {
    pub id: Option<UserId>,
    pub name: String,
    pub store: String,
    pub everest_initials: String,
    pub email: Option<String>,
    pub notifications: Option<Vec<NotificationId>>,
}

#[derive(Serialize, Deserialize)]
pub struct ActionSuccess{
    pub login_response: Option<LoginResponse>,
    pub success: bool
}


// Define a generic trait for account actions.
pub trait AccountAction {
    // The type of future that the action function will return.
    type Future: Future<Output = Result<ActionResult, ApiError>> + Send;

    // The function that will perform the action.
    fn execute(
        input: CredentialsInput,
        db: Extension<Database>,
        ctx: Ctx
    ) -> Self::Future;
}


// A generic route handler that works with any AccountAction.
pub async fn account_action_handler<A: AccountAction>(
    db: Extension<Database>, 
    cookies: Cookies,
    ctx: Ctx,
    Json(input): Json<CredentialsInput>, 
) -> impl IntoResponse {
    info!("->> {:<12} - account_action_handler", "HANDLER");

    // Execute the action and return the result.
    match A::execute(input, db, ctx).await {
        Ok(result) => {
            if let Some(token) = result.token.clone(){
                let jwt_cow: Cow<'static, str> = Cow::Owned(token.into_insecure_token());
                cookies.add(
                    Cookie::new(JWT_KEY, jwt_cow)
                );
            } else{
                cookies.remove(Cookie::from(JWT_KEY));
            }


            Json(result)
        },
        Err(e) => {
            //StatusCode::UNAUTHORIZED,
            debug!("Error: {:?}\n removing cookie", e);
            cookies.remove(Cookie::from(JWT_KEY));
            Json(ActionResult { result: Err(e), token: None })
        },
    }
}

// Implement the AccountAction trait for the login action.
pub struct LoginAction;

impl AccountAction for LoginAction {
    type Future = Pin<Box<dyn Future<Output = Result<ActionResult, ApiError>> + Send>>;

    fn execute(
        input: CredentialsInput,
        db: Extension<Database>,
        ctx: Ctx
    ) -> Self::Future {
        Box::pin(async move {
            info!("->> LOGIN INFORMATION: {:<12?}", input);

            // Perform the login logic here.
            let user = CredentialsInput{ 
                email: input.email.clone(), 
                password: input.password,
                name: input.name.clone(),
                store: input.store.clone(),
                everest_initials: input.everest_initials,  
            };
            
            let queried_user = query_user(db.0.clone(), input.email)
                .await
                .unwrap_or(None);

            println!("Value: {queried_user:?}");

            let returned_user: Option<LoginResponse>;
        
            if let Some(login_response) = queried_user{
                returned_user = Some( login_response );
            }else{
                returned_user = None;
            }

           let signin = db
                .database
                .signin(
                    Scope {
                    namespace: NS,
                    database: DB,
                    scope: USER_SCOPE,
                    params: user,
                })
                .await
                .or_else(|err|{
                    Err(ApiError{
                        error: Error::Generic{ description: err.to_string()},
                        req_id: ctx.req_id(),
                    })
                }).and_then(|token| {
                    Ok(ActionResult{
                        token: Some(token),
                        result: Ok(
                            ActionSuccess{ 
                                success: true, 
                                login_response: returned_user
                            }
                        )
                    })
                });

            signin
        })
    }
}

// Implement the AccountAction trait for the create account action.
pub struct CreateAccountAction;

impl AccountAction for CreateAccountAction {
    type Future = Pin<Box<dyn Future<Output = Result<ActionResult, ApiError>> + Send>>;

    fn execute(
        input: CredentialsInput,
        db: Extension<Database>, 
        ctx: Ctx
    ) -> Self::Future {
        Box::pin(async move {
            // Perform the login logic here.
            let user = CredentialsInput{ 
                email: input.email, 
                password: input.password,
                name: input.name.clone(),
                store: input.store.clone(),
                everest_initials: input.everest_initials.clone(),  
            };
            
            let returned_user: LoginResponse = LoginResponse{
                name: input.name,
                store: input.store,
                everest_initials: input.everest_initials,
                ..Default::default()
            };
            
            let signup = db
                .database
                .signup(
                    Scope {
                    namespace: NS,
                    database: DB,
                    scope: USER_SCOPE,
                    params: user,
                })
                .await
                .or_else(|err|{
                    Err(ApiError{
                        error: Error::Generic{ description: err.to_string()},
                        req_id: ctx.req_id(),
                    })
                }).and_then(|token| {
                    Ok(ActionResult{
                        token: Some(token),
                        result: Ok(
                            ActionSuccess{ 
                                success: true, 
                                login_response: Some(returned_user)
                            }
                        )
                    })
                });

            signup
        })
    }
}


pub async fn logout(
    db: Extension<Database>, 
    cookies: Cookies,
    ctx: Ctx
) -> ApiResult<Json<LogoutResult>> {
    info!("Logging out");
    let _ = db
        .database
        .invalidate()
        .await
        .or_else(|err|{
            Err(ApiError{
                error: Error::Generic{ description: err.to_string()},
                req_id: ctx.req_id(),
            })
        }).and_then(|_| {
            cookies.remove(Cookie::from(JWT_KEY));
            Ok(ActionResult{
                token: None,
                result: Ok(
                    ActionSuccess{ 
                        success: true, 
                        login_response: None
                    }
                )
            })
        });
    Ok(Json(LogoutResult {
        result: LogoutSuccess { success: true },
    }))
}

pub async fn query_user(db: Database, user_email: String)
-> Result<Option<LoginResponse>, surrealdb::Error>{ // Vec<TicketData>

    let query = format!("SELECT * FROM user WHERE email=='{}'", user_email);
    
    info!("->> QUERY_USER: {:<12?}", user_email);

    let queried_user: Option<Value> = db
        .database
        .query(query.as_str())
        .await?
        .take(0)?;

    println!("Value: {queried_user:?}");

    let returned_user: Option<LoginResponse>;
        

    if let Some(user) = queried_user{
        let login_response: LoginResponse = serde_json::from_value(user).unwrap_or(
            LoginResponse::default() 
        );
        returned_user = Some( login_response );
    }else{
        returned_user = None;
    }

    Ok(returned_user)
}

pub async fn query_user_from_initials(db: Database, initials: Option<String>, email: Option<String>)
-> Result<Option<UserId>, surrealdb::Error>{ // Vec<TicketData>
    
    
    let query: String;
    if let Some(initials) = initials.clone(){
        query = format!("SELECT id FROM user WHERE everest_initials == '{}'", initials);
    }else if let Some(email) = email{
        query = format!("SELECT id FROM user WHERE email == '{}'", email);
    }else{
        query = format!("");
    }

    let queried_user: Option<Record> = db
        .database
        .query(query.as_str())
        .await?
        .take(0)?;
    // println!("Queried User from input: {:?}", &queried_user.unwrap());
    if let Some(user) = queried_user{
        Ok(Some(UserId(user.id)))
    }else{
        Ok(
            Some(
                UserId
                (
                    Thing::from
                    (
                        (USER_TABLE.to_string(), initials.unwrap())
                    )
                )
            )
        )
    }
}

pub async fn get_current_user(
    db: Extension<Database>, 
    _ctx: Ctx,
    Json(input): Json<CredentialsInput>,
) -> Json<Result<Option<LoginResponse>, ApiError>> { 
    let query = format!("SELECT name, store, everest_initials FROM user WHERE email == '{}'", input.email);

    let queried_user: Option<Value> = db
        .database
        .query(query.as_str())
        .await
        .unwrap()
        .take(0)
        .unwrap();

    println!("Value: {queried_user:?}");

    let returned_user: Option<LoginResponse>;

    if let Some(user) = queried_user{
        
        let login_response: LoginResponse = serde_json::from_value(user).unwrap_or(
            LoginResponse::default()
        );

        returned_user = Some( login_response );
    }else{
        returned_user = None;
    }

    Json(Ok(returned_user))
}

pub async fn get_users_in_store(
    db: Extension<Database>, 
    _ctx: Ctx,
    Json(input): Json<OriginStore>,
) -> Json<Result<Vec<ReturnedStoreUsers>, ApiError>> { 
    println!("input: {input:?}");
    let query = format!("SELECT name, everest_initials FROM user WHERE store == '{}'", input.origin_store);
    println!("query: {query:?}");
    let returned_store_users: Vec<ReturnedStoreUsers> = db
        .database
        .query(query.as_str())
        .await
        .unwrap()
        .take(0)
        .unwrap();
    println!("returned users: {returned_store_users:?}");
    Json(Ok(returned_store_users))
}



