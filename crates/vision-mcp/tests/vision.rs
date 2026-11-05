//! An agent drives apps in the app sandbox through the vision tools, as an app's MCP server
//! serves them: a program that speaks vision-signal, whose every action returns the frame that
//! shows its reaction, and a shell, which speaks nothing and is seen through its pixels. The
//! tools see and drive the app again after each restart of its core, its window, or both.

use app_sandbox::{AppSandbox, Isolation, Options, Programs, Restart};
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::model::{CallToolRequestParams, CallToolResult, ContentBlock};
use rmcp::service::RunningService;
use rmcp::{RoleClient, ServerHandler, ServiceExt, tool_handler};
use serde_json::{Value, json};
use vision_mcp::{Vision, VisionTools};

/// An app's MCP server: the vision tools, which an app adds its own to.
struct App {
    vision: VisionTools,
    router: ToolRouter<Self>,
}

#[tool_handler(router = self.router)]
impl ServerHandler for App {}

/// A tool call's result: its summary, the screen's text, and whether it is an error.
struct Called {
    summary: Value,
    text: String,
    error: bool,
    image: Option<Vec<u8>>,
}

async fn call(client: &Client, tool: &str, arguments: Value) -> Called {
    let Value::Object(arguments) = arguments else {
        panic!("arguments are an object");
    };
    let result: CallToolResult = client
        .call_tool(CallToolRequestParams::new(tool.to_owned()).with_arguments(arguments))
        .await
        .unwrap();
    let mut texts = Vec::new();
    let mut image = None;
    for content in result.content {
        match content {
            ContentBlock::Text(text) => texts.push(text.text),
            ContentBlock::Image(picture) => {
                use base64::Engine;
                image = Some(
                    base64::engine::general_purpose::STANDARD
                        .decode(picture.data)
                        .unwrap(),
                );
            }
            _ => {}
        }
    }
    let error = result.is_error == Some(true);
    Called {
        summary: if error {
            Value::Null
        } else {
            serde_json::from_str(&texts[0]).unwrap()
        },
        text: texts.join("\n"),
        error,
        image,
    }
}

type Client = RunningService<RoleClient, ()>;

#[tokio::test(flavor = "multi_thread")]
async fn drives_apps_in_the_sandbox_through_the_tools() {
    let (client, vision) = start(Isolation::Bubblewrap).await;
    drives_a_program_that_speaks_vision_signal(&client, &vision).await;
    sees_a_shell_through_its_pixels(&client, &vision).await;
    stop(client, &vision).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn sees_and_drives_the_app_after_each_restart_in_bubblewrap() {
    sees_and_drives_the_app_after_each_restart(Isolation::Bubblewrap).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn sees_and_drives_the_app_after_each_restart_in_plain_isolation() {
    sees_and_drives_the_app_after_each_restart(Isolation::Plain).await;
}

async fn sees_and_drives_the_app_after_each_restart(isolation: Isolation) {
    let (client, vision) = start(isolation).await;
    let shown = call(&client, "wait_for", json!({"text": "vision probe"})).await;
    assert!(!shown.error, "{}", shown.text);

    let probe = shown.summary["terminal"].as_str().unwrap().to_owned();

    // A core restart ends the probe with every program of the core's terminals: its terminal
    // comes back as a shell, at its address, and the window runs a new core.
    restart(&vision, Restart::Core).await;
    sees_the_kept_shell(&client, &probe).await;
    let probe = drives_a_new_probe(&client, &vision).await;

    // While a reboot keeps the window closed, what needs the window is an error that says
    // so, and the core started alone shows its terminals by their address.
    restart(&vision, Restart::Reboot).await;
    let pressed = call(&client, "key", json!({"keys": ["a"]})).await;
    assert!(
        pressed.error && pressed.text.contains("window is closed"),
        "{}",
        pressed.text
    );
    sees_the_kept_shell(&client, &probe).await;

    restart(&vision, Restart::Window).await;
    drives_a_new_probe(&client, &vision).await;
    let screenshot = call(&client, "screenshot", json!({})).await;
    let png = png::Decoder::new(std::io::Cursor::new(screenshot.image.unwrap()))
        .read_info()
        .unwrap();
    assert_eq!((png.info().width, png.info().height), (1200, 800));
    stop(client, &vision).await;
}

/// Restart the session's app as `what` says.
async fn restart(vision: &VisionTools, what: Restart) {
    let session = vision.session().clone();
    tokio::task::spawn_blocking(move || {
        session
            .lock()
            .unwrap()
            .as_mut()
            .unwrap()
            .restart(what)
            .unwrap();
    })
    .await
    .unwrap();
}

/// Wait for the shell that replaced the probe in `terminal` to show its prompt, which the probe
/// never shows: the restart ended the probe, and kept its terminal at its address. The window's
/// view of the terminal takes input only a moment later, once it reattached
/// (`.agents/wiki/app-sandbox.md`), so the test drives a new probe instead.
async fn sees_the_kept_shell(client: &Client, terminal: &str) {
    let prompt = call(
        client,
        "wait_for",
        json!({"text": "$", "terminal": terminal, "timeout_ms": 30_000}),
    )
    .await;
    assert!(!prompt.error, "{}", prompt.text);
}

/// Open a space that runs a new probe, and drive it: the frame that acknowledges the typing
/// shows it. Returns the probe's terminal.
async fn drives_a_new_probe(client: &Client, vision: &VisionTools) -> String {
    herdr_app(vision, &["do", "space.new", &probe_space()]);
    let shown = call(client, "wait_for", json!({"text": "vision probe"})).await;
    assert!(!shown.error, "{}", shown.text);
    let typed = call(client, "type", json!({"text": "again"})).await;
    assert!(typed.summary["reaction"]["acknowledged"]["frame"].is_u64());
    assert!(typed.text.contains("typed: again"), "{}", typed.text);
    typed.summary["terminal"].as_str().unwrap().to_owned()
}

/// Stop the session, which leaves no process behind.
async fn stop(client: Client, vision: &VisionTools) {
    client.cancel().await.unwrap();
    let stopped = vision
        .session()
        .lock()
        .unwrap()
        .take()
        .unwrap()
        .stop()
        .unwrap();
    assert_eq!(stopped.processes, Vec::<u32>::new());
}

/// The arguments of `space.new` for a space that runs the probe.
fn probe_space() -> String {
    let app = env!("CARGO_BIN_EXE_vision-probe");
    let argv = json!(["sh", "-c", format!("stty raw -echo && exec '{app}'")]);
    json!({"name": "app", "argv": argv}).to_string()
}

/// A sandbox in `isolation` whose first space runs the probe, served by an app's MCP server,
/// and a client of that server.
async fn start(isolation: Isolation) -> (Client, VisionTools) {
    let options = Options {
        isolation,
        display_size: (1200, 800),
        ..Options::default()
    };
    let sandbox = AppSandbox::start(&Programs::pinned().unwrap(), &options).unwrap();
    sandbox
        .herdr_app(&["do", "space.new", &probe_space()])
        .unwrap();
    let vision = VisionTools::new();
    vision
        .session()
        .lock()
        .unwrap()
        .replace(Vision::start(sandbox).unwrap());
    let server = App {
        router: VisionTools::router(|app: &App| &app.vision),
        vision: vision.clone(),
    };
    let (server_end, client_end) = tokio::io::duplex(1 << 20);
    tokio::spawn(async move {
        let running = server.serve(server_end).await.unwrap();
        let _ = running.waiting().await;
    });
    (().serve(client_end).await.unwrap(), vision)
}

async fn drives_a_program_that_speaks_vision_signal(client: &Client, vision: &VisionTools) {
    let shown = call(client, "wait_for", json!({"text": "vision probe"})).await;
    assert!(!shown.error, "{}", shown.text);

    // Each action returns the screen of the frame that acknowledged it: no wait_for needed.
    let typed = call(client, "type", json!({"text": "hello"})).await;
    assert!(typed.summary["reaction"]["acknowledged"]["frame"].is_u64());
    assert!(typed.text.contains("typed: hello"), "{}", typed.text);
    let pasted = call(
        client,
        "type",
        json!({"text": "from the clipboard", "paste": true}),
    )
    .await;
    assert!(
        pasted.text.contains("pasted: from the clipboard"),
        "{}",
        pasted.text
    );
    let clicked = call(client, "click", json!({"x": 800, "y": 400})).await;
    assert!(clicked.text.contains("mouse: click "), "{}", clicked.text);
    let dragged = call(
        client,
        "drag",
        json!({"from": [600, 400], "to": [1000, 400]}),
    )
    .await;
    let drag = shown_after(&dragged.text, "mouse: drag ");
    let columns: Vec<u32> = drag
        .split(" -> ")
        .map(|cell| cell.split(',').next().unwrap().parse().unwrap())
        .collect();
    assert!(columns[0] < columns[1], "the drag went right: {drag}");

    holds_a_drag(client, vision).await;

    // A mistake is an error, and gives no input: no key of the list is pressed.
    let unknown = call(client, "key", json!({"keys": ["a", "nosuchkey"]})).await;
    assert!(unknown.error && unknown.text.contains("nosuchkey"));
    let outside = call(client, "click", json!({"x": 1200, "y": 0})).await;
    assert!(outside.error && outside.text.contains("outside the window"));
    let screen = call(client, "screen", json!({})).await;
    assert!(screen.text.contains("typed: hello\n"), "{}", screen.text);

    // Asynchronous work shows after the action returned.
    let entered = call(client, "key", json!({"keys": ["enter"]})).await;
    assert!(
        entered.text.contains("work: working on hello"),
        "{}",
        entered.text
    );
    let done = call(client, "wait_for", json!({"text": "done: hello"})).await;
    assert!(!done.error, "{}", done.text);
    assert!(done.summary["frame"].is_u64(), "{}", done.summary);

    let screenshot = call(client, "screenshot", json!({})).await;
    let png = png::Decoder::new(std::io::Cursor::new(screenshot.image.unwrap()))
        .read_info()
        .unwrap();
    assert_eq!((png.info().width, png.info().height), (1200, 800));

    // A click that moves the focus to a shell beside it gets its reaction from the pixels,
    // and pastes no acknowledgement request into the shell.
    herdr_app(vision, &["do", "pane.split", r#"{"direction": "right"}"#]);
    herdr_app(vision, &["do", "pane.focus", r#"{"terminal": "t1"}"#]);
    let clicked = call(client, "click", json!({"x": 1100, "y": 400})).await;
    assert!(!clicked.error, "{}", clicked.text);
    assert!(clicked.summary["reaction"]["pixels"].is_object());
    assert_ne!(clicked.summary["terminal"], "t1");
    assert!(
        !clicked.text.contains("vision-acknowledge"),
        "{}",
        clicked.text
    );
}

/// A held drag keeps the button down until the next drag or click, on the program and on
/// herdr-app's own panels alike.
async fn holds_a_drag(client: &Client, vision: &VisionTools) {
    // A held drag shows the drag under way, and the next drag goes on with it, with no press;
    // the next click drops it where it ended, then clicks.
    let held = call(
        client,
        "drag",
        json!({"from": [600, 500], "to": [900, 500], "hold": true}),
    )
    .await;
    let started = shown_after(&held.text, "mouse: dragging ")
        .split(" -> ")
        .next()
        .unwrap()
        .to_owned();
    let held = call(
        client,
        "drag",
        json!({"from": [900, 500], "to": [1000, 500], "hold": true}),
    )
    .await;
    let way = shown_after(&held.text, "mouse: dragging ");
    assert!(way.starts_with(&format!("{started} -> ")), "{}", held.text);
    let released = call(client, "click", json!({"x": 800, "y": 400})).await;
    assert!(released.text.contains("mouse: click "), "{}", released.text);
    assert_eq!(shown_after(&released.text, "dropped: "), way);

    // A held drag on herdr-app's own Spaces list, beside the program, drops too: the row of
    // the second space, dragged above the first, moves there.
    herdr_app(vision, &["do", "space.new", r#"{"name": "two"}"#]);
    herdr_app(vision, &["do", "pane.focus", r#"{"terminal": "t1"}"#]);
    let held = call(
        client,
        "drag",
        json!({"from": [130, 121], "to": [130, 84], "hold": true}),
    )
    .await;
    assert!(!held.error, "{}", held.text);
    let released = call(client, "click", json!({"x": 800, "y": 400})).await;
    assert!(!released.error, "{}", released.text);
    let spaces = herdr_app(vision, &["ls"])["spaces"]
        .as_array()
        .unwrap()
        .iter()
        .map(|space| space["name"].clone())
        .collect::<Vec<_>>();
    assert_eq!(spaces, ["two", "app"]);
}

/// The rest of the screen's line that holds `label`.
fn shown_after<'a>(screen: &'a str, label: &str) -> &'a str {
    screen
        .lines()
        .find_map(|line| line.split_once(label))
        .unwrap_or_else(|| panic!("no {label:?}: {screen}"))
        .1
}

/// Run `herdr-app <arguments>` against the session's app.
fn herdr_app(vision: &VisionTools, arguments: &[&str]) -> Value {
    let session = vision.session().lock().unwrap();
    session
        .as_ref()
        .unwrap()
        .sandbox()
        .herdr_app(arguments)
        .unwrap()
}

async fn sees_a_shell_through_its_pixels(client: &Client, vision: &VisionTools) {
    // A shell speaks no vision-signal: the window's pixels tell its reaction.
    herdr_app(vision, &["do", "space.new", r#"{"name": "shell"}"#]);
    let typed = call(client, "type", json!({"text": "echo $((6 * 7))\n"})).await;
    assert_eq!(typed.summary["reaction"]["pixels"]["changed"], true);
    let shown = call(client, "wait_for", json!({"text": "42"})).await;
    assert!(!shown.error, "{}", shown.text);
    let missing = call(
        client,
        "wait_for",
        json!({"text": "never shown", "timeout_ms": 200}),
    )
    .await;
    assert!(missing.error);
    assert!(missing.text.contains("42"), "{}", missing.text);
}
