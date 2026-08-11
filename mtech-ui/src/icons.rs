//! Phosphor icon glyphs for egui.

pub use egui_phosphor::regular as p;

use eframe::egui::{Color32, FontDefinitions, FontFamily, RichText};

pub fn install_fonts(fonts: &mut FontDefinitions) {
    egui_phosphor::add_to_fonts(fonts, egui_phosphor::Variant::Regular);

    // add_to_fonts only registers phosphor under Proportional; themes force Monospace for most
    // text, so register it under Monospace (and the named families) too or icons render as tofu.
    for family in [
        FontFamily::Monospace,
        FontFamily::Name("Regular".into()),
        FontFamily::Name("Bold".into()),
    ] {
        if let Some(keys) = fonts.families.get_mut(&family) {
            if !keys.iter().any(|k| k == "phosphor") {
                keys.insert(1.min(keys.len()), "phosphor".into());
            }
        }
    }
}

pub fn icon(glyph: &str) -> RichText {
    RichText::new(glyph).family(FontFamily::Proportional)
}

pub fn icon_sized(glyph: &str, size: f32) -> RichText {
    RichText::new(glyph).size(size).family(FontFamily::Proportional)
}

pub fn icon_colored(glyph: &str, color: Color32) -> RichText {
    icon(glyph).color(color)
}

pub fn menu_label(name: &str) -> String {
    format!("{name} {}", p::CARET_DOWN)
}

pub fn menu_item(icon_glyph: &str, label: &str) -> String {
    format!("{icon_glyph} {label}")
}

pub const MENU_SUFFIX: &str = p::CARET_DOWN;
pub const CHEV_OPEN: &str = p::CARET_DOWN;
pub const CHEV_CLOSED: &str = p::CARET_RIGHT;

pub const STOP: &str = p::STOP;
pub const PLAY: &str = p::PLAY;
pub const PAUSE: &str = p::PAUSE;

pub const POPOUT: &str = p::ARROW_SQUARE_OUT;
pub const FULLSCREEN_ENTER: &str = p::CORNERS_OUT;
pub const FULLSCREEN_EXIT: &str = p::CORNERS_IN;

pub const UP: &str = p::ARROW_UP;
pub const REFRESH: &str = p::ARROW_CLOCKWISE;
pub const HOME: &str = p::HOUSE;

pub const STATUS_ON: &str = p::CHECK_CIRCLE;
pub const STATUS_READY: &str = p::CHECK;
pub const STATUS_WARN: &str = p::WARNING;
pub const STATUS_ERR: &str = p::X;
pub const STATUS_OFF: &str = p::MINUS_CIRCLE;
pub const STATUS_DISABLED: &str = p::PROHIBIT;
pub const STATUS_IDLE: &str = p::CIRCLE;
pub const STATUS_WAIT: &str = p::HOURGLASS_SIMPLE;
pub const STATUS_QUEUED: &str = p::CLOCK;
pub const STATUS_DOT: &str = p::DOT;

pub const INFO: &str = p::INFO;
pub const CRITICAL: &str = p::SKULL;

pub const FOCUS: &str = p::CROSSHAIR;
pub const OPEN: &str = p::ARROWS_OUT_SIMPLE;
pub const CLOSE: &str = p::X;

/// Client-row actions, as icons with the action in hover text.
pub const DISCONNECT: &str = p::LINK_BREAK;
pub const FLOAT: &str = p::PICTURE_IN_PICTURE;
pub const DOCK: &str = p::ARROW_SQUARE_IN;
pub const RELINK: &str = p::USER_SWITCH;
pub const LINK_COMPUTER: &str = p::PLUGS_CONNECTED;
pub const REPAIR_LINKS: &str = p::WRENCH;
pub const AUTOPILOT: &str = p::ROBOT;

pub const BETA: &str = p::TEST_TUBE;
pub const BETA_TAG: &str = BETA;

pub const LOCK: &str = p::LOCK;
pub const POWER: &str = p::POWER;
pub const TERMINAL: &str = p::TERMINAL;
pub const DESKTOP: &str = p::DESKTOP;
pub const SIGN_OUT: &str = p::SIGN_OUT;

pub const FOLDER: &str = p::FOLDER;
pub const FOLDER_OPEN: &str = p::FOLDER_OPEN;
pub const FOLDER_PLUS: &str = p::FOLDER_PLUS;
pub const FILE: &str = p::FILE;
pub const FILE_TEXT: &str = p::FILE_TEXT;
pub const PACKAGE: &str = p::PACKAGE;

pub const DOWNLOAD: &str = p::DOWNLOAD;
pub const UPLOAD: &str = p::UPLOAD;
pub const TRASH: &str = p::TRASH;
pub const SAVE: &str = p::FLOPPY_DISK;
pub const EYE: &str = p::EYE;
pub const SEARCH: &str = p::MAGNIFYING_GLASS;
pub const GEAR: &str = p::GEAR;
pub const PLUS: &str = p::PLUS;
pub const EDIT: &str = p::PENCIL;
pub const COPY: &str = p::COPY;
pub const CLIPBOARD: &str = p::CLIPBOARD;
pub const TASK_CREATE: &str = p::CLIPBOARD;
pub const TASK_EXISTS: &str = p::CLIPBOARD_TEXT;
pub const LIST: &str = p::LIST;
pub const SCROLL: &str = p::SCROLL;
pub const CHART: &str = p::CHART_BAR;
pub const WRENCH: &str = p::WRENCH;
pub const LIGHTBULB: &str = p::LIGHTBULB;
pub const ROBOT: &str = p::ROBOT;
pub const STAR: &str = p::STAR;
pub const CHAT: &str = p::CHAT;
pub const BELL: &str = p::BELL;
pub const GAME: &str = p::GAME_CONTROLLER;
pub const FLASK: &str = p::FLASK;
pub const MONITOR: &str = p::MONITOR;
pub const BOOK: &str = p::BOOK;
pub const MUSIC: &str = p::MUSIC_NOTES;
pub const IMAGE: &str = p::IMAGE;
pub const VIDEO: &str = p::VIDEO;
pub const HARD_DRIVE: &str = p::HARD_DRIVE;
pub const DIAGNOSTICS: &str = p::MICROSCOPE;
pub const GRID: &str = p::SQUARES_FOUR;
pub const CARET_LEFT: &str = p::CARET_LEFT;
pub const CARET_RIGHT: &str = p::CARET_RIGHT;
pub const CARET_DOWN: &str = p::CARET_DOWN;
pub const ARROW_RIGHT: &str = p::ARROW_RIGHT;
pub const ARROW_DOWN: &str = p::ARROW_DOWN;
pub const CHECK: &str = p::CHECK;

pub fn folder_shortcut_icon(path: &str) -> &'static str {
    match path {
        "Desktop" => DESKTOP,
        "Documents" => FILE_TEXT,
        "Downloads" => DOWNLOAD,
        "Pictures" => IMAGE,
        "Music" => MUSIC,
        "Videos" => VIDEO,
        "AppData" => GEAR,
        "LocalAppData" => FOLDER,
        _ => FOLDER,
    }
}

pub fn file_icon(filename: &str, is_directory: bool) -> &'static str {
    if is_directory {
        return FOLDER;
    }

    let ext = filename.rsplit('.').next().unwrap_or("").to_lowercase();
    match ext.as_str() {
        "ps1" | "psm1" | "psd1" | "bat" | "cmd" | "sh" | "bash" | "zsh" => p::FILE_CODE,
        "js" | "ts" | "mjs" | "jsx" | "tsx" => p::FILE_JS,
        "py" | "pyw" => p::FILE_PY,
        "txt" | "log" => p::FILE_TXT,
        "md" | "rst" => p::FILE_MD,
        "doc" | "docx" => p::FILE_DOC,
        "pdf" => p::FILE_PDF,
        "xls" | "xlsx" => p::FILE_XLS,
        "csv" => p::FILE_CSV,
        "ppt" | "pptx" => p::FILE_PPT,
        "rs" => p::FILE_RS,
        "c" | "cpp" | "h" | "hpp" => p::FILE_CPP,
        "cs" | "go" | "java" | "kt" => p::FILE_CODE,
        "html" | "htm" => p::FILE_HTML,
        "css" | "scss" | "sass" => p::FILE_CSS,
        "json" | "yaml" | "yml" | "toml" | "xml" | "reg" | "ini" | "cfg" | "conf" | "config" => p::FILE_TEXT,
        "jpg" | "jpeg" => p::FILE_JPG,
        "png" => p::FILE_PNG,
        "gif" | "bmp" | "webp" | "ico" | "psd" | "ai" | "raw" | "arw" | "cr2" | "nef" | "dng" => p::FILE_IMAGE,
        "svg" => p::FILE_SVG,
        "mp3" | "wav" | "flac" | "ogg" | "m4a" => p::FILE_AUDIO,
        "mp4" | "mkv" | "avi" | "mov" | "wmv" | "webm" => p::FILE_VIDEO,
        "zip" | "rar" | "7z" | "tar" | "gz" | "bz2" | "iso" | "img" => p::FILE_ZIP,
        "exe" | "msi" | "dll" | "so" | "dylib" => p::FILE_ARCHIVE,
        "db" | "sqlite" | "mdb" | "sql" => p::DATABASE,
        "lnk" => p::LINK,
        _ => FILE,
    }
}
