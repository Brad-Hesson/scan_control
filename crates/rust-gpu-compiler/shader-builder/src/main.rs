use std::{env, fmt::Write as _, fs, path::PathBuf, str::FromStr};

use spirv_builder::{Capability, SpirvBuilder, SpirvMetadata};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = env::args().skip(1);
    let shader_crate = PathBuf::from(args.next().ok_or("missing shader crate path")?);
    let output_dir = PathBuf::from(args.next().ok_or("missing output directory")?);
    let mut target = None;
    let mut capabilities = Vec::new();
    let mut metadata = SpirvMetadata::NameVariables;
    let mut multimodule = false;
    let mut preserve_bindings = false;
    let mut nix_toolchain = false;

    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--target" => target = Some(args.next().ok_or("missing --target value")?),
            "--capability" => {
                let value = args.next().ok_or("missing --capability value")?;
                capabilities.push(
                    Capability::from_str(&value)
                        .map_err(|()| format!("unknown SPIR-V capability {value:?}"))?,
                );
            }
            "--metadata" => {
                metadata = match args.next().as_deref() {
                    Some("none") => SpirvMetadata::None,
                    Some("name-variables") => SpirvMetadata::NameVariables,
                    Some("full") => SpirvMetadata::Full,
                    value => return Err(format!("unknown SPIR-V metadata mode {value:?}").into()),
                };
            }
            "--multimodule" => multimodule = true,
            "--preserve-bindings" => preserve_bindings = true,
            "--nix-toolchain" => nix_toolchain = true,
            argument => return Err(format!("unknown argument {argument:?}").into()),
        }
    }

    let mut builder = SpirvBuilder::new(
        shader_crate,
        target.ok_or("missing required --target argument")?,
    )
    .multimodule(multimodule)
    .spirv_metadata(metadata)
    .preserve_bindings(preserve_bindings);
    for capability in capabilities {
        builder = builder.capability(capability);
    }
    builder.target_dir_path = Some(output_dir.join("spirv-builder"));
    if !nix_toolchain {
        builder.toolchain_overwrite = Some("nightly-2026-04-11".to_owned());
    }

    let result = builder.build()?;
    fs::create_dir_all(&output_dir)?;
    let mut manifest = String::new();
    if multimodule {
        for (entry_point, source) in result.module.unwrap_multi() {
            let file_name = format!("{}.spv", entry_point.replace("::", "__"));
            fs::copy(source, output_dir.join(&file_name))?;
            writeln!(manifest, "{entry_point}\t{file_name}")?;
        }
    } else {
        let file_name = "shader.spv";
        fs::copy(result.module.unwrap_single(), output_dir.join(file_name))?;
        for entry_point in &result.entry_points {
            writeln!(manifest, "{entry_point}\t{file_name}")?;
        }
    }
    fs::write(output_dir.join("modules.tsv"), manifest)?;
    Ok(())
}
