use super::config::*;
use super::errors::*;
use super::rest_model::OrderType;
use super::ws_model::WebsocketResponse;

use awc::ws::Message;
use log::debug;
use std::sync::atomic::{AtomicBool, Ordering};

use actix_codec::Framed;
use awc::{
    ws::{Codec, Frame},
    BoxedSocket, Client, ClientResponse,
};
use futures_util::{sink::SinkExt as _, stream::StreamExt as _};
use serde::{Deserialize, Serialize};
use serde_json::from_slice;
use tokio::sync::mpsc;
use uuid::Uuid;

pub struct WebSockets<WE: serde::de::DeserializeOwned> {
    pub socket: Option<(ClientResponse, Framed<BoxedSocket, Codec>)>,
    sender: mpsc::Sender<WE>,
    conf: Config,
}

impl<WE: serde::de::DeserializeOwned> WebSockets<WE> {
    /// New websocket holder with default configuration
    /// # Examples
    /// see examples/binance_WebSockets.rs
    pub fn new(sender: mpsc::Sender<WE>) -> WebSockets<WE> {
        Self::new_with_options(sender, Config::default())
    }

    /// New websocket holder with provided configuration
    /// # Examples
    /// see examples/binance_WebSockets.rs
    pub fn new_with_options(sender: mpsc::Sender<WE>, conf: Config) -> WebSockets<WE> {
        WebSockets {
            socket: None,
            sender: sender,
            conf,
        }
    }

    /// Connect to a websocket endpoint
    pub async fn connect(&mut self, endpoint: &str) -> Result<()> {
        let wss: String = format!("{}/{}", self.conf.ws_endpoint, endpoint);

        let client = Client::builder()
            .max_http_version(awc::http::Version::HTTP_11)
            .finish();

        match client.ws(wss).connect().await {
            Ok(answer) => {
                self.socket = Some(answer);
                Ok(())
            }
            Err(e) => Err(Error::Msg(format!("Error during handshake {}", e))),
        }
    }

    pub async fn subscribe_request(&mut self, request: &str) -> Result<()> {
        if let Some((_, ref mut socket)) = self.socket {
            socket.send(Message::Text(request.into())).await?;
            Ok(())
        } else {
            Err(Error::Msg("Not able to send requests".to_string()))
        }
    }

    /// Disconnect from the endpoint
    pub async fn disconnect(&mut self) -> Result<()> {
        if let Some((_, ref mut socket)) = self.socket {
            socket.close().await?;
            Ok(())
        } else {
            Err(Error::Msg("Not able to close the connection".to_string()))
        }
    }

    pub fn socket(&self) -> &Option<(ClientResponse, Framed<BoxedSocket, Codec>)> {
        &self.socket
    }

    pub async fn event_loop(&mut self, running: &AtomicBool) -> Result<()> {
        while running.load(Ordering::Relaxed) {
            if let Some((_, ref mut socket)) = self.socket {
                let message = socket.next().await.unwrap()?;
                debug!("event_loop message - {:?}", message);
                match message {
                    Frame::Text(msg) => {
                        if msg.is_empty() {
                            return Ok(());
                        }
                        if let Ok(event) = from_slice(&msg) {
                            if let Err(_e) = self.sender.send(event).await {
                                println!("SendError<WE>");
                            }
                        } else if let Ok(response) = from_slice::<WebsocketResponse>(&msg) {
                            println!("WebsocketResponse: {:?}", response);
                        } else {
                            return Err(Error::Msg(format!("Websocket Parse failed {:?}", msg)));
                        }
                    }
                    Frame::Ping(_) | Frame::Pong(_) | Frame::Binary(_) | Frame::Continuation(_) => {
                    }
                    Frame::Close(e) => {
                        return Err(Error::Msg(format!("Disconnected {:?}", e)));
                    }
                }
                actix_rt::task::yield_now().await;
            }
        }
        Ok(())
    }

    // trade start from here
    async fn place_order(&mut self, order: WSOrder) -> Result<()> {
        if let Some((_, ref mut socket)) = self.socket {
            let ws_order = WSOrderRequest {
                id: Uuid::new_v4().to_string(),
                op: "order".to_string(),
                args: vec![order],
            };

            let text = serde_json::to_string(&ws_order)?;
            socket.send(Message::Text(text.into())).await?;
            Ok(())
        } else {
            Err(Error::Msg("Not able to send requests".to_string()))
        }
    }

    async fn place_multipy_order(&mut self, orders: Vec<WSOrder>) -> Result<()> {
        if let Some((_, ref mut socket)) = self.socket {
            let ws_orders = WSOrderRequest {
                id: Uuid::new_v4().to_string(),
                op: "batch-orders".to_string(),
                args: orders,
            };

            let text = serde_json::to_string(&ws_orders)?;
            socket.send(Message::Text(text.into())).await?;
            Ok(())
        } else {
            Err(Error::Msg("Not able to send requests".to_string()))
        }
    }

    pub async fn limit_buy(
        &mut self,
        symbol: impl Into<String>,
        qty: impl Into<String>,
        price: impl Into<String>,
        order_type: OrderType,
    ) -> Result<()> {
        let order = WSOrder {
            symbol: symbol.into(),
            trade_mode: TradeMode::Cross,
            currency: None,
            client_order_id: None,
            tag: None,
            side: OrderSide::Buy,
            position_side: None, // None for net mode
            order_type: order_type,
            qty: qty.into(),
            price: Some(price.into()),
            reduce_only: None,
            target_currency: None,
        };
        self.place_order(order).await?;
        Ok(())
    }

    pub async fn limit_sell(
        &mut self,
        symbol: impl Into<String>,
        qty: impl Into<String>,
        price: impl Into<String>,
        order_type: OrderType,
    ) -> Result<()> {
        let order = WSOrder {
            symbol: symbol.into(),
            trade_mode: TradeMode::Cross,
            currency: None,
            client_order_id: None,
            tag: None,
            side: OrderSide::Sell,
            position_side: None, // None for net mode
            order_type: order_type,
            qty: qty.into(),
            price: Some(price.into()),
            reduce_only: None,
            target_currency: None,
        };
        self.place_order(order).await?;
        Ok(())
    }

    pub async fn market_buy() {}

    pub async fn market_sell() {}

    pub async fn cancel_order() {}

    pub async fn amend_order() {}

    pub async fn amend_multiple_order() {}
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PositionSide {
    Net,
    Long,
    Short,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "snake_case")]
pub enum OrderSide {
    Buy,
    Sell,
}

/// By default, buy
impl Default for OrderSide {
    fn default() -> Self {
        Self::Buy
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "snake_case")]
pub enum TradeMode {
    Isolated,
    Cross,
    Cash,
}

/// By default, Cross
impl Default for TradeMode {
    fn default() -> Self {
        Self::Cross
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WSOrderRequest {
    pub id: String,
    pub op: String,
    pub args: Vec<WSOrder>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WSOrder {
    #[serde(rename = "inst_id")]
    pub symbol: String,
    #[serde(rename = "td_mode")]
    pub trade_mode: TradeMode,
    #[serde(rename = "ccy")]
    pub currency: Option<String>,
    #[serde(rename = "clOrdId")]
    pub client_order_id: Option<String>,
    pub tag: Option<String>,
    pub side: OrderSide,
    #[serde(rename = "posSide")]
    pub position_side: Option<String>,
    #[serde(rename = "ord_type")]
    pub order_type: OrderType,
    #[serde(rename = "sz")]
    pub qty: String,
    #[serde(rename = "px")]
    pub price: Option<String>,
    pub reduce_only: Option<bool>,
    #[serde(rename = "tgtCcy")]
    pub target_currency: Option<String>,
}
