use super::*;

#[test]
fn shows_a_masked_directory_empty_inside_and_leaves_it_on_the_host() {
    let crate_directory = Path::new(env!("CARGO_MANIFEST_DIR"));
    let masked = crate_directory.join("src");
    let absent = crate_directory.join("absent");
    let mut bubble = Bubble::start(&[masked.clone(), absent.clone()]).unwrap();
    let environment = [("PATH".into(), std::env::var_os("PATH").unwrap())];
    let exists_inside = |path: &Path| {
        bubble
            .command(Path::new("/bin/sh"), Bubble::inside(), &environment)
            .args(["-c", "test -e \"$0\"", &path.to_string_lossy()])
            .status()
            .unwrap()
            .success()
    };

    assert!(exists_inside(&crate_directory.join("Cargo.toml")));
    assert!(exists_inside(&masked));
    assert!(!exists_inside(&masked.join("bubble.rs")));
    assert!(!exists_inside(&absent));
    assert!(!absent.exists());
    bubble.end().unwrap();
    assert!(masked.join("bubble.rs").exists());
}
