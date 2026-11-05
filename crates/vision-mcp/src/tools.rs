//! The vision tools, which an app's MCP server takes in beside its own: screen, screenshot,
//! key, type, click, drag, settle and `wait_for`. Every tool that acts returns once the app
//! reacted, and every mistake is an error result the agent reads.

use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use base64::Engine;
use eyre::{ContextCompat, Result, ensure};
use rmcp::handler::server::router::tool::{ToolRoute, ToolRouter};
use rmcp::handler::server::tool::ToolCallContext;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolRequestParams, CallToolResult, ContentBlock};
use rmcp::{ErrorData, schemars, tool, tool_router};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::session::{Input, Reaction, Screen, Vision};

/// What the tools are, for the instructions of an app's MCP server.
pub const INSTRUCTIONS: &str = "Eyes and hands on an app in a private herdr-app, on a headless \
display that never reaches the user's: screen reads a terminal's text, screenshot the window's \
pixels; key, type, click and drag act on the window as its user would, and return once the app \
reacted. When the focused terminal's program speaks vision-signal, and no held drag keeps the \
button down, an action returns the frame it painted once it read the input; otherwise, once the \
window's pixels changed and settled. After a change the app's own tools made, settle returns \
the same way, with no input. A vision-signal program's frame then shows what reached its \
terminal, not a change it learns another way (a socket, a file) nor the rest of the window: \
for those, pass pixels, which waits on the whole window. For asynchronous work, \
call wait_for with the text the finished screen shows. Screens give each row as `row: text`, \
drop the spaces and box sides that end a row, and fold runs of empty rows; changed_rows lists \
the rows that differ from the previous screen of that terminal a tool returned, and is absent \
on the first. click and drag take the window's pixels, as screenshot shows them.";

/// The longest a `wait_for` may wait.
const MAX_WAIT_MS: u64 = 60_000;

/// What `wait_for` waits when no timeout is given.
const DEFAULT_WAIT_MS: u64 = 5_000;

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct ScreenInput {
    /// The terminal to read, by its hc address (`t1`); the focused one when left out.
    terminal: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct KeyInput {
    /// Keys pressed in order: a character (`a`, `?`, `A`), `enter`, `esc`, `tab`, `backspace`,
    /// `space`, `up`, `down`, `left`, `right`, `home`, `end`, `pageup`, `pagedown`, `insert`,
    /// `delete`, `f1`..`f12`, with modifiers joined by `+` (`ctrl+shift+t`, `alt+x`). An unknown
    /// name is an error, and then no key is pressed.
    keys: Vec<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct TypeInput {
    /// The text: each character typed as its key, a newline as Enter.
    text: String,
    /// Paste the text into the focused terminal instead, as its user's clipboard pastes it
    /// (bracketed paste), newlines included.
    #[serde(default)]
    paste: bool,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct ClickInput {
    /// The window's pixel to click: zero-based, from the left.
    x: u16,
    /// From the top.
    y: u16,
    /// Modifiers held through the click: `ctrl` (herdr-app opens the link clicked so),
    /// `shift`, `alt`, `super`.
    #[serde(default)]
    modifiers: Vec<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct DragInput {
    /// The window's pixel the drag starts on, `[x, y]`.
    from: [u16; 2],
    /// The pixel it ends on, `[x, y]`.
    to: [u16; 2],
    /// Modifiers held through the drag: `ctrl`, `shift`, `alt`, `super`.
    #[serde(default)]
    modifiers: Vec<String>,
    /// Keep the button down at the end, to look at the drag under way, until a click, or a drag
    /// without hold, releases it. The next drag goes on with it, with no press: start it at this
    /// drag's `to`, or the pointer jumps to its `from` with the button down. The next click
    /// releases it where this drag ended, which drops there, then clicks. A paste while the
    /// button is held leaves a drag on herdr-app's panels unable to drop.
    #[serde(default)]
    hold: bool,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct WaitInput {
    /// The text to wait for.
    text: String,
    /// Wait until the screen no longer shows the text, instead of until it does.
    #[serde(default)]
    absent: bool,
    /// The longest wait, 5000 ms by default, at most 60000. Running out is an error that shows
    /// the latest screen.
    timeout_ms: Option<u64>,
    /// The terminal to watch, by its hc address; the focused one when left out.
    terminal: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct SettleInput {
    /// Wait on the whole window's pixels, whatever has the focus: for a change outside the
    /// focused terminal (herdr-app's panels, another pane, a new terminal), or one its program
    /// learns another way than its terminal.
    #[serde(default)]
    pixels: bool,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct NoInput {}

/// The vision tools, on the session an app starts and stops with its own tools.
#[derive(Clone, Default)]
pub struct VisionTools {
    session: Arc<Mutex<Option<Vision>>>,
}

/// What a tool returns: a summary, a screen's text, an image.
struct Outcome {
    summary: Value,
    screen: Option<Screen>,
    image: Option<Vec<u8>>,
}

impl Outcome {
    fn screen(status: &str, screen: Screen) -> Self {
        Self {
            summary: json!({"status": status}),
            screen: Some(screen),
            image: None,
        }
    }

    fn reaction((reaction, screen): (Reaction, Option<Screen>)) -> Self {
        Self {
            summary: json!({"status": "reacted", "reaction": reaction}),
            screen,
            image: None,
        }
    }

    fn into_result(mut self) -> CallToolResult {
        let mut content = Vec::new();
        if let Some(screen) = &self.screen {
            self.summary["terminal"] = json!(screen.terminal);
            if let Some(frame) = screen.frame {
                self.summary["frame"] = json!(frame);
            }
            if let Some(changed) = &screen.changed_rows {
                self.summary["changed_rows"] = json!(changed);
            }
        }
        content.push(ContentBlock::text(self.summary.to_string()));
        if let Some(screen) = self.screen {
            content.push(ContentBlock::text(screen.numbered()));
        }
        if let Some(png) = self.image {
            content.push(ContentBlock::image(
                base64::engine::general_purpose::STANDARD.encode(png),
                "image/png",
            ));
        }
        CallToolResult::success(content)
    }
}

impl VisionTools {
    /// Tools with no session yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// The session the tools act on, which an app's own tools start (`replace`) and stop
    /// (`take`).
    pub fn session(&self) -> &Arc<Mutex<Option<Vision>>> {
        &self.session
    }

    /// The tools, as routes of the MCP server `S`, whose `vision` gives them: add an app's own
    /// tools to it, and route its calls with `#[tool_handler(router = ...)]`.
    pub fn router<S: Send + Sync + 'static>(vision: fn(&S) -> &Self) -> ToolRouter<S> {
        let mut router = ToolRouter::new();
        for route in Self::own_router() {
            let call = Arc::clone(&route.call);
            router.add_route(ToolRoute::new_dyn(
                route.attr,
                move |context: ToolCallContext<'_, S>| {
                    let mut request = CallToolRequestParams::new(context.name.clone());
                    if let Some(arguments) = context.arguments {
                        request = request.with_arguments(arguments);
                    }
                    call(ToolCallContext::new(
                        vision(context.service),
                        request,
                        context.request_context,
                    ))
                },
            ));
        }
        router
    }

    /// Run `action` on the session off the async runtime, and turn an error into an error
    /// result.
    async fn run(
        &self,
        action: impl FnOnce(&mut Vision) -> Result<Outcome> + Send + 'static,
    ) -> Result<CallToolResult, ErrorData> {
        let session = Arc::clone(&self.session);
        tokio::task::spawn_blocking(move || {
            let mut session = session.lock().unwrap_or_else(PoisonError::into_inner);
            let outcome = session
                .as_mut()
                .context("no vision session is running: start one first")
                .and_then(action);
            match outcome {
                Ok(outcome) => outcome.into_result(),
                Err(error) => CallToolResult::error(vec![ContentBlock::text(format!("{error:#}"))]),
            }
        })
        .await
        .map_err(|error| ErrorData::internal_error(error.to_string(), None))
    }
}

#[tool_router(router = own_router)]
impl VisionTools {
    #[tool(
        description = "Read a terminal's whole screen as it is now, one line per row: the focused terminal's, or the one named by its hc address. changed_rows lists the rows that differ from the previous screen of that terminal a tool returned."
    )]
    async fn screen(
        &self,
        Parameters(input): Parameters<ScreenInput>,
    ) -> Result<CallToolResult, ErrorData> {
        self.run(move |vision| {
            Ok(Outcome::screen(
                "screen",
                vision.screen(input.terminal.as_deref())?,
            ))
        })
        .await
    }

    #[tool(
        description = "Take a PNG of the app's window, herdr-app's own rendering of its last frame, to judge how it looks (layout, colors) and find the pixels to click. Read screen for text."
    )]
    async fn screenshot(
        &self,
        Parameters(_): Parameters<NoInput>,
    ) -> Result<CallToolResult, ErrorData> {
        self.run(move |vision| {
            Ok(Outcome {
                summary: json!({"status": "screenshot"}),
                screen: None,
                image: Some(vision.screenshot()?),
            })
        })
        .await
    }

    #[tool(
        description = "Press keys in the app's window, in order, and return once it reacted, with the focused terminal's screen."
    )]
    async fn key(
        &self,
        Parameters(input): Parameters<KeyInput>,
    ) -> Result<CallToolResult, ErrorData> {
        self.run(move |vision| Ok(Outcome::reaction(vision.act(&Input::Keys(&input.keys))?)))
            .await
    }

    #[tool(
        name = "type",
        description = "Type text in the app's window, each character as its key, or paste it into the focused terminal with paste, and return once the app reacted, with the focused terminal's screen."
    )]
    async fn type_text(
        &self,
        Parameters(input): Parameters<TypeInput>,
    ) -> Result<CallToolResult, ErrorData> {
        self.run(move |vision| {
            let given = if input.paste {
                Input::Paste(&input.text)
            } else {
                Input::Text(&input.text)
            };
            Ok(Outcome::reaction(vision.act(&given)?))
        })
        .await
    }

    #[tool(
        description = "Left-click a pixel of the app's window, as the screenshot shows it, and return once the app reacted, with the focused terminal's screen. A pixel outside the window is an error."
    )]
    async fn click(
        &self,
        Parameters(input): Parameters<ClickInput>,
    ) -> Result<CallToolResult, ErrorData> {
        self.run(move |vision| {
            Ok(Outcome::reaction(vision.act(&Input::Click {
                x: input.x,
                y: input.y,
                modifiers: &input.modifiers,
            })?))
        })
        .await
    }

    #[tool(
        description = "Drag with the left button from one pixel of the app's window to another, as a hand does, and return once the app reacted, with the focused terminal's screen. With hold, the button stays down at the end, for a screenshot of the drag under way, until a click, or a drag without hold, releases it; the window's pixels then tell the app's reaction, as no frame is awaited while the button is down."
    )]
    async fn drag(
        &self,
        Parameters(input): Parameters<DragInput>,
    ) -> Result<CallToolResult, ErrorData> {
        self.run(move |vision| {
            let [from_x, from_y] = input.from;
            let [to_x, to_y] = input.to;
            Ok(Outcome::reaction(vision.act(&Input::Drag {
                from: (from_x, from_y),
                to: (to_x, to_y),
                modifiers: &input.modifiers,
                hold: input.hold,
            })?))
        })
        .await
    }

    #[tool(
        description = "Wait for the app to show a change no vision tool made, such as one the app's own tools made, and return the focused terminal's screen. When its program speaks vision-signal, returns the frame it painted once it read what reached its terminal before; for a change elsewhere in the window, or one the program learns another way (a socket, a file), pass pixels. Otherwise, or with pixels, returns once the window's pixels changed within a second and stayed quiet; changed: false says the window showed the change before the call, or never did."
    )]
    async fn settle(
        &self,
        Parameters(input): Parameters<SettleInput>,
    ) -> Result<CallToolResult, ErrorData> {
        self.run(move |vision| {
            Ok(Outcome::reaction(if input.pixels {
                vision.settle_pixels()?
            } else {
                vision.settle()?
            }))
        })
        .await
    }

    #[tool(
        description = "Wait until a terminal's screen shows a text, or no longer shows it with absent, and return that screen: for asynchronous work, with a text the screen before did not show. The screen is checked at once, then after each frame its program paints (vision-signal), or every few milliseconds."
    )]
    async fn wait_for(
        &self,
        Parameters(input): Parameters<WaitInput>,
    ) -> Result<CallToolResult, ErrorData> {
        self.run(move |vision| {
            let timeout = input.timeout_ms.unwrap_or(DEFAULT_WAIT_MS);
            ensure!(
                timeout <= MAX_WAIT_MS,
                "timeout_ms is at most {MAX_WAIT_MS}"
            );
            let screen = vision.wait_for(
                &input.text,
                input.absent,
                Duration::from_millis(timeout),
                input.terminal.as_deref(),
            )?;
            Ok(Outcome::screen(
                if input.absent { "gone" } else { "shown" },
                screen,
            ))
        })
        .await
    }
}
