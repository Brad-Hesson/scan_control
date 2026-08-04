use std::{env, fs, path::PathBuf};

use spirv_builder::SpirvBuilder;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = env::args_os().skip(1);
    let shader_crate = PathBuf::from(args.next().ok_or("missing shader crate path")?);
    let output = PathBuf::from(args.next().ok_or("missing output path")?);
    let nix_toolchain = args.any(|arg| arg == "--nix-toolchain");

    let mut builder = SpirvBuilder::new(shader_crate, "spirv-unknown-vulkan1.2");
    builder.target_dir_path = Some(output.parent().unwrap().join("spirv-builder"));
    if !nix_toolchain {
        builder.toolchain_overwrite = Some("nightly-2026-04-11".to_owned());
    }
    let result = builder.build()?;
    fs::copy(result.module.unwrap_single(), output)?;
    Ok(())
}
