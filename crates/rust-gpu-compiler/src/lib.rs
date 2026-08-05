//! Build-script facade for compiling a rust-gpu shader crate.
//!
//! Like `cc::Build`, [`Build`] owns compiler discovery, output placement, Cargo
//! rebuild directives, and invocation. The bundled `spirv-builder` driver stays
//! in a nested nightly-only workspace, so consumers remain on stable Rust.

use std::{
    collections::BTreeMap,
    env,
    ffi::OsString,
    path::{Path, PathBuf},
    process::Command,
};

use thiserror::Error;

#[derive(Clone, Debug)]
pub struct Build {
    shader_crate: PathBuf,
    compiler_crate: PathBuf,
    out_dir: Option<PathBuf>,
    cargo: OsString,
    cargo_metadata: bool,
    target: String,
    capabilities: Vec<String>,
    metadata: Metadata,
    multimodule: bool,
    preserve_bindings: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub enum Metadata {
    None,
    #[default]
    NameVariables,
    Full,
}

impl Build {
    pub fn new(shader_crate: impl Into<PathBuf>) -> Self {
        Self {
            shader_crate: shader_crate.into(),
            compiler_crate: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("shader-builder"),
            out_dir: None,
            cargo: OsString::from("cargo"),
            cargo_metadata: true,
            target: "spirv-unknown-vulkan1.2".to_owned(),
            capabilities: Vec::new(),
            metadata: Metadata::NameVariables,
            multimodule: true,
            preserve_bindings: true,
        }
    }

    pub fn compiler_crate(&mut self, path: impl Into<PathBuf>) -> &mut Self {
        self.compiler_crate = path.into();
        self
    }

    pub fn target(&mut self, target: impl Into<String>) -> &mut Self {
        self.target = target.into();
        self
    }

    pub fn capability(&mut self, capability: impl Into<String>) -> &mut Self {
        self.capabilities.push(capability.into());
        self
    }

    pub fn metadata(&mut self, metadata: Metadata) -> &mut Self {
        self.metadata = metadata;
        self
    }

    pub fn multimodule(&mut self, enabled: bool) -> &mut Self {
        self.multimodule = enabled;
        self
    }

    pub fn preserve_bindings(&mut self, enabled: bool) -> &mut Self {
        self.preserve_bindings = enabled;
        self
    }

    pub fn out_dir(&mut self, path: impl Into<PathBuf>) -> &mut Self {
        self.out_dir = Some(path.into());
        self
    }

    pub fn cargo(&mut self, program: impl Into<OsString>) -> &mut Self {
        self.cargo = program.into();
        self
    }

    pub fn cargo_metadata(&mut self, enabled: bool) -> &mut Self {
        self.cargo_metadata = enabled;
        self
    }

    pub fn compile(&self, output_name: &str) -> Result<Artifacts, Error> {
        validate_output_name(output_name)?;
        let manifest_dir = env_path("CARGO_MANIFEST_DIR")?;
        let shader_crate = resolve(&manifest_dir, &self.shader_crate);
        let compiler_crate = resolve(&manifest_dir, &self.compiler_crate);
        let out_dir = match &self.out_dir {
            Some(path) => resolve(&manifest_dir, path),
            None => env_path("OUT_DIR")?,
        };
        let output = out_dir.join("rust-gpu").join(output_name);
        // Keep the helper cache independent from the host workspace target.
        let compiler_target = out_dir.join("rust-gpu-builder-target");

        if self.cargo_metadata {
            println!("cargo:rerun-if-changed={}", shader_crate.display());
            println!("cargo:rerun-if-changed={}", compiler_crate.display());
            println!("cargo:rerun-if-env-changed=RUST_GPU_TOOLCHAIN_BIN");
        }

        let mut command = Command::new(&self.cargo);
        command
            .current_dir(&compiler_crate)
            .args(["run", "--quiet", "--release", "--locked", "--target-dir"])
            .arg(&compiler_target)
            .arg("--")
            .arg(&shader_crate)
            .arg(&output)
            .arg("--target")
            .arg(&self.target)
            .arg("--metadata")
            .arg(match self.metadata {
                Metadata::None => "none",
                Metadata::NameVariables => "name-variables",
                Metadata::Full => "full",
            });
        for capability in &self.capabilities {
            command.arg("--capability").arg(capability);
        }
        if self.multimodule {
            command.arg("--multimodule");
        }
        if self.preserve_bindings {
            command.arg("--preserve-bindings");
        }

        if let Some(bin_dir) = env::var_os("RUST_GPU_TOOLCHAIN_BIN") {
            let mut paths = vec![PathBuf::from(bin_dir)];
            paths.extend(env::split_paths(&env::var_os("PATH").unwrap_or_default()));
            command.env("PATH", env::join_paths(paths).map_err(Error::JoinPaths)?);
            command.arg("--nix-toolchain");
        }

        let status = command.status().map_err(Error::CompilerLaunch)?;
        if !status.success() {
            return Err(Error::CompilerFailed(status));
        }

        Artifacts::read(output)
    }
}

#[derive(Clone, Debug)]
pub struct Artifacts {
    modules: BTreeMap<String, PathBuf>,
}

#[derive(Clone, Copy, Debug)]
pub struct SpirvArtifact<'a> {
    pub entry_point: &'a str,
    pub path: &'a Path,
}

impl Artifacts {
    fn read(output: PathBuf) -> Result<Self, Error> {
        let manifest_path = output.join("modules.tsv");
        let manifest =
            std::fs::read_to_string(&manifest_path).map_err(|source| Error::ReadManifest {
                path: manifest_path,
                source,
            })?;
        let mut modules = BTreeMap::new();
        for line in manifest.lines() {
            let (entry, file) = line
                .split_once('\t')
                .ok_or_else(|| Error::InvalidManifestLine(line.to_owned()))?;
            modules.insert(entry.to_owned(), output.join(file));
        }
        Ok(Self { modules })
    }

    pub fn entry_point(&self, entry_point: &str) -> Result<SpirvArtifact<'_>, Error> {
        let (entry_point, path) = self
            .modules
            .get_key_value(entry_point)
            .ok_or_else(|| Error::MissingEntryPoint(entry_point.to_owned()))?;
        Ok(SpirvArtifact {
            entry_point,
            path: path.as_path(),
        })
    }

    pub fn iter(&self) -> impl Iterator<Item = SpirvArtifact<'_>> {
        self.modules
            .iter()
            .map(|(entry_point, path)| SpirvArtifact {
                entry_point,
                path: path.as_path(),
            })
    }
}

fn validate_output_name(output_name: &str) -> Result<(), Error> {
    let mut components = Path::new(output_name).components();
    match (components.next(), components.next()) {
        (Some(std::path::Component::Normal(_)), None) => Ok(()),
        _ => Err(Error::InvalidOutputName(output_name.to_owned())),
    }
}

fn env_path(name: &'static str) -> Result<PathBuf, Error> {
    env::var_os(name)
        .map(PathBuf::from)
        .ok_or(Error::MissingEnvironment(name))
}

fn resolve(base: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_owned()
    } else {
        base.join(path)
    }
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("compiler output name must be one normal path component, got {0:?}")]
    InvalidOutputName(String),
    #[error("environment variable {0} is not set")]
    MissingEnvironment(&'static str),
    #[error("failed to construct compiler PATH: {0}")]
    JoinPaths(#[source] env::JoinPathsError),
    #[error("failed to launch rust-gpu compiler: {0}")]
    CompilerLaunch(#[source] std::io::Error),
    #[error("rust-gpu compiler exited with {0}")]
    CompilerFailed(std::process::ExitStatus),
    #[error("failed to read rust-gpu module manifest {path}: {source}")]
    ReadManifest {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid rust-gpu module manifest line {0:?}")]
    InvalidManifestLine(String),
    #[error("rust-gpu did not emit entry point {0:?}")]
    MissingEntryPoint(String),
}
