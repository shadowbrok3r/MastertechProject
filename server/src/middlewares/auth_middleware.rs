use crate::{
    middlewares::context::Ctx, 
    utils::error::{
        Error,
        ApiResult
    },
    Database
};
use axum::{http::Request, body::Body, middleware::Next, response::{Response, Redirect}, Extension};
use log::{info, debug};
use tower_cookies::{Cookie, Cookies};
use uuid::Uuid;


#[derive(Clone)]
pub struct CtxState {
    pub db: Database,
}

pub const JWT_KEY: &str = "jwt";

pub async fn mw_require_auth(ctx: Ctx, req: Request<Body>, next: Next) -> ApiResult<Response> {
    info!("->> {:<12} - mw_require_auth - {ctx:?}", "MIDDLEWARE");

    ctx.user_id()?;
    Ok(next.run(req).await)
}

pub async fn jwt_auth(
    db: Extension<Database>,
    cookies: Cookies,
    mut req: Request<Body>,
    next: Next,
) -> Response {

    info!("->> {:<12} - jwt_auth", "MIDDLEWARE");

    let database = &db.database;
    let uuid = Uuid::new_v4();

    let extract_token = cookies.get(JWT_KEY)
        .ok_or(Error::AuthFailNoJwtCookie)
        .and_then(|cookie| 
            Ok(cookie.value().to_string()
        ));

    let headers = req.headers();
    // println!("Request Headers: \n{:?}", headers);


    match extract_token{
        Ok(jwt_token) => {
            debug!("extracted jwt_token -> {:?}", jwt_token.get(0..10));
            let _ = database
                .authenticate(jwt_token.clone())
                .await;
            // Store Ctx in the request extension, for extracting in rest handlers
            let ctx = Ctx::new(Ok(jwt_token), uuid);
            req.extensions_mut().insert(ctx);
        },
        Err(e) => {
            log::error!("Error extracting token -> {e:?}");
            // if req.uri().path().contains("/socket.io") || req.uri().path().contains("/ws") {
            //     debug!("Detected Websocket client connection. passing");
            //     let ctx = Ctx::new(Ok("SocketUUID".to_string()), uuid);
            //     req.extensions_mut().insert(ctx);

            // }else{
                let _ = database.invalidate().await;
            
                // Store Ctx in the request extension, for extracting in rest handlers
                let ctx = Ctx::new(Err(e.clone()), uuid);
                // debug!("Error: {e:?}");
                req.extensions_mut().insert(ctx);
    
                if let Error::AuthFailJwtInvalid { .. } = e{
                    log::error!("Error: {e:?}");
                    let _ = Redirect::to("/login");
                    cookies.remove(Cookie::from(JWT_KEY)) 
                } else if let Error::AuthFailCtxNotInRequestExt { .. } = e{
                //     let _ = Redirect::to("/login");
                    log::error!("No ctx in request");
                }
            // }

        },
    }
    
    next.run(req).await
}
