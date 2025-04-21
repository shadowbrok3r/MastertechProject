// Import anyhow macro
use anyhow::{anyhow, Context}; use base64::Engine;
// *** Add display-info import ***
use display_info::DisplayInfo;
// *** Using enigo now ***
use enigo::{
    Button, Coordinate,
    Direction, // For key press/release/click actions
    Enigo, Key, Keyboard, Mouse, Settings, // Note: enigo::Mouse/Keyboard traits
};
use rmcp::transport::stdio;
// *** Added for wait tool ***
use tokio::time::{sleep, Duration};


// --- Specific rmcp Imports ---
use rmcp::schemars; // For deriving schema
use rmcp::handler::server::ServerHandler;
// use rmcp::transport::stdio;
use tokio::net::TcpListener; // Added TcpListener
// Import types needed for tool return values and ServerHandler impl
use rmcp::model::{
    // *** Added ErrorCode, ErrorData ***
    CallToolResult, Content, ErrorCode, ErrorData, Implementation, ProtocolVersion, ServerCapabilities,
    ServerInfo,
};
// Added serve_server back
// *** Ensure tool_box is imported ***
// Removed rmcp::tool_box from here as it's applied via attribute macro
use rmcp::{serve_server, tool}; // Keep McpError for type alias if needed internally

use serde::{Deserialize, Serialize};
use serde_json::json;
use std::io::Cursor;
use std::process::Command;
use tracing::{info, warn}; // Added warn

// --- Tool Parameter Struct Definitions ---

// Structs for existing custom tools
#[derive(Deserialize, Debug, Serialize, schemars::JsonSchema)]
struct GetScreenDetailsParams {
    #[schemars(description = "Ignored dummy field.")]
    _dummy: Option<bool>,
}
#[derive(Deserialize, Debug, Serialize, schemars::JsonSchema)]
struct GetMousePositionParams {
     #[schemars(description = "Ignored dummy field.")]
    _dummy: Option<bool>,
}
#[derive(Deserialize, Debug, Serialize, schemars::JsonSchema)]
struct MoveMouseParams {
    #[schemars(description = "Target X coordinate.")]
    x: i32,
    #[schemars(description = "Target Y coordinate.")]
    y: i32,
    #[schemars(description = "Type of mouse move ('Absolute'/'Abs' for absolute coordinates, 'Relative'/'Rel' for relative offset).")]
    coordinate: String
}
#[derive(Deserialize, Debug, Serialize, schemars::JsonSchema)]
struct MouseClickParams { // Renamed to avoid conflict, used by 'mouse_action' tool
    #[schemars(description = "Which mouse button/action ('Left', 'Right', 'Middle', 'Back', 'Forward', 'ScrollUp', 'ScrollDown', 'ScrollLeft', 'ScrollRight'). Case-insensitive.")]
    button: String,
    #[schemars(description = "Type of action ('Click', 'Press', 'Release'). Default is 'Click'. Double click not directly supported.", default)]
    click_type: Option<String>,
}
#[derive(Deserialize, Debug, Serialize, schemars::JsonSchema)]
struct KeyboardActionParams {
    #[schemars(description = "Optional: Text to type using enigo's text input method.")]
    text: Option<String>,
    #[schemars(description = "Optional: A specific key to press/release/click (e.g., 'a', 'Enter', 'Control', 'Shift', 'Alt', 'F5', 'PageDown'). Takes precedence over 'text' if both are provided.")]
    key: Option<String>,
    #[schemars(description = "Action for the specified 'key': 'Click' (default), 'Press', 'Release'. Ignored if 'text' is used.", default)]
    key_action: Option<String>,
}
#[derive(Deserialize, Debug, Serialize, schemars::JsonSchema)]
struct CaptureScreenParams {
    #[schemars(description = "Optional X coordinate of the top-left corner for regional capture.")]
    x: Option<i32>,
    #[schemars(description = "Optional Y coordinate of the top-left corner for regional capture.")]
    y: Option<i32>,
    #[schemars(description = "Optional width for regional capture.")]
    width: Option<u32>,
    #[schemars(description = "Optional height for regional capture.")]
    height: Option<u32>,
}
#[derive(Deserialize, Debug, Serialize, schemars::JsonSchema)]
struct RunShellParams {
    command: String,
    args: Vec<String>,
}

// --- Structs for NEW OpenAI Action Tools ---

#[derive(Deserialize, Debug, Serialize, schemars::JsonSchema)]
struct OpenAIClickParams {
    #[schemars(description = "X coordinate for the click.")]
    x: i32,
    #[schemars(description = "Y coordinate for the click.")]
    y: i32,
    #[schemars(description = "Button to click ('left', 'right', 'middle').")]
    button: String,
}

#[derive(Deserialize, Debug, Serialize, schemars::JsonSchema)]
struct OpenAIScrollParams {
     #[schemars(description = "X coordinate where scroll should originate.")]
    x: i32,
    #[schemars(description = "Y coordinate where scroll should originate.")]
    y: i32,
    #[schemars(description = "Pixels to scroll horizontally (positive right, negative left).")]
    scroll_x: i32,
    #[schemars(description = "Pixels to scroll vertically (positive down, negative up).")]
    scroll_y: i32,
}

#[derive(Deserialize, Debug, Serialize, schemars::JsonSchema)]
struct OpenAIKeyPressParams {
    #[schemars(description = "Array of key names to press sequentially (e.g., ['Control', 'c']). Mapping based on enigo::Key.")]
    keys: Vec<String>,
}

#[derive(Deserialize, Debug, Serialize, schemars::JsonSchema)]
struct OpenAITypeParams {
     #[schemars(description = "The text string to type.")]
    text: String,
}

#[derive(Deserialize, Debug, Serialize, schemars::JsonSchema)]
struct OpenAIWaitParams {
     #[schemars(description = "Optional duration in milliseconds to wait. Defaults to 2000ms if not provided.", default)]
    duration_ms: Option<u64>,
}

#[derive(Deserialize, Debug, Serialize, schemars::JsonSchema)]
struct FindWindowParams {
    #[schemars(description = "The title (or part of the title) of the window to find. Case-insensitive search.")]
    title_query: String,
}

// --- Tool Provider Implementation ---

#[derive(Clone)] // Clone is required by ServerHandler
struct DesktopToolProvider;

// *** First impl block: Contains the tool definitions ***
#[tool(tool_box)]// Apply tool_box here as well
impl DesktopToolProvider {
    // --- Existing Custom Tools (Unchanged) ---
    #[tool(name = "get_screen_details", description = "Gets the primary screen resolution (width and height).")]
    async fn get_screen_details(
        &self,
        #[tool(aggr)] _params: GetScreenDetailsParams // Use dummy struct with aggr
    ) -> Result<CallToolResult, ErrorData> {
        info!("Received request to get screen details.");
        let display_infos = DisplayInfo::all()
            .map_err(|e| anyhow!(e).context("display_info::DisplayInfo::all() failed"))
            .map_err(|e| ErrorData::new(ErrorCode::INTERNAL_ERROR, e.to_string(), None))?;

        let mut screens = vec![];

        for screen in display_infos.iter() {
            screens.push(
                json!({
                    "screen_id": screen.id,
                    "name": screen.name,
                    "width": screen.width,
                    "height": screen.height,
                    "scale_factor": screen.scale_factor,
                    "x": screen.x,
                    "y": screen.y
                })
            );
        }

        Ok(CallToolResult::success(
            vec![
                Content::json(screens)
                    .map_err(|e| anyhow!(e).context("Failed to serialize screen details to JSON"))
                    .map_err(|e| ErrorData::new(ErrorCode::INTERNAL_ERROR, e.to_string(), None))?
            ]
        ))
    }

    #[tool(name = "find_window", description = "Finds the first non-minimized window whose title contains the given query string (case-insensitive) and returns its details.")]
    async fn find_window(
        &self,
        #[tool(aggr)] params: FindWindowParams
    ) -> Result<CallToolResult, ErrorData> {
        info!("Executing find window with query: '{}'", params.title_query);

        let windows = xcap::Window::all()
            .context("Failed to get window list")
            .map_err(|e| ErrorData::new(ErrorCode::INTERNAL_ERROR, e.to_string(), None))?;

        let query_lower = params.title_query.to_lowercase();

        for window in windows {
            // Skip minimized windows
            let is_minimized = window.is_minimized()
                .unwrap_or(true); // Treat error as minimized to skip
            if is_minimized {
                continue;
            }

            // Get window title
            let title = match window.title() {
                 Ok(t) => t,
                 Err(_) => continue, // Skip windows where title cannot be retrieved
            };

            // Perform case-insensitive partial match
            if title.to_lowercase().contains(&query_lower) {
                let x = window.x().unwrap_or(0); // Provide default on error
                let y = window.y().unwrap_or(0);
                let width = window.width().unwrap_or(0);
                let height = window.height().unwrap_or(0);
                let app_name = window.app_name().unwrap_or_default(); // Get app name if available

                info!("Found matching window: Title='{}', App='{}', Pos=({}, {}), Size=({}x{})", title, app_name, x, y, width, height);

                let result_json = json!({
                    "status": "success",
                    "found": true,
                    "title": title,
                    "app_name": app_name,
                    "x": x,
                    "y": y,
                    "width": width,
                    "height": height,
                    "is_maximized": window.is_maximized().unwrap_or(false) // Include maximized state
                });

                return Ok(CallToolResult::success(vec![Content::json(result_json)
                    .map_err(|e| anyhow!(e).context("Failed to serialize find_window result"))
                    .map_err(|e| ErrorData::new(ErrorCode::INTERNAL_ERROR, e.to_string(), None))?
                ]));
            }
        }

        // If no window was found after checking all
        info!("No matching window found for query: '{}'", params.title_query);
        Ok(CallToolResult::success(vec![Content::json(json!({
            "status": "success", // Still a successful tool execution, just no result found
            "found": false,
            "message": format!("No non-minimized window found matching title query '{}'", params.title_query)
        }))
            .map_err(|e| anyhow!(e).context("Failed to serialize find_window 'not found' result"))
            .map_err(|e| ErrorData::new(ErrorCode::INTERNAL_ERROR, e.to_string(), None))?
        ]))
        // Alternatively, you could return an error:
        // Err(ErrorData::new(ErrorCode::NOT_FOUND, format!("No non-minimized window found matching title query '{}'", params.title_query), None))
    }

    #[tool(name = "move_mouse", description = "Moves the mouse cursor")]
    async fn move_mouse(
        &self,
        #[tool(aggr)] params: MoveMouseParams
    ) -> Result<CallToolResult, ErrorData> {
        info!("Executing move mouse to: {:?}", params);
        let mut enigo = Enigo::new(&Settings::default())
            .map_err(|e| ErrorData::new(ErrorCode::INTERNAL_ERROR, e.to_string(), None))?;

        let coordinate = match params.coordinate.to_lowercase().as_str() {
            "absolute" | "abs" => Coordinate::Abs,
            "relative" | "rel" | _ => Coordinate::Rel,
        };
        if coordinate == Coordinate::Rel { info!("Moving mouse relatively by ({}, {})", params.x, params.y); }
        else { info!("Moving mouse absolutely to ({}, {})", params.x, params.y); }

        enigo.move_mouse(params.x, params.y, coordinate)
            .map_err(|e| ErrorData::new(ErrorCode::INTERNAL_ERROR, format!("Couldnt move mouse: {e:?}"), None))?;

        let (x, y) = enigo.location().map_err(|e| ErrorData::new(ErrorCode::INTERNAL_ERROR, e.to_string(), None))?;
        info!("Mouse moved successfully.");
        Ok(CallToolResult::success(vec![Content::json(json!({ "status": "success", "current_x": x, "current_y": y }))
            .map_err(|e| anyhow!(e).context("Failed to serialize move_mouse result"))
            .map_err(|e| ErrorData::new(ErrorCode::INTERNAL_ERROR, e.to_string(), None))?
        ]))
    }

    #[tool(name = "get_mouse_position", description = "Gets the current absolute screen coordinates (X, Y) of the mouse cursor")]
    async fn get_mouse_position(
        &self,
        #[tool(aggr)] _params: GetMousePositionParams, // Use aggr with dummy struct
    ) -> Result<CallToolResult, ErrorData> {
        info!("Executing get mouse position.");
        let enigo = Enigo::new(&Settings::default())
            .map_err(|e| ErrorData::new(ErrorCode::INTERNAL_ERROR, e.to_string(), None))?;

        let (x, y) = enigo.location().map_err(|e| ErrorData::new(ErrorCode::INTERNAL_ERROR, e.to_string(), None))?;
        info!("Mouse position retrieved successfully: ({}, {})", x, y);
        let result_json = json!({ "status": "success", "x": x, "y": y });
        Ok(CallToolResult::success(vec![Content::json(result_json)
            .map_err(|e| anyhow!(e).context("Failed to serialize get_mouse_position result"))
            .map_err(|e| ErrorData::new(ErrorCode::INTERNAL_ERROR, e.to_string(), None))?
        ]))
    }

    #[tool(name = "mouse_action", description = "Performs a mouse action (click, press, release) or scrolls the mouse wheel")]
    async fn mouse_action(
        &self,
        #[tool(aggr)] params: MouseClickParams
    ) -> Result<CallToolResult, ErrorData> {
        info!("Executing mouse action: {:?}", params);
        let mut enigo = Enigo::new(&Settings::default())
            .map_err(|e| ErrorData::new(ErrorCode::INTERNAL_ERROR, e.to_string(), None))?;

        let button_str = params.button.to_lowercase();
        let action_str = params.click_type.as_deref().unwrap_or("click").to_lowercase();

        let direction = match action_str.as_str() {
            "click" => Direction::Click, "press" => Direction::Press, "release" => Direction::Release,
            "double" => { warn!("Double click not directly supported by enigo, performing single click instead."); Direction::Click }
            _ => { warn!("Invalid click_type '{}', defaulting to Click.", action_str); Direction::Click }
        };

        let button_enum = match button_str.as_str() {
            "left" => Button::Left, "right" => Button::Right, "middle" => Button::Middle,
            "back" => Button::Back, "forward" => Button::Forward,
            "scrollup" | "scroll_up" => Button::ScrollUp,
            "scrolldown" | "scroll_down" => Button::ScrollDown,
            "scrollleft" | "scroll_left" => Button::ScrollLeft,
            "scrollright" | "scroll_right" => Button::ScrollRight,
            _ => return Err(ErrorData::invalid_params( format!("Invalid mouse button/action specified: '{}'.", params.button), None)),
        };

        enigo.button(button_enum, direction).map_err(|e| ErrorData::new(ErrorCode::INTERNAL_ERROR, e.to_string(), None))?;
        info!("Mouse action successful: Button='{}', Action='{:?}'", button_str, direction);
        Ok(CallToolResult::success(vec![Content::json(json!({ "status": "success", "button": button_str, "action": action_str }))
            .map_err(|e| anyhow!(e).context("Failed to serialize mouse_action result"))
            .map_err(|e| ErrorData::new(ErrorCode::INTERNAL_ERROR, e.to_string(), None))?
        ]))
    }

    #[tool(name = "keyboard_action", description = "Types text or performs a key event (click, press, release)")]
    async fn keyboard_action(
        &self,
        #[tool(aggr)] params: KeyboardActionParams
    ) -> Result<CallToolResult, ErrorData> {
        info!("Executing keyboard action: {:?}", params);
        let mut enigo = Enigo::new(&Settings::default())
             .map_err(|e| ErrorData::new(ErrorCode::INTERNAL_ERROR, e.to_string(), None))?;

        if let Some(key_str) = &params.key {
            let action_str = params.key_action.as_deref().unwrap_or("click").to_lowercase();
            info!("Performing key action: key='{}', action='{}'", key_str, action_str);
            let direction = match action_str.as_str() {
                "click" => Direction::Click, "press" => Direction::Press, "release" => Direction::Release,
                 _ => { warn!("Invalid key_action '{}', defaulting to Click.", action_str); Direction::Click }
            };
            let key_enum = match key_str.to_lowercase().as_str() {
                "alt" | "altgraph" => Key::Alt, "backspace" => Key::Backspace, "capslock" | "caps_lock" => Key::CapsLock,
                "control" | "ctrl" => Key::Control, "delete" => Key::Delete, "down" | "downarrow" => Key::DownArrow,
                "end" => Key::End, "escape" | "esc" => Key::Escape,
                "f1" => Key::F1, "f2" => Key::F2, "f3" => Key::F3, "f4" => Key::F4, "f5" => Key::F5,
                "f6" => Key::F6, "f7" => Key::F7, "f8" => Key::F8, "f9" => Key::F9, "f10" => Key::F10,
                "f11" => Key::F11, "f12" => Key::F12, "home" => Key::Home, "left" | "leftarrow" => Key::LeftArrow,
                "meta" | "win" | "command" | "super" | "windows" => Key::Meta, "option" => Key::Option,
                "pagedown" | "page_down" => Key::PageDown, "pageup" | "page_up" => Key::PageUp,
                "return" | "enter" => Key::Return, "right" | "rightarrow" => Key::RightArrow,
                "shift" => Key::Shift, "space" => Key::Space, "tab" => Key::Tab, "up" | "uparrow" => Key::UpArrow,
                s if s.chars().count() == 1 => Key::Unicode(s.chars().next().unwrap()),
                _ => return Err(ErrorData::invalid_params( format!("Unsupported key specified: '{}'.", key_str), None)),
            };
            enigo.key(key_enum, direction).map_err(|e| ErrorData::new(ErrorCode::INTERNAL_ERROR, e.to_string(), None))?;
            info!("Key action successful.");
            Ok(CallToolResult::success(vec![Content::json(json!({ "status": "success", "key": key_str, "action": action_str }))
                .map_err(|e| anyhow!(e).context("Failed to serialize keyboard key action result"))
                .map_err(|e| ErrorData::new(ErrorCode::INTERNAL_ERROR, e.to_string(), None))?
            ]))
        } else if let Some(text_to_type) = &params.text {
            info!("Typing text: '{}'", text_to_type);
            enigo.text(text_to_type).map_err(|e| ErrorData::new(ErrorCode::INTERNAL_ERROR, e.to_string(), None))?;
            info!("Text typing successful.");
            Ok(CallToolResult::success(vec![Content::json(json!({ "status": "success", "text_typed": text_to_type }))
                .map_err(|e| anyhow!(e).context("Failed to serialize keyboard text typing result"))
                .map_err(|e| ErrorData::new(ErrorCode::INTERNAL_ERROR, e.to_string(), None))?
            ]))
        } else {
            Err(ErrorData::invalid_params("Keyboard action requires either 'key' or 'text' parameter.".to_string(), None))
        }
    }

    #[tool(name = "capture_screen", description = "Captures the screen (or a region) and returns image data as base64.")]
    async fn capture_screen(
        &self,
        #[tool(aggr)] params: CaptureScreenParams
    ) -> Result<CallToolResult, ErrorData> {
        info!("Executing screen capture with params: {:?}", params);
        let screens =  xcap::Monitor::all()
            .context("Failed to get screen list")
            .map_err(|e| ErrorData::new(ErrorCode::INTERNAL_ERROR, e.to_string(), None))?;
        let screen_to_capture = screens.first()
            .ok_or_else(|| anyhow!("No screen found to capture"))
            .map_err(|e| ErrorData::new(ErrorCode::INTERNAL_ERROR, e.to_string(), None))?;
        info!("Capturing from screen ID: {:?}", screen_to_capture.id());
        let image = screen_to_capture
            .capture_image()
            .context("Failed to capture screen area")
            .map_err(|e| ErrorData::new(ErrorCode::INTERNAL_ERROR, e.to_string(), None))?;

        info!("Capture successful ({}x{})", image.width(), image.height());
        let mut buf: Vec<u8> = Vec::new();
        image.write_to(&mut Cursor::new(&mut buf), image::ImageFormat::Png).map_err(|e| ErrorData::new(ErrorCode::INTERNAL_ERROR, e.to_string(), None))?;
        let base64_image = base64::engine::general_purpose::STANDARD.encode(&buf);
        info!("Encoded image to base64 (length: {})", base64_image.len());
        let result_json = json!({
            "status": "success", "format": "png", "width": image.width(), "height": image.height(), "base64_data": base64_image,
        });
        Ok(CallToolResult::success(vec![Content::json(result_json)
            .map_err(|e| anyhow!(e).context("Failed to serialize capture_screen result"))
            .map_err(|e| ErrorData::new(ErrorCode::INTERNAL_ERROR, e.to_string(), None))?
        ]))
    }

    #[tool(name = "run_shell_command", description = "Runs a command in the default system shell.")]
     async fn run_shell_command(
        &self,
        #[tool(aggr)] params: RunShellParams
    ) -> Result<CallToolResult, ErrorData> {
        info!("Received request to run command: {:?}", params);
        let _ = Command::new(&params.command)
            .args(&params.args)
            .spawn()
            .context(format!("Failed to execute command: {}", params.command))
            .map_err(|e| ErrorData::new(ErrorCode::INTERNAL_ERROR, e.to_string(), None))?;
        // let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        // let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        // let exit_code = output.status.code().unwrap_or(-1);
        // info!( "Command '{}' executed. Status: {}, Stdout len: {}, Stderr len: {}", params.command, exit_code, stdout.len(), stderr.len());
        let result_json = json!({ "status": "success"  }); // , "exit_code": exit_code, "stdout": stdout, "stderr": stderr,
        Ok(CallToolResult::success(vec![Content::json(result_json)
             .map_err(|e| anyhow!(e).context("Failed to serialize run_shell_command result"))
             .map_err(|e| ErrorData::new(ErrorCode::INTERNAL_ERROR, e.to_string(), None))?
        ]))
    }

    /*
    // --- NEW Tools for OpenAI Computer Use Actions ---
    #[tool(name = "execute_openai_click", description = "Executes a mouse click action requested by the OpenAI Computer Use model.")]
    async fn execute_openai_click(
        &self,
        #[tool(aggr)] params: OpenAIClickParams
    ) -> Result<CallToolResult, ErrorData> {
        info!("Executing OpenAI action: click at ({}, {}) with button '{}'", params.x, params.y, params.button);
        let mut enigo = Enigo::new(&Settings::default())
            .map_err(|e| ErrorData::new(ErrorCode::INTERNAL_ERROR, e.to_string(), None))?;

        // Move mouse first
        enigo.move_mouse(params.x, params.y, Coordinate::Abs)
            .map_err(|e| ErrorData::new(ErrorCode::INTERNAL_ERROR, format!("OpenAI Click: Failed to move mouse: {e:?}"), None))?;

        // Determine button
        let button_enum = match params.button.to_lowercase().as_str() {
            "left" => Button::Left,
            "right" => Button::Right,
            "middle" => Button::Middle,
            _ => return Err(ErrorData::invalid_params(format!("OpenAI Click: Invalid button '{}'", params.button), None)),
        };

        // Perform click
        enigo.button(button_enum, Direction::Click)
            .map_err(|e| ErrorData::new(ErrorCode::INTERNAL_ERROR, format!("OpenAI Click: Failed to click button: {e:?}"), None))?;

        Ok(CallToolResult::success(vec![Content::json(json!({ "status": "success" }))
            .map_err(|e| anyhow!(e).context("Failed to serialize execute_openai_click result"))
            .map_err(|e| ErrorData::new(ErrorCode::INTERNAL_ERROR, e.to_string(), None))?
        ]))
    }

    #[tool(name = "execute_openai_scroll", description = "Executes a mouse scroll action requested by the OpenAI Computer Use model.")]
    async fn execute_openai_scroll(
        &self,
        #[tool(aggr)] params: OpenAIScrollParams
    ) -> Result<CallToolResult, ErrorData> {
        info!("Executing OpenAI action: scroll at ({}, {}) with delta ({}, {})", params.x, params.y, params.scroll_x, params.scroll_y);
        let mut enigo = Enigo::new(&Settings::default())
            .map_err(|e| ErrorData::new(ErrorCode::INTERNAL_ERROR, e.to_string(), None))?;

        // Move mouse to scroll origin first
        enigo.move_mouse(params.x, params.y, Coordinate::Abs)
             .map_err(|e| ErrorData::new(ErrorCode::INTERNAL_ERROR, format!("OpenAI Scroll: Failed to move mouse: {e:?}"), None))?;

        // Perform scroll - enigo uses Button enum for scroll direction
        // Note: This scrolls once per direction. Magnitude requires looping.
        if params.scroll_y != 0 {
            let button = if params.scroll_y < 0 { Button::ScrollUp } else { Button::ScrollDown };
            let count = params.scroll_y.abs();
            info!("Scrolling vertically: {:?} {} times", button, count);
            for _ in 0..count { // Loop for magnitude
                 enigo.button(button, Direction::Click)
                    .map_err(|e| ErrorData::new(ErrorCode::INTERNAL_ERROR, format!("OpenAI Scroll: Failed vertical scroll: {e:?}"), None))?;
                 // Optional small delay between scroll clicks might be needed
                 // tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }
        if params.scroll_x != 0 {
             let button = if params.scroll_x < 0 { Button::ScrollLeft } else { Button::ScrollRight };
             let count = params.scroll_x.abs();
             info!("Scrolling horizontally: {:?} {} times", button, count);
             for _ in 0..count { // Loop for magnitude
                 enigo.button(button, Direction::Click)
                    .map_err(|e| ErrorData::new(ErrorCode::INTERNAL_ERROR, format!("OpenAI Scroll: Failed horizontal scroll: {e:?}"), None))?;
                 // Optional small delay
                 // tokio::time::sleep(Duration::from_millis(10)).await;
             }
        }

        Ok(CallToolResult::success(vec![Content::json(json!({ "status": "success" }))
            .map_err(|e| anyhow!(e).context("Failed to serialize execute_openai_scroll result"))
            .map_err(|e| ErrorData::new(ErrorCode::INTERNAL_ERROR, e.to_string(), None))?
        ]))
    }

     #[tool(name = "execute_openai_keypress", description = "Executes key presses requested by the OpenAI Computer Use model.")]
    async fn execute_openai_keypress(
        &self,
        #[tool(aggr)] params: OpenAIKeyPressParams
    ) -> Result<CallToolResult, ErrorData> {
        info!("Executing OpenAI action: keypress sequence: {:?}", params.keys);
        let mut enigo = Enigo::new(&Settings::default())
             .map_err(|e| ErrorData::new(ErrorCode::INTERNAL_ERROR, e.to_string(), None))?;

        // OpenAI keypress action sends an array of keys to be pressed sequentially (like modifiers + key)
        // We simulate this by pressing down all keys then releasing them in reverse.
        // This might need refinement based on observed OpenAI behavior.
        let mut key_enums = Vec::new();
        for key_str in &params.keys {
             let key_enum = match key_str.to_lowercase().as_str() {
                "alt" | "altgraph" => Key::Alt, "backspace" => Key::Backspace, "capslock" | "caps_lock" => Key::CapsLock,
                "control" | "ctrl" => Key::Control, "delete" => Key::Delete, "down" | "downarrow" => Key::DownArrow,
                "end" => Key::End, "escape" | "esc" => Key::Escape,
                "f1" => Key::F1, "f2" => Key::F2, "f3" => Key::F3, "f4" => Key::F4, "f5" => Key::F5,
                "f6" => Key::F6, "f7" => Key::F7, "f8" => Key::F8, "f9" => Key::F9, "f10" => Key::F10,
                "f11" => Key::F11, "f12" => Key::F12, "home" => Key::Home, "left" | "leftarrow" => Key::LeftArrow,
                "meta" | "win" | "command" | "super" | "windows" => Key::Meta, "option" => Key::Option,
                "pagedown" | "page_down" => Key::PageDown, "pageup" | "page_up" => Key::PageUp,
                "return" | "enter" => Key::Return, "right" | "rightarrow" => Key::RightArrow,
                "shift" => Key::Shift, "space" => Key::Space, "tab" => Key::Tab, "up" | "uparrow" => Key::UpArrow,
                s if s.chars().count() == 1 => Key::Unicode(s.chars().next().unwrap()),
                _ => return Err(ErrorData::invalid_params(format!("OpenAI Keypress: Unsupported key specified: '{}'.", key_str), None)),
            };
            key_enums.push(key_enum);
        }

        // Press keys down
        for key_enum in &key_enums {
             enigo.key(*key_enum, Direction::Press)
                  .map_err(|e| ErrorData::new(ErrorCode::INTERNAL_ERROR, format!("OpenAI Keypress: Failed to press key '{:?}': {}", key_enum, e), None))?;
        }
        // Release keys in reverse order
        for key_enum in key_enums.iter().rev() {
             enigo.key(*key_enum, Direction::Release)
                  .map_err(|e| ErrorData::new(ErrorCode::INTERNAL_ERROR, format!("OpenAI Keypress: Failed to release key '{:?}': {}", key_enum, e), None))?;
        }

        info!("OpenAI keypress sequence executed successfully.");
        Ok(CallToolResult::success(vec![Content::json(json!({ "status": "success" }))
            .map_err(|e| anyhow!(e).context("Failed to serialize execute_openai_keypress result"))
            .map_err(|e| ErrorData::new(ErrorCode::INTERNAL_ERROR, e.to_string(), None))?
        ]))
    }

     #[tool(name = "execute_openai_type", description = "Executes typing text requested by the OpenAI Computer Use model.")]
    async fn execute_openai_type(
        &self,
        #[tool(aggr)] params: OpenAITypeParams
    ) -> Result<CallToolResult, ErrorData> {
        info!("Executing OpenAI action: type text: '{}'", params.text);
        let mut enigo = Enigo::new(&Settings::default())
             .map_err(|e| ErrorData::new(ErrorCode::INTERNAL_ERROR, e.to_string(), None))?;

        enigo.text(&params.text)
            .map_err(|e| ErrorData::new(ErrorCode::INTERNAL_ERROR, format!("OpenAI Type: Failed to type text: {e:?}"), None))?;

        info!("OpenAI text typing successful.");
        Ok(CallToolResult::success(vec![Content::json(json!({ "status": "success" }))
            .map_err(|e| anyhow!(e).context("Failed to serialize execute_openai_type result"))
            .map_err(|e| ErrorData::new(ErrorCode::INTERNAL_ERROR, e.to_string(), None))?
        ]))
    }
    */
    
     #[tool(name = "execute_openai_wait", description = "Executes a wait/sleep action requested by the OpenAI Computer Use model.")]
    async fn execute_openai_wait(
        &self,
        #[tool(aggr)] params: OpenAIWaitParams
    ) -> Result<CallToolResult, ErrorData> {
        let duration_ms = params.duration_ms.unwrap_or(2000); // Default to 2000ms if not specified
        info!("Executing OpenAI action: wait for {} ms", duration_ms);

        sleep(Duration::from_millis(duration_ms)).await;

        info!("Wait completed.");
        Ok(CallToolResult::success(vec![Content::json(json!({ "status": "success", "duration_ms": duration_ms }))
            .map_err(|e| anyhow!(e).context("Failed to serialize execute_openai_wait result"))
            .map_err(|e| ErrorData::new(ErrorCode::INTERNAL_ERROR, e.to_string(), None))?
        ]))
    }

}

#[tool(tool_box)] // Added missing attribute
impl ServerHandler for DesktopToolProvider {
    // Provide basic server information
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::V_2024_11_05,
            capabilities: ServerCapabilities::builder()
                .enable_tools()
                .build(),
            server_info: Implementation::from_build_env(),
            instructions: Some(
                r#"This server allows controlling the desktop via various tools
                (mouse, keyboard, screen capture, shell commands).
                It also includes tools specifically for executing actions
                requested by OpenAI's Computer Use API."#.to_string()
            ),
        }
    }
    // Add other ServerHandler methods if needed
}

// --- TCP Server Function ---
pub async fn run_mcp_server_tcp() -> anyhow::Result<()> {
    let addr = "127.0.0.1:9001"; // The TCP address to listen on
    let listener = TcpListener::bind(addr).await?;
    info!("MCP Server listening on TCP {}", addr);

    let tool_provider = DesktopToolProvider; // Create the tool provider instance

    loop {
        let (stream, client_addr) = listener.accept().await?;
        info!("Accepted TCP connection from: {}", client_addr);
        let provider_clone = tool_provider.clone();

        tokio::spawn(async move {
            info!("Serving client {}...", client_addr);
            match serve_server(provider_clone, stream).await {
                Ok(server_handle) => {
                    if let Err(e) = server_handle.waiting().await {
                        if !e.to_string().contains("connection closed")
                            && !e.to_string().contains("Connection reset by peer")
                            && !e.to_string().contains("broken pipe")
                           {
                            tracing::error!("Client {} error: {:?}", client_addr, e);
                        } else {
                            info!("Client {} disconnected.", client_addr);
                        }
                    }
                }
                Err(e) => {
                    tracing::error!("Failed to start serving client {}: {:?}", client_addr, e);
                }
            }
        });
    }
    // Ok(()) // Unreachable
}

// --- Stdio Server Function ---
// Renamed from run_mcp_server_tcp
pub async fn run_mcp_server_stdio() -> anyhow::Result<()> {
    // You might want to configure tracing here if this binary is run directly
    // or ensure the parent process configures it.
    // Example:
    // tracing_subscriber::fmt()
    //     .with_env_filter(EnvFilter::from_default_env().add_directive("info".parse()?))
    //     .init(); // Or .try_init() if it might be initialized elsewhere

    info!("MCP Server starting in STDIO mode.");

    let tool_provider = DesktopToolProvider; // Create the tool provider instance

    info!("Using stdio transport. Waiting for commands on stdin...");

    // Serve the server using the stdio transport.
    // This function will run until the stdio stream is closed (e.g., the parent process closes the pipes)
    // or an unrecoverable error occurs.
    match serve_server(tool_provider, stdio()).await {
        Ok(server_handle) => {
            // server_handle.waiting() waits for the server loop to exit.
            if let Err(e) = server_handle.waiting().await {
                // Log errors unless they are expected closure types
                // (These string checks might need adjustment based on exact errors seen)
                let err_str = e.to_string();
                if !err_str.contains("EOF") // Standard end-of-file
                    && !err_str.contains("Broken pipe") // Pipe closed by the other end
                    && !err_str.contains("Connection reset by peer") // Another common pipe closure error
                    && !err_str.contains("os error 109") // Windows specific pipe error
                    && !err_str.contains("The pipe is being closed") // Another Windows pipe error
                    && !err_str.contains("Input/output error") // Can happen on pipe close
                {
                    log::error!("Server exited with unexpected error: {:?}", e);
                    // Consider propagating the error if the calling context needs it
                    // return Err(e.into());
                } else {
                    info!("Stdio stream closed (EOF or broken pipe). Server shutting down gracefully.");
                }
            } else {
                 info!("Server finished gracefully.");
            }
        }
        Err(e) => {
            log::error!("Failed to start serving on stdio: {:?}", e);
            return Err(e.into()); // Propagate the startup error
        }
    }

    Ok(())
}