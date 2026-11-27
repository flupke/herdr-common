use std::ffi::OsString;
use std::fmt;
use std::path::{Path, PathBuf};

use eyre::{Result, bail};
use serde::{Deserialize, Serialize};

/// What the sandbox's display and app draw with, as a caller asks for it.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Rendering {
    /// The GPU when the machine has one on amdgpu, i915, xe or nouveau whose render node
    /// (`/dev/dri/renderD*`) the sandbox may open, and software otherwise, such as on a CI
    /// runner, a VPS, or a machine with NVIDIA's own driver alone.
    #[default]
    Auto,
    /// The GPU, or the sandbox fails to start.
    Gpu,
    /// Mesa's software renderer (llvmpipe), on the CPU.
    Software,
}

/// What a sandbox draws with, as it started.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Renderer {
    Gpu {
        /// The first such render node, such as `/dev/dri/renderD128`. On a machine with two
        /// GPUs of one kernel driver, Weston and the app may draw on the other.
        render_node: PathBuf,
        /// The host's kernel driver of the GPU, such as `amdgpu`.
        kernel_driver: String,
    },
    Software,
}

/// Mesa's drivers reach the GPU through the kernel's interface alone, so the dev shell's Mesa
/// draws on the host's GPU with none of the host's own libraries. NVIDIA's own kernel driver is
/// left out: it takes its own libraries, of its exact version, which Nix does not pin.
const GPU_DRIVERS: [(&str, &[&str]); 4] = [
    ("amdgpu", &["radeon"]),
    ("i915", &["intel", "intel_hasvk"]),
    ("xe", &["intel"]),
    ("nouveau", &["nouveau"]),
];

impl Renderer {
    /// What `rendering` draws with on the machine whose sysfs is `sys` and whose devices are
    /// in `dev`: `/sys` and `/dev`.
    pub(crate) fn choose(rendering: Rendering, sys: &Path, dev: &Path) -> Result<Self> {
        let gpu = || find_gpu(sys, dev);
        Ok(match rendering {
            Rendering::Software => Self::Software,
            Rendering::Auto => gpu().unwrap_or(Self::Software),
            Rendering::Gpu => match gpu() {
                Some(gpu) => gpu,
                None => bail!(
                    "no GPU the sandbox can draw with: none of {} has a render node it may \
                     open in {}",
                    GPU_DRIVERS.map(|(driver, _)| driver).join(", "),
                    dev.join("dri").display()
                ),
            },
        })
    }

    /// Mesa's Vulkan drivers for this renderer, as `VK_DRIVER_FILES` takes them.
    pub(crate) fn vulkan_drivers(&self, mesa: &Path) -> OsString {
        let names: &[&str] = match self {
            Self::Software => &["lvp"],
            Self::Gpu { kernel_driver, .. } => GPU_DRIVERS
                .iter()
                .find(|(driver, _)| driver == kernel_driver)
                .map_or(&[], |(_, names)| names),
        };
        let files = names.iter().map(|name| {
            mesa.join("share/vulkan/icd.d")
                .join(format!("{name}_icd.{}.json", std::env::consts::ARCH))
                .into_os_string()
        });
        std::env::join_paths(files).unwrap_or_default()
    }

    /// Weston's renderer: on the GPU, its OpenGL renderer, which lets its Xwayland draw on the
    /// GPU too (glamor) and offer the app DRI3, which a GPU's Vulkan driver needs to present.
    pub(crate) fn weston_renderer(&self) -> &'static str {
        match self {
            Self::Gpu { .. } => "gl",
            Self::Software => "pixman",
        }
    }
}

impl fmt::Display for Renderer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Gpu {
                render_node,
                kernel_driver,
            } => write!(
                formatter,
                "the GPU ({}, {kernel_driver})",
                render_node.display()
            ),
            Self::Software => write!(formatter, "software (llvmpipe, on the CPU)"),
        }
    }
}

fn find_gpu(sys: &Path, dev: &Path) -> Option<Renderer> {
    let mut nodes: Vec<(u32, String)> = std::fs::read_dir(sys.join("class/drm"))
        .ok()?
        .filter_map(|entry| {
            let name = entry.ok()?.file_name().into_string().ok()?;
            Some((name.strip_prefix("renderD")?.parse().ok()?, name))
        })
        .collect();
    nodes.sort();
    nodes.into_iter().find_map(|(_, name)| {
        let driver =
            std::fs::read_link(sys.join("class/drm").join(&name).join("device/driver")).ok()?;
        let kernel_driver = driver.file_name()?.to_str()?.to_owned();
        GPU_DRIVERS
            .iter()
            .any(|(driver, _)| *driver == kernel_driver)
            .then_some(())?;
        let render_node = dev.join("dri").join(&name);
        std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&render_node)
            .ok()?;
        Some(Renderer::Gpu {
            render_node,
            kernel_driver,
        })
    })
}

#[cfg(test)]
#[path = "rendering.tests.rs"]
mod tests;
