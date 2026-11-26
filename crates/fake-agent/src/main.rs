fn main() {
    fake_agent::run(
        &fake_agent::App::default(),
        &std::env::args().skip(1).collect::<Vec<_>>(),
    );
}
