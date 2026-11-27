use std::os::unix::fs::{PermissionsExt, symlink};

use super::*;

fn machine_with_an_nvidia_gpu_then_an_amd_one_then_an_intel_one() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    for (node, driver) in [
        ("renderD128", "nvidia"),
        ("renderD129", "amdgpu"),
        ("renderD130", "i915"),
        ("card0", "amdgpu"),
    ] {
        let device = root.path().join(format!("sys/devices/{node}"));
        std::fs::create_dir_all(&device).unwrap();
        symlink(
            format!("/sys/bus/pci/drivers/{driver}"),
            device.join("driver"),
        )
        .unwrap();
        let class = root.path().join("sys/class/drm").join(node);
        std::fs::create_dir_all(&class).unwrap();
        symlink(&device, class.join("device")).unwrap();
    }
    std::fs::create_dir_all(root.path().join("dev/dri")).unwrap();
    for node in ["renderD128", "renderD129", "renderD130", "card0"] {
        std::fs::write(root.path().join("dev/dri").join(node), "").unwrap();
    }
    root
}

fn choose(rendering: Rendering, root: &Path) -> Result<Renderer> {
    Renderer::choose(rendering, &root.join("sys"), &root.join("dev"))
}

#[test]
fn draws_on_the_first_gpu_mesa_drives_and_the_sandbox_may_open_else_in_software() {
    let root = machine_with_an_nvidia_gpu_then_an_amd_one_then_an_intel_one();
    let amd = Renderer::Gpu {
        render_node: root.path().join("dev/dri/renderD129"),
        kernel_driver: "amdgpu".to_owned(),
    };
    assert_eq!(choose(Rendering::Auto, root.path()).unwrap(), amd);
    assert_eq!(choose(Rendering::Gpu, root.path()).unwrap(), amd);
    assert_eq!(
        choose(Rendering::Software, root.path()).unwrap(),
        Renderer::Software
    );

    let close = |node: &str| {
        let node = root.path().join("dev/dri").join(node);
        std::fs::set_permissions(node, std::fs::Permissions::from_mode(0o400)).unwrap();
    };
    close("renderD129");
    assert_eq!(
        choose(Rendering::Auto, root.path()).unwrap(),
        Renderer::Gpu {
            render_node: root.path().join("dev/dri/renderD130"),
            kernel_driver: "i915".to_owned(),
        }
    );
    close("renderD130");
    assert_eq!(
        choose(Rendering::Auto, root.path()).unwrap(),
        Renderer::Software
    );
    assert!(choose(Rendering::Gpu, root.path()).is_err());
}

#[test]
fn draws_in_software_on_a_machine_with_no_render_node() {
    let root = tempfile::tempdir().unwrap();
    assert_eq!(
        choose(Rendering::Auto, root.path()).unwrap(),
        Renderer::Software
    );
    assert!(choose(Rendering::Gpu, root.path()).is_err());
}

#[test]
fn names_vulkan_drivers_the_dev_shell_s_mesa_has() {
    let mesa = PathBuf::from(std::env::var_os("TEST_MESA_PATH").expect("run in the dev shell"));
    let renderers = GPU_DRIVERS
        .iter()
        .map(|(driver, _)| Renderer::Gpu {
            render_node: "/dev/dri/renderD128".into(),
            kernel_driver: (*driver).to_owned(),
        })
        .chain([Renderer::Software]);
    for renderer in renderers {
        let drivers = renderer.vulkan_drivers(&mesa);
        let files: Vec<PathBuf> = std::env::split_paths(&drivers).collect();
        assert!(!files.is_empty(), "{renderer}");
        for file in files {
            assert!(file.is_file(), "{renderer}: no {}", file.display());
        }
    }
}
