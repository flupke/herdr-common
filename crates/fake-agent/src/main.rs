//! The fake agent with no app's particulars ([`fake_agent::App::default`]): the program its own
//! tests run.

fn main() {
    fake_agent::run(
        &fake_agent::App::default(),
        &std::env::args().skip(1).collect::<Vec<_>>(),
    );
}
