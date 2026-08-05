use std::{env, path::PathBuf};

use rust_gpu_compiler::Artifacts;
use spirv_to_wgpu::{Builder, EntryPointConfig, PipelineLayoutConfig};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let rust_gpu_modules = rust_gpu_compiler::Build::new("shader")
        .target("spirv-unknown-vulkan1.2")
        .capability("Float64")
        .capability("Sampled1D")
        .multimodule(true)
        .compile("image_compute")?;
    generate_rust_gpu_bindings(&rust_gpu_modules)?;
    Ok(())
}

fn generate_rust_gpu_bindings(artifacts: &Artifacts) -> Result<(), Box<dyn std::error::Error>> {
    let scan_vertex = artifacts.entry_point("scan_image::vs_main")?;
    let scan_fragment = artifacts.entry_point("scan_image::fs_main")?;
    let scan = PipelineLayoutConfig {
        rust_module_name: "scan_image",
        entry_points: &[
            EntryPointConfig {
                spirv_name: scan_vertex.entry_point,
                spirv_path: scan_vertex.path,
                rust_name: "vs_main",
            },
            EntryPointConfig {
                spirv_name: scan_fragment.entry_point,
                spirv_path: scan_fragment.path,
                rust_name: "fs_main",
            },
        ],
        binding_overrides: &[],
    };

    let file_vs = artifacts.entry_point("file_image::vs_main")?;
    let file_fs = artifacts.entry_point("file_image::fs_main")?;
    let file = PipelineLayoutConfig {
        rust_module_name: "file_image",
        entry_points: &[
            EntryPointConfig {
                spirv_name: file_vs.entry_point,
                spirv_path: file_vs.path,
                rust_name: "vs_main",
            },
            EntryPointConfig {
                spirv_name: file_fs.entry_point,
                spirv_path: file_fs.path,
                rust_name: "fs_main",
            },
        ],
        binding_overrides: &[],
    };
    let border_vs = artifacts.entry_point("border_line::vs_main")?;
    let border_fs = artifacts.entry_point("border_line::fs_main")?;
    let border = PipelineLayoutConfig {
        rust_module_name: "border_line",
        entry_points: &[
            EntryPointConfig {
                spirv_name: border_vs.entry_point,
                spirv_path: border_vs.path,
                rust_name: "vs_main",
            },
            EntryPointConfig {
                spirv_name: border_fs.entry_point,
                spirv_path: border_fs.path,
                rust_name: "fs_main",
            },
        ],
        binding_overrides: &[],
    };

    let plane_names = [
        "copy_image",
        "copy_image_transpose",
        "reduce_image",
        "reduce_image_lines",
        "generate_sums_plane",
        "generate_sums_lines",
        "reduce_sums_plane",
        "reduce_sums_lines",
        "generate_normalization__mean_subtract",
        "generate_normalization__plane_fit",
        "generate_normalization__line_fit",
        "generate_normalization__line_mean",
        "reduce_normalizations",
        "clear_texture",
    ];
    let plane_artifacts = plane_names
        .iter()
        .map(|name| artifacts.entry_point(&format!("plane_fit::{name}")))
        .collect::<Result<Vec<_>, _>>()?;
    let plane_entries = plane_names
        .iter()
        .zip(&plane_artifacts)
        .map(|(name, artifact)| EntryPointConfig {
            spirv_name: artifact.entry_point,
            spirv_path: artifact.path,
            rust_name: name,
        })
        .collect::<Vec<_>>();
    let plane = PipelineLayoutConfig {
        rust_module_name: "plane_fit",
        entry_points: &plane_entries,
        binding_overrides: &[],
    };

    Builder::new()
        .pipeline_layout(scan)
        .pipeline_layout(file)
        .pipeline_layout(border)
        .pipeline_layout(plane)
        .generate()?
        .write_to_file(PathBuf::from(env::var("OUT_DIR")?).join("rust_gpu.rs"))?;
    Ok(())
}
