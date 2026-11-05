//! An agent drives apps in the app sandbox through the vision tools, as an app's MCP server
//! serves them: a program that speaks vision-signal, whose every action returns the frame that
//! shows its reaction, and a shell, which speaks nothing and is seen through its pixels.

use app_sandbox::{AppSandbox, Options, Programs};
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
    let (client, vision) = start().await;
    drives_a_program_that_speaks_vision_signal(&client, &vision).await;
    sees_a_shell_through_its_pixels(&client, &vision).await;
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

/// A sandbox whose first space runs the probe, served by an app's MCP server, and a client
/// of that server.
async fn start() -> (Client, VisionTools) {
    let options = Options {
        display_size: (1200, 800),
        ..Options::default()
    };
    let sandbox = AppSandbox::start(&Programs::pinned().unwrap(), &options).unwrap();
    let app = env!("CARGO_BIN_EXE_vision-probe");
    let argv = json!(["sh", "-c", format!("stty raw -echo && exec '{app}'")]);
    sandbox
        .herdr_app(&[
            "do",
            "space.new",
            &json!({"name": "app", "argv": argv}).to_string(),
        ])
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
    let drag = dragged
        .text
        .lines()
        .find_map(|line| line.split_once("mouse: drag "))
        .unwrap_or_else(|| panic!("no drag: {}", dragged.text))
        .1;
    let columns: Vec<u32> = drag
        .split(" -> ")
        .map(|cell| cell.split(',').next().unwrap().parse().unwrap())
        .collect();
    assert!(columns[0] < columns[1], "the drag went right: {drag}");

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
    assert!(done.summary["frame"].as_u64() > entered.summary["frame"].as_u64());

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

/// Run `herdr-app <arguments>` against the session's app.
fn herdr_app(vision: &VisionTools, arguments: &[&str]) {
    let session = vision.session().lock().unwrap();
    session
        .as_ref()
        .unwrap()
        .sandbox()
        .herdr_app(arguments)
        .unwrap();
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
