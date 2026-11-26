use std::io::BufRead;
use std::path::Path;

use fake_agent::{Command, Events, Role};

fn main() {
    let mut args = std::env::args().skip(1);
    let socket = args.next().expect("the socket to listen at");
    let role = Role::agent(args.next().expect("the name of the fake agent to command"));
    let events = std::sync::Arc::new(Events::listen(Path::new(&socket)).expect("a free socket"));
    let printer = std::sync::Arc::clone(&events);
    std::thread::spawn(move || {
        loop {
            let reported = printer.wait_for("anything", |_| true);
            println!("{:?}: {:?}", reported.from, reported.event);
        }
    });
    for line in std::io::stdin().lock().lines() {
        let line = line.expect("standard input");
        match serde_json::from_str::<Command>(&line) {
            Ok(command) => events.send(&role, &command),
            Err(error) => eprintln!("not a command: {error}"),
        }
    }
}
