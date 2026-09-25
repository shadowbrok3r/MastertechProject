//! Files attached to an agent chat message: pictures downscaled and re-encoded, text inlined, the rest refused.

use std::io::Cursor;
use std::sync::{LazyLock, Mutex};

use base64::Engine;
use crossbeam::channel::Sender;
use database::schema::{upload_name, TurnImage};
use eframe::egui::{self, ColorImage, TextureHandle, TextureOptions};
use image::imageops::FilterType;
use image::{DynamicImage, ImageFormat, RgbaImage};

/// Longest side of a sent picture, in pixels.
pub const IMAGE_MAX_SIDE: u32 = 1600;
/// Largest encoded picture a message carries.
pub const IMAGE_MAX_BYTES: usize = 1536 * 1024;
/// Largest text file inlined into a message.
pub const TEXT_MAX_BYTES: usize = 200 * 1024;
/// Pictures one message may carry.
pub const MAX_IMAGES: usize = 4;
/// Longest side of a thumbnail, in pixels.
const THUMB_SIDE: u32 = 160;
/// JPEG qualities tried, best first, before a picture is scaled down further.
const JPEG_QUALITIES: [u8; 3] = [85, 72, 60];
/// Scale applied per round when no quality fits the cap.
const SHRINK: f32 = 0.75;
/// Rounds of shrinking before a picture is refused.
const SHRINK_ROUNDS: usize = 4;
/// Thumbnails of sent pictures kept for the transcript.
const SENT_KEPT: usize = 64;

/// A file ready to go out with a message.
pub struct Attachment {
    pub name: String,
    pub body: Body,
    pub thumb: Option<TextureHandle>,
}

/// An attachment's content as it will be sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Body {
    Image { mime: &'static str, bytes: Vec<u8>, width: u32, height: u32 },
    Text(String),
}

/// A file once read: a picture with its thumbnail pixels, or text.
pub struct Prepared {
    pub name: String,
    pub body: Body,
    pub thumb: Option<ColorImage>,
}

impl Prepared {
    /// The attachment, with its thumbnail uploaded as a texture.
    pub fn into_attachment(self, ctx: &egui::Context) -> Attachment {
        let thumb = self.thumb.map(|img| ctx.load_texture(format!("attach:{}", self.name), img, TextureOptions::LINEAR));
        Attachment { name: self.name, body: self.body, thumb }
    }
}

/// Progress of a background read of an attachment.
pub enum AttachEvent {
    Started,
    Ready(Prepared),
    Refused(String),
}

/// `(width, height)` scaled down to fit `max_side`, keeping the aspect ratio; never scaled up.
pub fn fit_within(width: u32, height: u32, max_side: u32) -> (u32, u32) {
    let longest = width.max(height);
    if longest <= max_side || longest == 0 {
        return (width.max(1), height.max(1));
    }
    let scale = max_side as f64 / longest as f64;
    let side = |v: u32| ((v as f64 * scale).round() as u32).clamp(1, max_side);
    (side(width), side(height))
}

/// A picture resized to fit `max_side` and encoded under [`IMAGE_MAX_BYTES`]: PNG when it fits, else JPEG.
pub fn encode_for_send(img: &DynamicImage, prefer_jpeg: bool) -> Result<(&'static str, Vec<u8>, u32, u32), String> {
    let mut side = IMAGE_MAX_SIDE;
    for _ in 0..=SHRINK_ROUNDS {
        let (w, h) = fit_within(img.width(), img.height(), side);
        let sized = if (w, h) == (img.width(), img.height()) { img.clone() } else { img.resize_exact(w, h, FilterType::CatmullRom) };
        if !prefer_jpeg {
            let png = encode_png(&sized)?;
            if png.len() <= IMAGE_MAX_BYTES {
                return Ok(("image/png", png, w, h));
            }
        }
        for quality in JPEG_QUALITIES {
            let jpeg = encode_jpeg(&sized, quality)?;
            if jpeg.len() <= IMAGE_MAX_BYTES {
                return Ok(("image/jpeg", jpeg, w, h));
            }
        }
        side = ((side as f32) * SHRINK) as u32;
    }
    Err(format!("the picture stays over {} KB even scaled down", IMAGE_MAX_BYTES / 1024))
}

fn encode_png(img: &DynamicImage) -> Result<Vec<u8>, String> {
    let mut out = Cursor::new(Vec::new());
    img.write_to(&mut out, ImageFormat::Png).map_err(|e| format!("PNG encode failed: {e}"))?;
    Ok(out.into_inner())
}

/// JPEG of `img` with any transparency flattened onto white.
fn encode_jpeg(img: &DynamicImage, quality: u8) -> Result<Vec<u8>, String> {
    let rgba = img.to_rgba8();
    let mut rgb = image::RgbImage::new(rgba.width(), rgba.height());
    for (dst, src) in rgb.pixels_mut().zip(rgba.pixels()) {
        let a = u16::from(src[3]);
        let mix = |c: u8| ((u16::from(c) * a + 255 * (255 - a)) / 255) as u8;
        *dst = image::Rgb([mix(src[0]), mix(src[1]), mix(src[2])]);
    }
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality)
        .encode_image(&rgb)
        .map_err(|e| format!("JPEG encode failed: {e}"))?;
    Ok(out)
}

/// Thumbnail pixels of a picture, fit within [`THUMB_SIDE`].
fn thumbnail(img: &DynamicImage) -> ColorImage {
    let (w, h) = fit_within(img.width(), img.height(), THUMB_SIDE);
    let small = img.resize_exact(w, h, FilterType::Triangle).to_rgba8();
    ColorImage::from_rgba_unmultiplied([w as usize, h as usize], small.as_raw())
}

/// `name` with its extension replaced to match `mime`.
fn name_for(name: &str, mime: &str) -> String {
    let stem = match name.rsplit_once('.') {
        Some((stem, _)) if !stem.is_empty() => stem,
        _ => name,
    };
    let ext = if mime == "image/jpeg" { "jpg" } else { "png" };
    format!("{stem}.{ext}")
}

/// Text of a file: UTF-8, or UTF-16 with a byte-order mark; `None` for binary content.
pub fn decode_text(bytes: &[u8]) -> Option<String> {
    let text = if let Some(rest) = bytes.strip_prefix(b"\xEF\xBB\xBF") {
        String::from_utf8(rest.to_vec()).ok()?
    } else if let Some(rest) = bytes.strip_prefix(b"\xFF\xFE") {
        utf16(rest, u16::from_le_bytes)?
    } else if let Some(rest) = bytes.strip_prefix(b"\xFE\xFF") {
        utf16(rest, u16::from_be_bytes)?
    } else {
        String::from_utf8(bytes.to_vec()).ok()?
    };
    (!text.contains('\0')).then_some(text)
}

fn utf16(bytes: &[u8], word: fn([u8; 2]) -> u16) -> Option<String> {
    if bytes.len() % 2 != 0 {
        return None;
    }
    let units: Vec<u16> = bytes.chunks_exact(2).map(|c| word([c[0], c[1]])).collect();
    String::from_utf16(&units).ok()
}

/// A picture, a text file within [`TEXT_MAX_BYTES`], or the reason the file is refused.
pub fn prepare(name: &str, bytes: &[u8]) -> Result<Prepared, String> {
    if bytes.is_empty() {
        return Err(format!("{name} is empty"));
    }
    if let Ok(format) = image::guess_format(bytes) {
        let img = image::load_from_memory_with_format(bytes, format).map_err(|e| format!("{name} could not be read as a picture: {e}"))?;
        return prepare_image(name, &img, format == ImageFormat::Jpeg);
    }
    let Some(text) = decode_text(bytes) else {
        return Err(format!("{name} is not a picture or a text file, so it cannot be attached"));
    };
    if bytes.len() > TEXT_MAX_BYTES {
        return Err(format!("{name} is {} KB; text files up to {} KB can be attached", bytes.len() / 1024, TEXT_MAX_BYTES / 1024));
    }
    Ok(Prepared { name: name.to_string(), body: Body::Text(text), thumb: None })
}

/// A picture from raw RGBA pixels, such as a clipboard screenshot.
pub fn prepare_rgba(name: &str, width: u32, height: u32, rgba: Vec<u8>) -> Result<Prepared, String> {
    let img = RgbaImage::from_raw(width, height, rgba).ok_or_else(|| "the clipboard picture was malformed".to_string())?;
    prepare_image(name, &DynamicImage::ImageRgba8(img), false)
}

fn prepare_image(name: &str, img: &DynamicImage, prefer_jpeg: bool) -> Result<Prepared, String> {
    let (mime, bytes, width, height) = encode_for_send(img, prefer_jpeg).map_err(|e| format!("{name}: {e}"))?;
    Ok(Prepared {
        name: name_for(name, mime),
        body: Body::Image { mime, bytes, width, height },
        thumb: Some(thumbnail(img)),
    })
}

/// A backtick fence longer than any run of backticks in `text`.
fn fence_for(text: &str) -> String {
    let longest = text.split(|c| c != '`').map(str::len).max().unwrap_or(0);
    "`".repeat((longest + 1).max(3))
}

/// A text file as a fenced block headed by its name.
pub fn inline_block(name: &str, text: &str) -> String {
    let fence = fence_for(text);
    let lang = name.rsplit_once('.').map(|(_, ext)| ext.to_ascii_lowercase()).filter(|e| e.chars().all(|c| c.is_ascii_alphanumeric())).unwrap_or_default();
    let body = text.trim_end_matches(['\r', '\n']);
    format!("**{name}**\n{fence}{lang}\n{body}\n{fence}")
}

/// A text file [`inline_block`] added to a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Inlined<'a> {
    pub name: &'a str,
    pub lang: &'a str,
    pub body: &'a str,
}

/// A message split into what was typed and the text files inlined after it; unsplit when they do not parse.
pub fn split_inlined(text: &str) -> (&str, Vec<Inlined<'_>>) {
    let start = if text.starts_with("**") { Some(0) } else { text.find("\n\n**").map(|i| i + 2) };
    let Some(start) = start else { return (text, Vec::new()) };
    let (typed, mut rest) = text.split_at(start);
    let mut files = Vec::new();
    loop {
        let Some((file, after)) = parse_inlined(rest) else { return (text, Vec::new()) };
        files.push(file);
        let after = after.trim_start_matches('\n');
        if after.trim().is_empty() {
            return (typed.trim_end(), files);
        }
        rest = after;
    }
}

/// One `**name**` heading and its fenced body, and the text after the closing fence.
fn parse_inlined(s: &str) -> Option<(Inlined<'_>, &str)> {
    let (name, s) = s.strip_prefix("**")?.split_once("**\n")?;
    if name.trim().is_empty() || name.contains(['\n', '*']) {
        return None;
    }
    let (open, s) = s.split_once('\n')?;
    let ticks = open.len() - open.trim_start_matches('`').len();
    if ticks < 3 {
        return None;
    }
    let (fence, lang) = open.split_at(ticks);
    let close = if s.starts_with(fence) && s[ticks..].chars().next().is_none_or(|c| c == '\n') {
        (0, 0)
    } else {
        let needle = format!("\n{fence}");
        let at = s.match_indices(&needle).map(|(i, _)| i).find(|i| s[i + needle.len()..].chars().next().is_none_or(|c| c == '\n'))?;
        (at, 1)
    };
    let (at, newline) = close;
    Some((Inlined { name, lang, body: &s[..at] }, &s[at + newline + ticks..]))
}

/// The message text with every text attachment inlined, and the pictures as turn images.
pub fn compose(text: &str, attachments: &[Attachment]) -> (String, Vec<TurnImage>) {
    let mut out = text.trim().to_string();
    let mut images = Vec::new();
    for a in attachments {
        match &a.body {
            Body::Text(content) => {
                if !out.is_empty() {
                    out.push_str("\n\n");
                }
                out.push_str(&inline_block(&a.name, content));
            }
            Body::Image { mime, bytes, .. } => images.push(TurnImage {
                name: a.name.clone(),
                mime: (*mime).to_string(),
                data: Some(base64::engine::general_purpose::STANDARD.encode(bytes)),
                path: None,
            }),
        }
    }
    (out, images)
}

/// Keeps the thumbnails of pictures about to be sent and returns the file names the broker stages them as.
pub fn remember_sent(attachments: &[Attachment]) -> Vec<String> {
    let staged: Vec<(String, Option<&TextureHandle>)> = attachments
        .iter()
        .filter_map(|a| match &a.body {
            Body::Image { bytes, .. } => Some((upload_name(&a.name, bytes), a.thumb.as_ref())),
            Body::Text(_) => None,
        })
        .collect();
    if let Ok(mut sent) = SENT.lock() {
        for (name, thumb) in &staged {
            let Some(thumb) = thumb else { continue };
            sent.retain(|(kept, _)| kept != name);
            sent.push((name.clone(), (*thumb).clone()));
        }
        let excess = sent.len().saturating_sub(SENT_KEPT);
        sent.drain(..excess);
    }
    staged.into_iter().map(|(name, _)| name).collect()
}

/// The thumbnail of a picture this app sent, by its staged file name.
pub fn sent_thumb(file_name: &str) -> Option<TextureHandle> {
    SENT.lock().ok()?.iter().find(|(name, _)| name == file_name).map(|(_, t)| t.clone())
}

static SENT: LazyLock<Mutex<Vec<(String, TextureHandle)>>> = LazyLock::new(Mutex::default);

/// File names of the pictures a stored `userMessage` item carries.
pub fn image_names(item: Option<&serde_json::Value>) -> Vec<String> {
    let Some(content) = item.and_then(|i| i.get("content")).and_then(serde_json::Value::as_array) else {
        return Vec::new();
    };
    content
        .iter()
        .filter(|c| c.get("type").and_then(serde_json::Value::as_str) == Some("localImage"))
        .filter_map(|c| c.get("path").and_then(serde_json::Value::as_str))
        .map(|p| p.rsplit(['/', '\\']).next().unwrap_or(p).to_string())
        .collect()
}

/// Reads `bytes` off the UI thread where there is one, then reports to `tx` and repaints.
pub fn spawn_prepare(ctx: &egui::Context, tx: &Sender<AttachEvent>, name: String, bytes: Vec<u8>) {
    run_off_thread(ctx, tx, move || prepare(&name, &bytes));
}

fn run_off_thread(ctx: &egui::Context, tx: &Sender<AttachEvent>, work: impl FnOnce() -> Result<Prepared, String> + Send + 'static) {
    let _ = tx.send(AttachEvent::Started);
    let (ctx, tx) = (ctx.clone(), tx.clone());
    let finish = move || {
        let event = match work() {
            Ok(p) => AttachEvent::Ready(p),
            Err(e) => AttachEvent::Refused(e),
        };
        let _ = tx.send(event);
        ctx.request_repaint();
    };
    #[cfg(not(target_arch = "wasm32"))]
    {
        if let Err(e) = std::thread::Builder::new().name("chat-attach".into()).spawn(finish) {
            log::warn!("attachment worker failed to start: {e}");
        }
    }
    #[cfg(target_arch = "wasm32")]
    finish();
}

/// Opens the file dialog and reads each picked file as an attachment.
#[cfg(not(any(target_os = "ios", target_os = "android")))]
pub fn pick_files(ctx: &egui::Context, tx: &Sender<AttachEvent>) {
    use crate::{PlatformSpawner, Spawner};
    let (ctx, tx) = (ctx.clone(), tx.clone());
    PlatformSpawner::spawn(async move {
        let Some(files) = rfd::AsyncFileDialog::new().set_title("Attach files").pick_files().await else { return };
        for file in files {
            let name = file.file_name();
            let bytes = file.read().await;
            spawn_prepare(&ctx, &tx, name, bytes);
        }
    });
}

/// Reads the clipboard's picture, if any, as an attachment.
#[cfg(not(target_arch = "wasm32"))]
pub fn paste_image(ctx: &egui::Context, tx: &Sender<AttachEvent>) {
    use crate::tabs::admin_console::client_interface::clipboard_bridge;
    run_off_thread(ctx, tx, || {
        let img = clipboard_bridge::read_image(std::time::Duration::from_secs(3)).ok_or_else(String::new)?;
        let (w, h) = (u32::try_from(img.width).map_err(|e| e.to_string())?, u32::try_from(img.height).map_err(|e| e.to_string())?);
        prepare_rgba(&pasted_name(), w, h, img.rgba)
    });
}

/// A name for a pasted picture that carries none.
fn pasted_name() -> String {
    format!("pasted-{}.png", chrono::Local::now().format("%Y%m%d-%H%M%S"))
}

/// Takes files dropped on the window and reads each as an attachment.
pub fn take_dropped(ctx: &egui::Context, tx: &Sender<AttachEvent>) {
    let dropped = ctx.input_mut(|i| std::mem::take(&mut i.raw.dropped_files));
    for file in dropped {
        let name = file
            .path()
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "file".to_string());
        #[cfg(not(target_arch = "wasm32"))]
        {
            let (ctx, tx) = (ctx.clone(), tx.clone());
            run_off_thread(&ctx, &tx, move || {
                let bytes = file.bytes().map_err(|e| format!("{name} could not be read: {e}"))?;
                prepare(&name, &bytes)
            });
        }
        #[cfg(target_arch = "wasm32")]
        {
            use crate::{PlatformSpawner, Spawner};
            let (ctx, tx) = (ctx.clone(), tx.clone());
            PlatformSpawner::spawn(async move {
                match file.bytes_async().await {
                    Ok(bytes) => spawn_prepare(&ctx, &tx, name, bytes),
                    Err(e) => {
                        let _ = tx.send(AttachEvent::Refused(format!("{name} could not be read: {e}")));
                    }
                }
            });
        }
    }
}

/// Browser paste events that carried files, read by the composer that has focus.
#[cfg(target_arch = "wasm32")]
pub mod web_paste {
    use std::cell::RefCell;

    use eframe::egui;
    use wasm_bindgen::closure::Closure;
    use wasm_bindgen::JsCast;

    /// Pasted files older than this are dropped unread.
    const FRESH_FOR: f64 = 2_000.0;

    thread_local! {
        static INSTALLED: RefCell<bool> = const { RefCell::new(false) };
        static PASTED: RefCell<Vec<(f64, String, Vec<u8>)>> = const { RefCell::new(Vec::new()) };
    }

    /// Listens for paste events on the window, once per page.
    pub fn install(ctx: &egui::Context) {
        if INSTALLED.with(|i| i.replace(true)) {
            return;
        }
        let Some(window) = web_sys::window() else { return };
        let ctx = ctx.clone();
        let listener = Closure::<dyn FnMut(web_sys::ClipboardEvent)>::new(move |event: web_sys::ClipboardEvent| {
            let Some(files) = event.clipboard_data().and_then(|d| d.files()) else { return };
            for i in 0..files.length() {
                let Some(file) = files.get(i) else { continue };
                let ctx = ctx.clone();
                wasm_bindgen_futures::spawn_local(async move {
                    let Ok(buffer) = wasm_bindgen_futures::JsFuture::from(file.array_buffer()).await else { return };
                    let bytes = js_sys::Uint8Array::new(&buffer).to_vec();
                    let name = file.name();
                    let name = if name.trim().is_empty() { super::pasted_name() } else { name };
                    PASTED.with(|p| p.borrow_mut().push((js_sys::Date::now(), name, bytes)));
                    ctx.request_repaint();
                });
            }
        });
        let _ = window.add_event_listener_with_callback_and_bool("paste", listener.as_ref().unchecked_ref(), true);
        listener.forget();
    }

    /// Files pasted in the last moment, handed out once.
    pub fn take() -> Vec<(String, Vec<u8>)> {
        let now = js_sys::Date::now();
        PASTED.with(|p| p.borrow_mut().drain(..).filter(|(at, ..)| now - at < FRESH_FOR).map(|(_, n, b)| (n, b)).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn picture(w: u32, h: u32, noisy: bool) -> DynamicImage {
        let mut seed = 0x2545_f491_u32;
        DynamicImage::ImageRgba8(RgbaImage::from_fn(w, h, |x, y| {
            if noisy {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                let b = seed.to_le_bytes();
                image::Rgba([b[0], b[1], b[2], 255])
            } else {
                image::Rgba([(x % 256) as u8, (y % 256) as u8, 90, 255])
            }
        }))
    }

    #[test]
    fn pictures_shrink_to_the_longest_side_and_keep_their_shape() {
        assert_eq!(fit_within(3840, 2160, 1600), (1600, 900));
        assert_eq!(fit_within(1080, 2400, 1600), (720, 1600));
        assert_eq!(fit_within(800, 600, 1600), (800, 600), "a small picture is never enlarged");
        assert_eq!(fit_within(10_000, 1, 1600), (1600, 1));
        assert_eq!(fit_within(0, 0, 1600), (1, 1));
    }

    #[test]
    fn a_large_screenshot_is_sent_within_the_side_and_size_caps() {
        let (mime, bytes, w, h) = encode_for_send(&picture(2400, 1350, false), false).expect("encodes");
        assert_eq!((w, h), (1600, 900));
        assert!(bytes.len() <= IMAGE_MAX_BYTES, "{} bytes", bytes.len());
        assert_eq!(image::guess_format(&bytes).ok(), Some(if mime == "image/png" { ImageFormat::Png } else { ImageFormat::Jpeg }));
    }

    #[test]
    fn noise_that_no_png_can_hold_falls_back_to_jpeg_under_the_cap() {
        let (mime, bytes, w, _) = encode_for_send(&picture(1600, 1600, true), false).expect("encodes");
        assert_eq!(mime, "image/jpeg");
        assert!(bytes.len() <= IMAGE_MAX_BYTES && w <= IMAGE_MAX_SIDE);
    }

    #[test]
    fn a_picture_file_is_prepared_with_a_thumbnail_and_a_matching_name() {
        let png = encode_png(&picture(300, 200, false)).expect("png");
        let p = prepare("shot.bmp", &png).expect("a picture");
        assert_eq!(p.name, "shot.png");
        assert!(matches!(p.body, Body::Image { mime: "image/png", width: 300, height: 200, .. }));
        let thumb = p.thumb.expect("a thumbnail");
        assert_eq!(thumb.size, [160, 107]);
    }

    #[test]
    fn text_files_are_read_in_utf8_or_utf16_and_binaries_are_refused() {
        assert_eq!(decode_text("héllo".as_bytes()).as_deref(), Some("héllo"));
        assert_eq!(decode_text(b"\xEF\xBB\xBFbom").as_deref(), Some("bom"));
        let le: Vec<u8> = [0xFF, 0xFE].into_iter().chain("REGEDIT4".encode_utf16().flat_map(u16::to_le_bytes)).collect();
        assert_eq!(decode_text(&le).as_deref(), Some("REGEDIT4"));
        assert_eq!(decode_text(b"MZ\x90\x00\x03"), None);
        assert_eq!(decode_text(&[0xC3, 0x28]), None);
        assert!(prepare("app.exe", b"MZ\x90\x00\x03\x00").is_err());
        assert!(prepare("big.log", &vec![b'a'; TEXT_MAX_BYTES + 1]).is_err());
        assert!(matches!(prepare("ok.log", b"line").map(|p| p.body), Ok(Body::Text(t)) if t == "line"));
    }

    #[test]
    fn a_text_file_is_inlined_under_its_name_in_a_fence_it_cannot_close() {
        assert_eq!(inline_block("setup.log", "ok\n"), "**setup.log**\n```log\nok\n```");
        let tricky = "before\n```\nnested\n````\nafter";
        let block = inline_block("notes.md", tricky);
        assert!(block.starts_with("**notes.md**\n`````md\n"), "{block}");
        assert!(block.ends_with("\n`````"));
        assert_eq!(inline_block("README", "x"), "**README**\n```\nx\n```");
    }

    #[test]
    fn composing_puts_text_files_after_the_message_and_pictures_on_the_turn() {
        let text = Attachment { name: "a.txt".into(), body: Body::Text("alpha".into()), thumb: None };
        let pic = Attachment { name: "p.png".into(), body: Body::Image { mime: "image/png", bytes: vec![1, 2, 3], width: 1, height: 1 }, thumb: None };
        let (message, images) = compose("  look at these ", &[text, pic]);
        assert_eq!(message, "look at these\n\n**a.txt**\n```txt\nalpha\n```");
        assert_eq!(images.len(), 1);
        assert_eq!(images[0].data.as_deref(), Some("AQID"));
        assert_eq!((images[0].name.as_str(), images[0].mime.as_str()), ("p.png", "image/png"));
    }

    #[test]
    fn inlined_files_split_back_out_of_a_sent_message() {
        let a = Attachment { name: "setup.log".into(), body: Body::Text("line 1\n```\nline 3".into()), thumb: None };
        let b = Attachment { name: "empty.txt".into(), body: Body::Text(String::new()), thumb: None };
        let (message, _) = compose("why did this fail?", &[a, b]);
        let (typed, files) = split_inlined(&message);
        assert_eq!(typed, "why did this fail?");
        assert_eq!(files.len(), 2);
        assert_eq!((files[0].name, files[0].lang, files[0].body), ("setup.log", "log", "line 1\n```\nline 3"));
        assert_eq!((files[1].name, files[1].body), ("empty.txt", ""));

        let only = compose("", &[Attachment { name: "a.json".into(), body: Body::Text("{}".into()), thumb: None }]).0;
        assert_eq!(split_inlined(&only), ("", vec![Inlined { name: "a.json", lang: "json", body: "{}" }]));
    }

    #[test]
    fn a_message_without_inlined_files_stays_whole() {
        for text in ["plain", "**bold** words", "see\n\n**Note**\nnot a fence", "a\n\n**x**\n```\nunclosed"] {
            assert_eq!(split_inlined(text), (text, Vec::new()), "{text}");
        }
    }

    #[test]
    fn sent_picture_names_come_from_local_image_paths() {
        let item = serde_json::json!({ "content": [
            { "type": "localImage", "path": "/tmp/zc-codexd-uploads/shot-0a1b2c3d.png" },
            { "type": "text", "text": "what is this?" },
            { "type": "image", "url": "[image/png omitted, 9 bytes]" }
        ]});
        assert_eq!(image_names(Some(&item)), vec!["shot-0a1b2c3d.png".to_string()]);
        assert!(image_names(None).is_empty());
    }
}
