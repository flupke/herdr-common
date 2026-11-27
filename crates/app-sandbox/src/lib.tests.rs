use super::*;

fn programs(variables: &[(&str, &str)]) -> Result<Programs> {
    let variables: Vec<(String, OsString)> = [
        ("TEST_HERDR_APP_BIN_PATH", "/app/bin/herdr-app"),
        ("TEST_HC_BIN_PATH", "/hc/bin/hc"),
        ("TEST_WESTON_BIN_PATH", "/weston/bin/weston"),
    ]
    .iter()
    .chain(variables)
    .map(|(name, value)| ((*name).to_owned(), OsString::from(value)))
    .collect();
    Programs::from_variables(|name| {
        variables
            .iter()
            .find(|(variable, _)| variable == name)
            .map(|(_, value)| value.clone())
    })
}

#[test]
fn finds_mesa_by_its_path_or_by_its_software_vulkan_driver_as_apps_dev_shells_set_it() {
    let driver = "/nix/store/mesa/share/vulkan/icd.d/lvp_icd.x86_64.json";
    let cases: [(&[(&str, &str)], &str); 3] = [
        (&[("TEST_MESA_PATH", "/nix/store/mesa")], "/nix/store/mesa"),
        (
            &[("TEST_HERDR_APP_VULKAN_DRIVER", driver)],
            "/nix/store/mesa",
        ),
        (
            &[
                ("TEST_MESA_PATH", "/nix/store/new-mesa"),
                ("TEST_HERDR_APP_VULKAN_DRIVER", driver),
            ],
            "/nix/store/new-mesa",
        ),
    ];
    for (variables, mesa) in cases {
        assert_eq!(
            programs(variables).unwrap().mesa,
            Path::new(mesa),
            "{variables:?}"
        );
    }
    assert!(programs(&[]).is_err());
}
