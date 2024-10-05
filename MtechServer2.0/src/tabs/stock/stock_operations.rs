use super::row_viewer::{RawStockData, SerialData, StockData};
use anyhow::{Error, Result};
use crossbeam::channel::Sender;
use database::DATABASE;

pub async fn get_stock(stock_tx: Sender<Vec<RawStockData>>, location: u64) -> Result<(), Error> {
    let res: Option<StockData> = DATABASE
        .query("RETURN fn::store_stock($location, 5000)")
        .bind(("location", location))
        .await?
        .take(0)?;

    // info!("Result: {res:?}");

    stock_tx.try_send(res.unwrap().result)?;
    Ok(())
}

pub async fn find_attached_serial(
    serial: String,
    stock_tx: Sender<SerialData>,
) -> Result<(), Error> {
    // info!("Finding S/N info: {serial}");
    let res: Option<SerialData> = DATABASE
        .query("RETURN fn::find_attached_serial($serial)")
        .bind(("serial", serial))
        .await?
        .take(0)?;

    // info!("Result: {res:?}");

    stock_tx.try_send(res.unwrap())?;
    Ok(())
}

pub async fn find_attached_serials(
    serials: Vec<String>,
    stock_tx: Sender<SerialData>,
) -> Result<(), Error> {
    // info!("Finding S/N info: {serials:?}");
    let res: Option<SerialData> = DATABASE
        .query("RETURN fn::find_attached_serials($serials)")
        .bind(("serials", serials))
        .await?
        .take(0)?;

    // info!("Result: {res:?}");

    stock_tx.try_send(res.unwrap())?;
    Ok(())
}

pub async fn find_products_by_name(
    serial: String,
    stock_tx: Sender<StockData>,
) -> Result<(), Error> {
    let res: Option<StockData> = DATABASE
        .query("RETURN fn::search_stock($serial)")
        .bind(("serial", serial))
        .await?
        .take(0)?;

    // info!("Result: {res:?}");

    stock_tx.try_send(res.unwrap())?;
    Ok(())
}
