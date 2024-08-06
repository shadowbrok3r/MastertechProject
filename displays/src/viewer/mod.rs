use bincode::Options;
use eframe::{egui::{ClippedPrimitive, Color32, FontDefinitions, Mesh, PlatformOutput, Pos2, RawInput, Rect, Shape}, emath::History, epaint::{CircleShape, ClippedShape, CubicBezierShape, EllipseShape, Fonts, PathShape, PathStroke, Primitive, QuadraticBezierShape, RectShape, TessellationOptions, Tessellator, TextShape}};
use crossbeam::channel::{Receiver, Sender};
use ewebsock::{WsEvent, WsMessage, WsReceiver, WsSender};
use serde::{Deserialize, Serialize};
use parking_lot::Mutex;
use std::sync::Arc;
use log::info;
use anyhow::{Context, Result};

#[derive(Serialize, Deserialize, Default)]
pub struct EguiFrame {
    pub frame_index: u64,
    pub clipped_meshes: Vec<NetMesh>,
    pub output: PlatformOutput,
    pub pixels_per_point: f32,
}

#[derive(Serialize, Deserialize)]
pub struct NetMesh(Rect, NetShape);

#[derive(Serialize, Deserialize, Default)]
pub enum NetShape {
    #[default]
    Noop,
    /// Circle with optional outline and fill.
    Circle(CircleShape),
    /// Ellipse with optional outline and fill.
    Ellipse(EllipseShape),
    /// A line between two points.
    LineSegment {
        points: [Pos2; 2],
        stroke: PathStroke,
    },
    /// A series of lines between points.
    /// The path can have a stroke and/or fill (if closed).
    Path(PathShape),
    /// Rectangle with optional outline and fill.
    Rect(RectShape),
    /// Text.
    ///
    /// This needs to be recreated if `pixels_per_point` (dpi scale) changes.
    Text(TextShape),
    /// A general triangle mesh.
    ///
    /// Can be used to display images.
    Mesh(Mesh),
    /// A quadratic [Bézier Curve](https://en.wikipedia.org/wiki/B%C3%A9zier_curve).
    QuadraticBezier(QuadraticBezierShape),
    /// A cubic [Bézier Curve](https://en.wikipedia.org/wiki/B%C3%A9zier_curve).
    CubicBezier(CubicBezierShape),
}

#[derive(Serialize, Deserialize)]
pub enum ClientToServerMessage {
    Input {
        raw_input: RawInput,
        /// Seconds since epoch. Used to measure latency.
        client_time: f64,
    },
    Goodbye,
}
/// Mastertech will be the server, and the website is the client.
/// This will be used to send all of Mastertechs fonts,
/// meshes, and everything needed to redraw and recreate
/// a window from mastertech and view it on the website
#[derive(Serialize, Deserialize)]
pub enum ServerToClientMessage {
    /// Sent first to all clients so they know how to paint the NetShapes
    Fonts {
        font_definitions: FontDefinitions,
    },

    /// What to paint to screen.
    Frame {
        frame_index: u64,
        clipped_meshes: Vec<NetMesh>,
        output: PlatformOutput,
        pixels_per_point: f32,
        /// If this frame is a response to a `ClientToServerMessage::Input`.
        /// Used to measure latency.
        client_time: Option<f64>,
    },
}

pub struct Client {
    outgoing_msg_tx: Sender<ClientToServerMessage>,
    incoming_msg_rx: Receiver<ServerToClientMessage>,
    font_definitions: FontDefinitions,
    fonts: Option<Fonts>,
    latest_frame: Option<EguiFrame>,
    bandwidth_history: Arc<Mutex<History<f32>>>,
    frame_size_history: Arc<Mutex<History<f32>>>,
    latency_history: History<f32>,
    frame_history: History<()>,
}

impl Client {
    pub fn new(ws_sender: WsSender, ws_receiver: WsReceiver) -> Self {
        let mut bandwidth_history = Arc::new(Mutex::new(History::new(0..200, 2.0)));
        let mut frame_size_history = Arc::new(Mutex::new(History::new(1..100, 0.5)));
        let (outgoing_msg_tx, mut outgoing_msg_rx) = crossbeam::channel::unbounded();
        let (mut incoming_msg_tx, incoming_msg_rx) = crossbeam::channel::unbounded();

        let client = Self {
            outgoing_msg_tx,
            incoming_msg_rx,
            font_definitions: Default::default(),
            fonts: None,
            latest_frame: Default::default(),
            bandwidth_history: bandwidth_history.clone(),
            frame_size_history: frame_size_history.clone(),
            latency_history: History::new(1..100, 1.0),
            frame_history: History::new(2..100, 1.0),
        };

        if let Err(e) = run(
            ws_sender,
            ws_receiver,
            &mut outgoing_msg_rx,
            &mut incoming_msg_tx,
            &mut bandwidth_history,
            &mut frame_size_history,
        ) {
            info!("Error with Client: {e:?}");
        }

        client
    }

    pub fn send_input(&self, raw_input: RawInput) {
        self.outgoing_msg_tx
            .send(ClientToServerMessage::Input {
                raw_input,
                client_time: now(),
            })
            .ok();
    }

    /// Estimated bandwidth use (downstream).
    pub fn bytes_per_second(&self) -> f32 {
        self.bandwidth_history.lock().bandwidth().unwrap_or(0.0)
    }

    /// Estimated size of one frame packet
    pub fn average_frame_packet_size(&self) -> Option<f32> {
        self.frame_size_history.lock().average()
    }

    /// Smoothed round-trip-time estimate in seconds.
    pub fn latency(&self) -> Option<f32> {
        self.latency_history.average()
    }

    /// Smoothed estimate of the adaptive frames per second.
    pub fn adaptive_fps(&self) -> Option<f32> {
        self.frame_history.rate()
    }

    pub fn update(&mut self, pixels_per_point: f32) -> Option<EguiFrame> {
        if self.fonts.is_none() {
            self.fonts = Some(Fonts::new(pixels_per_point, 1, self.font_definitions.clone()));
        }
        let fonts = self.fonts.as_mut().unwrap();
        if pixels_per_point != fonts.pixels_per_point() {
            *fonts = Fonts::new(pixels_per_point, 1, self.font_definitions.clone());
        }

        while let Ok(msg) = self.incoming_msg_rx.try_recv() {
            match msg {
                ServerToClientMessage::Fonts { font_definitions } => {
                    self.font_definitions = font_definitions;
                    *fonts = Fonts::new(pixels_per_point, 1, self.font_definitions.clone());
                }
                ServerToClientMessage::Frame {
                    frame_index,
                    output,
                    clipped_meshes,
                    client_time,
                    pixels_per_point
                } => {
                    let clipped_shapes = from_clipped_net_shapes(fonts, clipped_meshes);
                    let tesselator_options = TessellationOptions::default();
                    let tex_size = fonts.font_image_size();
                    let mut tesselator = Tessellator::new(pixels_per_point, tesselator_options, tex_size, vec![]);
                    let clipped_prims = tesselator.tessellate_shapes(clipped_shapes);
                    let latest_frame = self.latest_frame.get_or_insert_with(EguiFrame::default);
                    latest_frame.frame_index = frame_index;
                    latest_frame.output.append(output);
                    latest_frame.clipped_meshes = from_clipped_prims(clipped_prims);


                    if let Some(client_time) = client_time {
                        info!("Client time: {client_time:?}");
                        let rtt = (now() - client_time) as f32;
                        self.latency_history.add(now(), rtt);
                    }

                    self.frame_history.add(now(), ());
                }
            }
        }

        self.bandwidth_history.lock().flush(now());
        self.frame_size_history.lock().flush(now());
        self.latency_history.flush(now());
        self.frame_history.flush(now());

        self.latest_frame.take()
    }
}

fn run(
    mut ws_sender: WsSender,
    ws_receiver: WsReceiver,
    outgoing_msg_rx: &mut Receiver<ClientToServerMessage>,
    incoming_msg_tx: &mut Sender<ServerToClientMessage>,
    bandwidth_history: &mut Arc<Mutex<History<f32>>>,
    frame_size_history: &mut Arc<Mutex<History<f32>>>,
) -> Result<()> {

    loop {
        match outgoing_msg_rx.try_recv() {
            Ok(msg) => ws_sender.send(WsMessage::Binary(encode_message(&msg)?)),
            Err(e) => info!("Error: {e:?}"),
        }

        while let Some(event) = ws_receiver.try_recv() {
            match event{
                WsEvent::Opened => info!("Connection opened"),
                WsEvent::Message(msg) => {
                    match msg{
                        ewebsock::WsMessage::Binary(bin) => {
                            bandwidth_history.lock().add(now(), bin.len() as f32);
                            let message = decode_message(&bin).context("decode")?;
                            if let ServerToClientMessage::Frame { .. } = &message {
                                frame_size_history.lock().add(now(), bin.len() as f32);
                            }
                            incoming_msg_tx.send(message)?;
                        },
                        _ => {}
                    }
                },
                WsEvent::Error(e) => info!("Error: {e:?}"),
                WsEvent::Closed => break,
            }
        }
    }
}

fn encode_message<M: ?Sized + Serialize>(message: &M) -> anyhow::Result<Vec<u8>> {
    let bincoded = bincode::options().serialize(message).context("bincode")?;
    const ZSTD_LEVEL: i32 = 5;
    let compressed = zstd::encode_all(std::io::Cursor::new(&bincoded), ZSTD_LEVEL).context("zstd")?;
    Ok(compressed.into())
}

fn decode_message<M: serde::de::DeserializeOwned>(packet: &[u8]) -> anyhow::Result<M> {
    let bincoded = zstd::decode_all(packet).context("zstd")?;

    let message = bincode::options()
        .deserialize(&bincoded)
        .context("bincode")?;

    Ok(message)
}

pub fn from_clipped_net_shapes(
    fonts: &Fonts,
    in_shapes: Vec<NetMesh>
) -> Vec<ClippedShape> {
    in_shapes
        .into_iter()
        .map(|NetMesh(clip_rect, net_shape)| {
            ClippedShape{ clip_rect, shape: to_epaint_shape(fonts, net_shape) }
        })
        .collect()
}

pub fn from_clipped_prims(in_shapes: Vec<ClippedPrimitive>) -> Vec<NetMesh> {
    in_shapes
        .into_iter()
        .map(|ClippedPrimitive{clip_rect, primitive }| {
            if let Primitive::Mesh(mesh) = primitive {
                NetMesh(clip_rect, NetShape::Mesh(mesh))
            } else {
                NetMesh(clip_rect, NetShape::Noop)
            }
        })
        .collect()
}

fn to_epaint_shape(fonts: &Fonts, net_shape: NetShape) -> Shape {
    match net_shape {
        NetShape::Circle(circle_shape) => Shape::Circle(circle_shape),
        NetShape::LineSegment { points, stroke } => Shape::LineSegment { points, stroke },
        NetShape::Path(path_shape) => Shape::Path(path_shape),
        NetShape::Rect(rect_shape) => Shape::Rect(rect_shape),
        NetShape::Text(text_shape) => {
            // let g: std::sync::Arc<eframe::egui::Galley> = text_shape.clone().galley.clone();
            // let job = g.job.clone();
            let galley = fonts.layout_job(text_shape.galley.job.as_ref().clone());
            Shape::Text(TextShape {
                pos: text_shape.pos,
                galley,
                underline: text_shape.underline,
                override_text_color: text_shape.override_text_color,
                angle: text_shape.angle,
                fallback_color: Color32::default(),
                opacity_factor: 1.0,
            })
        }
        NetShape::Mesh(net_mesh) => Shape::Mesh(Mesh::from(net_mesh)),
        NetShape::Ellipse(ellipse_shape) => Shape::Ellipse(ellipse_shape),
        NetShape::QuadraticBezier(quadratic_bezier_shape) => Shape::QuadraticBezier(quadratic_bezier_shape),
        NetShape::CubicBezier(cubic_bezier_shape) => Shape::CubicBezier(cubic_bezier_shape),
        NetShape::Noop => Shape::Noop,
    }
}

fn now() -> f64 {
    std::time::UNIX_EPOCH.elapsed().unwrap().as_secs_f64()
}