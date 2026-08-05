//! Build-time generation of typed `wgpu` bindings from SPIR-V modules.

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::{self, Write as _},
    path::Path,
};

use rspirv_reflect::{
    BindingCount, DescriptorInfo, DescriptorType, Reflection,
    rspirv::{
        dr::{Instruction, Module, Operand},
        spirv::{Decoration, Dim, ExecutionModel, Op},
    },
};
use thiserror::Error;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShaderStage {
    Vertex,
    Fragment,
    Compute,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SamplerKind {
    Filtering,
    NonFiltering,
    Comparison,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TextureSampleType {
    Float { filterable: bool },
    Depth,
    Sint,
    Uint,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TextureDimension {
    D1,
    D2,
    D2Array,
    Cube,
    CubeArray,
    D3,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BindingKind {
    UniformBuffer,
    StorageBuffer {
        read_only: bool,
    },
    Sampler(SamplerKind),
    Texture {
        sample_type: TextureSampleType,
        dimension: TextureDimension,
        multisampled: bool,
    },
}

/// Optional host-side choices that cannot always be recovered from SPIR-V.
#[derive(Clone, Copy, Debug, Default)]
pub struct BindingOverride {
    pub group: u32,
    pub binding: u32,
    pub sampler_kind: Option<SamplerKind>,
    pub float_filterable: Option<bool>,
    pub storage_read_only: Option<bool>,
    pub dynamic_offset: Option<bool>,
    pub min_binding_size: Option<u64>,
}

#[derive(Clone, Copy, Debug)]
pub struct EntryPointConfig<'a> {
    pub spirv_name: &'a str,
    pub rust_name: &'a str,
    pub spirv_path: &'a Path,
}

#[derive(Clone, Debug)]
pub struct PipelineLayoutConfig<'a> {
    pub rust_module_name: &'a str,
    pub entry_points: &'a [EntryPointConfig<'a>],
    pub binding_overrides: &'a [BindingOverride],
}

#[derive(Debug, Error)]
pub enum GenerateError {
    #[error("failed to read SPIR-V module {path}: {source}")]
    Read {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid SPIR-V module {path}: {message}")]
    InvalidSpirv { path: String, message: String },
    #[error("entry point {entry:?} was not found in SPIR-V module {path}")]
    MissingEntry { entry: String, path: String },
    #[error("entry point {entry:?} has unsupported execution model {model:?}")]
    UnsupportedStage {
        entry: String,
        model: ExecutionModel,
    },
    #[error("binding override {group}:{binding} was not reflected")]
    UnusedBindingOverride { group: u32, binding: u32 },
    #[error("binding {group}:{binding} has no preserved SPIR-V name")]
    MissingBindingName { group: u32, binding: u32 },
    #[error("cannot infer SPIR-V binding {group}:{binding}: {message}")]
    BindingInference {
        group: u32,
        binding: u32,
        message: String,
    },
    #[error("binding arrays are not yet supported at {group}:{binding}: {count:?}")]
    UnsupportedBindingCount {
        group: u32,
        binding: u32,
        count: BindingCount,
    },
    #[error("binding {group}:{binding} differs between entry points")]
    ConflictingBinding { group: u32, binding: u32 },
    #[error("pipeline {0:?} has no entry points")]
    EmptyPipeline(String),
    #[error("invalid Rust identifier {0:?}")]
    InvalidIdentifier(String),
    #[error("generated invalid Rust source: {0}")]
    InvalidGeneratedSource(#[from] syn::Error),
}

#[derive(Clone, Debug)]
struct ReflectedBinding {
    name: String,
    kind: BindingKind,
    dynamic_offset: bool,
    min_binding_size: Option<u64>,
    visibility: u8,
    reflected: DescriptorInfo,
}

#[derive(Clone, Debug)]
struct ReflectedEntry<'a> {
    spirv_name: String,
    rust_name: &'a str,
    spirv_path: &'a Path,
    stage: ShaderStage,
    color_targets: usize,
}

const VERTEX: u8 = 1;
const FRAGMENT: u8 = 2;
const COMPUTE: u8 = 4;

#[derive(Clone, Debug, Default)]
pub struct Builder<'a> {
    pipeline_layouts: Vec<PipelineLayoutConfig<'a>>,
}

impl<'a> Builder<'a> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn pipeline_layout(mut self, config: PipelineLayoutConfig<'a>) -> Self {
        self.pipeline_layouts.push(config);
        self
    }

    pub fn generate(self) -> Result<Bindings, GenerateError> {
        let mut source = String::new();
        for (index, config) in self.pipeline_layouts.iter().enumerate() {
            if index != 0 {
                source.push('\n');
            }
            source.push_str(&generate_pipeline_layout(config)?);
        }
        Ok(Bindings { source })
    }
}

#[derive(Clone, Debug)]
pub struct Bindings {
    source: String,
}

impl Bindings {
    pub fn write_to_file(&self, path: impl AsRef<Path>) -> std::io::Result<()> {
        let mut file = std::fs::File::create(path)?;
        self.write(&mut file)
    }

    pub fn write(&self, mut writer: impl std::io::Write) -> std::io::Result<()> {
        writer.write_all(self.source.as_bytes())
    }
}

impl fmt::Display for Bindings {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.source)
    }
}

fn generate_pipeline_layout(config: &PipelineLayoutConfig<'_>) -> Result<String, GenerateError> {
    ident(config.rust_module_name)?;
    if config.entry_points.is_empty() {
        return Err(GenerateError::EmptyPipeline(
            config.rust_module_name.to_owned(),
        ));
    }

    let overrides = config
        .binding_overrides
        .iter()
        .map(|binding| ((binding.group, binding.binding), binding))
        .collect::<BTreeMap<_, _>>();
    let mut bindings: BTreeMap<(u32, u32), ReflectedBinding> = BTreeMap::new();
    let mut entries = Vec::new();

    for entry in config.entry_points {
        ident(entry.rust_name)?;
        let bytes = std::fs::read(entry.spirv_path).map_err(|source| GenerateError::Read {
            path: entry.spirv_path.display().to_string(),
            source,
        })?;
        let reflection =
            Reflection::new_from_spirv(&bytes).map_err(|error| GenerateError::InvalidSpirv {
                path: entry.spirv_path.display().to_string(),
                message: error.to_string(),
            })?;
        let reflected_entry = reflect_entry(&reflection, entry)?;
        let stage = reflected_entry.stage;
        let visibility = stage_bit(stage);
        let descriptor_sets = descriptor_sets_for_entry(&reflection, entry).map_err(|error| {
            GenerateError::InvalidSpirv {
                path: entry.spirv_path.display().to_string(),
                message: error.to_string(),
            }
        })?;

        for (group, set) in descriptor_sets {
            for (binding, reflected) in set {
                validate_binding_count(group, binding, &reflected)?;
                let binding_override = overrides
                    .get(&(group, binding))
                    .copied()
                    .copied()
                    .unwrap_or_default();
                let inferred =
                    infer_binding(&reflection.0, group, binding, &reflected, binding_override)?;
                match bindings.get_mut(&(group, binding)) {
                    Some(existing) => {
                        if existing.name != inferred.name
                            || existing.kind != inferred.kind
                            || existing.dynamic_offset != inferred.dynamic_offset
                            || existing.min_binding_size != inferred.min_binding_size
                            || existing.reflected.ty != reflected.ty
                            || existing.reflected.binding_count != reflected.binding_count
                        {
                            return Err(GenerateError::ConflictingBinding { group, binding });
                        }
                        existing.visibility |= visibility;
                    }
                    None => {
                        bindings.insert(
                            (group, binding),
                            ReflectedBinding {
                                visibility,
                                ..inferred
                            },
                        );
                    }
                }
            }
        }
        entries.push(reflected_entry);
    }

    for binding in config.binding_overrides {
        if !bindings.contains_key(&(binding.group, binding.binding)) {
            return Err(GenerateError::UnusedBindingOverride {
                group: binding.group,
                binding: binding.binding,
            });
        }
    }

    let mut output = String::new();
    writeln!(
        output,
        "// File automatically generated from rust-gpu SPIR-V."
    )
    .unwrap();
    writeln!(output, "pub mod {} {{", config.rust_module_name).unwrap();
    emit_bind_groups(&mut output, &bindings);
    emit_entries(&mut output, &entries);
    emit_pipeline_layout(&mut output, &bindings);
    writeln!(output, "}}").unwrap();
    Ok(prettyplease::unparse(&syn::parse_file(&output)?))
}

fn descriptor_sets_for_entry(
    reflection: &Reflection,
    entry: &EntryPointConfig<'_>,
) -> Result<BTreeMap<u32, BTreeMap<u32, DescriptorInfo>>, rspirv_reflect::ReflectError> {
    let module = &reflection.0;
    let entry_function = module
        .entry_points
        .iter()
        .find_map(|instruction| match instruction.operands.as_slice() {
            [
                _,
                Operand::IdRef(function),
                Operand::LiteralString(name),
                ..,
            ] if name == entry.spirv_name => Some(*function),
            _ => None,
        })
        .expect("entry point was checked before descriptor reflection");

    let resource_variables = module
        .types_global_values
        .iter()
        .filter(|instruction| instruction.class.opcode == Op::Variable)
        .filter(|instruction| {
            matches!(
                instruction.operands.first(),
                Some(Operand::StorageClass(
                    rspirv_reflect::rspirv::spirv::StorageClass::Uniform
                        | rspirv_reflect::rspirv::spirv::StorageClass::UniformConstant
                        | rspirv_reflect::rspirv::spirv::StorageClass::StorageBuffer
                ))
            )
        })
        .filter_map(|instruction| instruction.result_id)
        .collect::<BTreeSet<_>>();
    let function_ids = module
        .functions
        .iter()
        .filter_map(|function| function.def.as_ref()?.result_id)
        .collect::<BTreeSet<_>>();

    let mut used_resources = BTreeSet::new();
    let mut visited_functions = BTreeSet::new();
    let mut pending_functions = vec![entry_function];
    while let Some(function_id) = pending_functions.pop() {
        if !visited_functions.insert(function_id) {
            continue;
        }
        let Some(function) = module.functions.iter().find(|function| {
            function.def.as_ref().and_then(|def| def.result_id) == Some(function_id)
        }) else {
            continue;
        };
        for instruction in function
            .parameters
            .iter()
            .chain(function.blocks.iter().flat_map(|block| &block.instructions))
        {
            for operand in &instruction.operands {
                let Operand::IdRef(id) = operand else {
                    continue;
                };
                if resource_variables.contains(id) {
                    used_resources.insert(*id);
                }
                if function_ids.contains(id) && !visited_functions.contains(id) {
                    pending_functions.push(*id);
                }
            }
        }
    }

    let mut scoped_module = module.clone();
    scoped_module.types_global_values.retain(|instruction| {
        instruction.class.opcode != Op::Variable
            || instruction
                .result_id
                .is_none_or(|id| !resource_variables.contains(&id) || used_resources.contains(&id))
    });
    Reflection::new(scoped_module).get_descriptor_sets()
}

fn reflect_entry<'a>(
    reflection: &Reflection,
    entry: &EntryPointConfig<'a>,
) -> Result<ReflectedEntry<'a>, GenerateError> {
    let path = entry.spirv_path.display().to_string();
    let entry_point = reflection.0.entry_points.iter().find_map(|instruction| {
        let [
            Operand::ExecutionModel(model),
            _,
            Operand::LiteralString(name),
            interfaces @ ..,
        ] = instruction.operands.as_slice()
        else {
            return None;
        };
        (name == entry.spirv_name).then_some((*model, name.as_str(), interfaces))
    });
    let Some((model, spirv_name, interfaces)) = entry_point else {
        return Err(GenerateError::MissingEntry {
            entry: entry.spirv_name.to_owned(),
            path,
        });
    };
    let stage = match model {
        ExecutionModel::Vertex => ShaderStage::Vertex,
        ExecutionModel::Fragment => ShaderStage::Fragment,
        ExecutionModel::GLCompute => ShaderStage::Compute,
        model => {
            return Err(GenerateError::UnsupportedStage {
                entry: spirv_name.to_owned(),
                model,
            });
        }
    };
    let color_targets = if stage == ShaderStage::Fragment {
        fragment_color_target_count(&reflection.0, interfaces)
    } else {
        0
    };
    Ok(ReflectedEntry {
        spirv_name: spirv_name.to_owned(),
        rust_name: entry.rust_name,
        spirv_path: entry.spirv_path,
        stage,
        color_targets,
    })
}

fn fragment_color_target_count(module: &Module, interfaces: &[Operand]) -> usize {
    interfaces
        .iter()
        .filter_map(|operand| match operand {
            Operand::IdRef(id) => decoration_literal(module, *id, Decoration::Location),
            _ => None,
        })
        .max()
        .map_or(0, |location| location as usize + 1)
}

fn validate_binding_count(
    group: u32,
    binding: u32,
    reflected: &DescriptorInfo,
) -> Result<(), GenerateError> {
    if reflected.binding_count != BindingCount::One {
        return Err(GenerateError::UnsupportedBindingCount {
            group,
            binding,
            count: reflected.binding_count.clone(),
        });
    }
    Ok(())
}

fn infer_binding(
    module: &Module,
    group: u32,
    binding: u32,
    reflected: &DescriptorInfo,
    binding_override: BindingOverride,
) -> Result<ReflectedBinding, GenerateError> {
    if reflected.name.is_empty() {
        return Err(GenerateError::MissingBindingName { group, binding });
    }
    ident(&reflected.name)?;

    let kind = if reflected.ty == DescriptorType::UNIFORM_BUFFER {
        BindingKind::UniformBuffer
    } else if reflected.ty == DescriptorType::STORAGE_BUFFER {
        let variable = find_resource_variable(module, group, binding)?;
        let pointee = pointee_type(module, variable)?;
        let read_only = binding_override.storage_read_only.unwrap_or_else(|| {
            has_decoration(module, variable.result_id.unwrap(), Decoration::NonWritable)
                || has_decoration(module, pointee.result_id.unwrap(), Decoration::NonWritable)
        });
        BindingKind::StorageBuffer { read_only }
    } else if reflected.ty == DescriptorType::SAMPLER {
        BindingKind::Sampler(
            binding_override
                .sampler_kind
                .unwrap_or(SamplerKind::Filtering),
        )
    } else if reflected.ty == DescriptorType::SAMPLED_IMAGE {
        infer_sampled_image(module, group, binding, binding_override.float_filterable)?
    } else {
        return Err(binding_inference(
            group,
            binding,
            format!("unsupported descriptor type {:?}", reflected.ty),
        ));
    };

    let is_buffer = matches!(
        kind,
        BindingKind::UniformBuffer | BindingKind::StorageBuffer { .. }
    );
    if !is_buffer
        && (binding_override.dynamic_offset.is_some()
            || binding_override.min_binding_size.is_some()
            || binding_override.storage_read_only.is_some())
    {
        return Err(binding_inference(
            group,
            binding,
            "buffer-only override applied to a non-buffer resource",
        ));
    }
    if !matches!(kind, BindingKind::Sampler(_)) && binding_override.sampler_kind.is_some() {
        return Err(binding_inference(
            group,
            binding,
            "sampler override applied to a non-sampler resource",
        ));
    }
    if !matches!(kind, BindingKind::Texture { .. }) && binding_override.float_filterable.is_some() {
        return Err(binding_inference(
            group,
            binding,
            "texture override applied to a non-texture resource",
        ));
    }

    Ok(ReflectedBinding {
        name: reflected.name.clone(),
        kind,
        dynamic_offset: binding_override.dynamic_offset.unwrap_or(false),
        min_binding_size: binding_override.min_binding_size,
        visibility: 0,
        reflected: reflected.clone(),
    })
}

fn infer_sampled_image(
    module: &Module,
    group: u32,
    binding: u32,
    float_filterable: Option<bool>,
) -> Result<BindingKind, GenerateError> {
    let variable = find_resource_variable(module, group, binding)?;
    let image = pointee_type(module, variable)?;
    if image.class.opcode != Op::TypeImage {
        return Err(binding_inference(
            group,
            binding,
            "descriptor does not point to OpTypeImage",
        ));
    }
    let [
        Operand::IdRef(sampled_type),
        Operand::Dim(dim),
        Operand::LiteralBit32(depth),
        Operand::LiteralBit32(arrayed),
        Operand::LiteralBit32(multisampled),
        ..,
    ] = image.operands.as_slice()
    else {
        return Err(binding_inference(group, binding, "malformed OpTypeImage"));
    };

    let dimension = match (*dim, *arrayed != 0) {
        (Dim::Dim1D, false) => TextureDimension::D1,
        (Dim::Dim2D, false) => TextureDimension::D2,
        (Dim::Dim2D, true) => TextureDimension::D2Array,
        (Dim::DimCube, false) => TextureDimension::Cube,
        (Dim::DimCube, true) => TextureDimension::CubeArray,
        (Dim::Dim3D, false) => TextureDimension::D3,
        _ => {
            return Err(binding_inference(
                group,
                binding,
                format!("unsupported image dimension {dim:?}, arrayed={arrayed}"),
            ));
        }
    };
    let sample_type = if *depth == 1 {
        TextureSampleType::Depth
    } else {
        let scalar = assignment(module, *sampled_type)
            .ok_or_else(|| binding_inference(group, binding, "sampled scalar type is missing"))?;
        match scalar.class.opcode {
            Op::TypeFloat => TextureSampleType::Float {
                filterable: float_filterable.unwrap_or(true),
            },
            Op::TypeInt => match scalar.operands.as_slice() {
                [Operand::LiteralBit32(_), Operand::LiteralBit32(0)] => TextureSampleType::Uint,
                [Operand::LiteralBit32(_), Operand::LiteralBit32(1)] => TextureSampleType::Sint,
                _ => return Err(binding_inference(group, binding, "malformed OpTypeInt")),
            },
            op => {
                return Err(binding_inference(
                    group,
                    binding,
                    format!("unsupported sampled scalar type {op:?}"),
                ));
            }
        }
    };
    Ok(BindingKind::Texture {
        sample_type,
        dimension,
        multisampled: *multisampled != 0,
    })
}

fn find_resource_variable(
    module: &Module,
    group: u32,
    binding: u32,
) -> Result<&Instruction, GenerateError> {
    module
        .types_global_values
        .iter()
        .filter(|instruction| instruction.class.opcode == Op::Variable)
        .find(|instruction| {
            let id = instruction.result_id.unwrap();
            decoration_literal(module, id, Decoration::DescriptorSet) == Some(group)
                && decoration_literal(module, id, Decoration::Binding) == Some(binding)
        })
        .ok_or_else(|| binding_inference(group, binding, "descriptor variable is missing"))
}

fn pointee_type<'a>(
    module: &'a Module,
    variable: &Instruction,
) -> Result<&'a Instruction, GenerateError> {
    let pointer_id = variable.result_type.unwrap();
    let pointer = assignment(module, pointer_id).ok_or_else(|| GenerateError::InvalidSpirv {
        path: "<module>".to_owned(),
        message: format!("pointer type %{pointer_id} is missing"),
    })?;
    let Some(Operand::IdRef(pointee_id)) = pointer.operands.last() else {
        return Err(GenerateError::InvalidSpirv {
            path: "<module>".to_owned(),
            message: format!("pointer type %{pointer_id} has no pointee"),
        });
    };
    assignment(module, *pointee_id).ok_or_else(|| GenerateError::InvalidSpirv {
        path: "<module>".to_owned(),
        message: format!("pointee type %{pointee_id} is missing"),
    })
}

fn assignment(module: &Module, id: u32) -> Option<&Instruction> {
    module
        .types_global_values
        .iter()
        .find(|instruction| instruction.result_id == Some(id))
}

fn decoration_literal(module: &Module, id: u32, decoration: Decoration) -> Option<u32> {
    module.annotations.iter().find_map(|instruction| {
        if instruction.class.opcode != Op::Decorate {
            return None;
        }
        match instruction.operands.as_slice() {
            [
                Operand::IdRef(target),
                Operand::Decoration(found),
                Operand::LiteralBit32(value),
                ..,
            ] if *target == id && *found == decoration => Some(*value),
            _ => None,
        }
    })
}

fn has_decoration(module: &Module, id: u32, decoration: Decoration) -> bool {
    module.annotations.iter().any(|instruction| {
        instruction.class.opcode == Op::Decorate
            && matches!(
                instruction.operands.as_slice(),
                [Operand::IdRef(target), Operand::Decoration(found), ..]
                    if *target == id && *found == decoration
            )
    })
}

fn binding_inference(group: u32, binding: u32, message: impl Into<String>) -> GenerateError {
    GenerateError::BindingInference {
        group,
        binding,
        message: message.into(),
    }
}

fn emit_bind_groups(output: &mut String, bindings: &BTreeMap<(u32, u32), ReflectedBinding>) {
    writeln!(output, "pub mod bind_groups {{").unwrap();
    let groups = bindings
        .keys()
        .map(|(group, _)| *group)
        .collect::<std::collections::BTreeSet<_>>();
    for group in &groups {
        let set = bindings
            .iter()
            .filter(|((g, _), _)| g == group)
            .collect::<Vec<_>>();
        writeln!(
            output,
            "#[derive(Debug)] pub struct BindGroup{group}(wgpu::BindGroup);"
        )
        .unwrap();
        writeln!(
            output,
            "#[derive(Debug)] pub struct BindGroupLayout{group}<'a> {{"
        )
        .unwrap();
        for (_, binding) in &set {
            writeln!(
                output,
                "pub {}: {},",
                binding.name,
                resource_field_type(binding.kind)
            )
            .unwrap();
        }
        writeln!(output, "}}").unwrap();
        writeln!(output, "impl BindGroup{group} {{").unwrap();
        writeln!(output, "pub fn get_bind_group_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {{ device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {{ label: Some(\"LayoutDescriptor{group}\"), entries: &[").unwrap();
        for ((_, index), binding) in &set {
            writeln!(output, "wgpu::BindGroupLayoutEntry {{ binding: {index}, visibility: {}, ty: {}, count: None }},", visibility_code(binding.visibility), binding_type_code(binding)).unwrap();
        }
        writeln!(output, "] }}) }}").unwrap();
        writeln!(output, "pub fn from_bindings(device: &wgpu::Device, bindings: BindGroupLayout{group}) -> Self {{ let layout = Self::get_bind_group_layout(device); Self(device.create_bind_group(&wgpu::BindGroupDescriptor {{ label: Some(\"BindGroup{group}\"), layout: &layout, entries: &[").unwrap();
        for ((_, index), binding) in &set {
            writeln!(
                output,
                "wgpu::BindGroupEntry {{ binding: {index}, resource: {} }},",
                resource_code(&binding.name, binding.kind)
            )
            .unwrap();
        }
        writeln!(output, "] }})) }}").unwrap();
        writeln!(output, "pub fn set<P: SetBindGroup>(&self, pass: &mut P) {{ pass.set_bind_group({group}, &self.0, &[]); }} }}").unwrap();
    }
    writeln!(
        output,
        "#[derive(Debug, Copy, Clone)] pub struct BindGroups<'a> {{"
    )
    .unwrap();
    for group in &groups {
        writeln!(output, "pub bind_group{group}: &'a BindGroup{group},").unwrap();
    }
    writeln!(output, "}}").unwrap();
    writeln!(
        output,
        "impl BindGroups<'_> {{ pub fn set<P: SetBindGroup>(&self, pass: &mut P) {{"
    )
    .unwrap();
    for group in &groups {
        writeln!(output, "self.bind_group{group}.set(pass);").unwrap();
    }
    writeln!(output, "}} }}").unwrap();
    writeln!(output, "pub trait SetBindGroup {{ fn set_bind_group(&mut self, index: u32, bind_group: &wgpu::BindGroup, offsets: &[wgpu::DynamicOffset]); }}").unwrap();
    for pass in ["ComputePass", "RenderPass", "RenderBundleEncoder"] {
        writeln!(output, "impl SetBindGroup for wgpu::{pass}<'_> {{ fn set_bind_group(&mut self, index: u32, bind_group: &wgpu::BindGroup, offsets: &[wgpu::DynamicOffset]) {{ self.set_bind_group(index, bind_group, offsets); }} }}").unwrap();
    }
    writeln!(output, "}}").unwrap();
    write!(
        output,
        "pub fn set_bind_groups<P: bind_groups::SetBindGroup>(pass: &mut P"
    )
    .unwrap();
    for group in &groups {
        write!(
            output,
            ", bind_group{group}: &bind_groups::BindGroup{group}"
        )
        .unwrap();
    }
    writeln!(output, ") {{").unwrap();
    for group in &groups {
        writeln!(output, "bind_group{group}.set(pass);").unwrap();
    }
    writeln!(output, "}}").unwrap();
}

fn emit_entries(output: &mut String, entries: &[ReflectedEntry<'_>]) {
    writeln!(output, "pub struct ShaderModules {{").unwrap();
    for entry in entries {
        writeln!(output, "pub {}: wgpu::ShaderModule,", entry.rust_name).unwrap();
    }
    writeln!(output, "}}").unwrap();
    writeln!(
        output,
        "pub fn create_shader_modules(device: &wgpu::Device) -> ShaderModules {{ ShaderModules {{"
    )
    .unwrap();
    for entry in entries {
        let path = entry.spirv_path.display().to_string().replace('\\', "\\\\");
        writeln!(output, "{}: device.create_shader_module(wgpu::ShaderModuleDescriptor {{ label: Some({:?}), source: wgpu::util::make_spirv(include_bytes!({:?})) }}),", entry.rust_name, entry.spirv_name, path).unwrap();
    }
    writeln!(output, "}} }}").unwrap();
    for entry in entries {
        let upper = entry.rust_name.to_ascii_uppercase();
        writeln!(
            output,
            "pub const ENTRY_{upper}: &str = {:?};",
            entry.spirv_name
        )
        .unwrap();
        match entry.stage {
            ShaderStage::Vertex => {
                writeln!(output, "pub fn {}_state(module: &wgpu::ShaderModule) -> wgpu::VertexState<'_> {{ wgpu::VertexState {{ module, entry_point: Some(ENTRY_{upper}), buffers: &[], compilation_options: Default::default() }} }}", entry.rust_name).unwrap();
            }
            ShaderStage::Fragment => {
                let n = entry.color_targets;
                writeln!(output, "pub fn {}_state<'a>(module: &'a wgpu::ShaderModule, targets: &'a [Option<wgpu::ColorTargetState>; {n}]) -> wgpu::FragmentState<'a> {{ wgpu::FragmentState {{ module, entry_point: Some(ENTRY_{upper}), targets, compilation_options: Default::default() }} }}", entry.rust_name).unwrap();
            }
            ShaderStage::Compute => {
                writeln!(output, "pub fn create_{}_pipeline(device: &wgpu::Device) -> wgpu::ComputePipeline {{ let modules = create_shader_modules(device); let layout = create_pipeline_layout(device); device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {{ label: Some({:?}), layout: Some(&layout), module: &modules.{}, entry_point: Some(ENTRY_{upper}), compilation_options: Default::default(), cache: None }}) }}", entry.rust_name, format!("Compute Pipeline {}", entry.rust_name), entry.rust_name).unwrap();
            }
        }
    }
}

fn emit_pipeline_layout(output: &mut String, bindings: &BTreeMap<(u32, u32), ReflectedBinding>) {
    let groups = bindings
        .keys()
        .map(|(group, _)| *group)
        .collect::<std::collections::BTreeSet<_>>();
    writeln!(output, "pub fn create_pipeline_layout(device: &wgpu::Device) -> wgpu::PipelineLayout {{ device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {{ label: None, bind_group_layouts: &[").unwrap();
    for group in groups {
        writeln!(
            output,
            "&bind_groups::BindGroup{group}::get_bind_group_layout(device),"
        )
        .unwrap();
    }
    writeln!(output, "], push_constant_ranges: &[] }}) }}").unwrap();
}

fn resource_field_type(kind: BindingKind) -> &'static str {
    match kind {
        BindingKind::UniformBuffer | BindingKind::StorageBuffer { .. } => "wgpu::BufferBinding<'a>",
        BindingKind::Sampler(_) => "&'a wgpu::Sampler",
        BindingKind::Texture { .. } => "&'a wgpu::TextureView",
    }
}

fn resource_code(name: &str, kind: BindingKind) -> String {
    match kind {
        BindingKind::UniformBuffer | BindingKind::StorageBuffer { .. } => {
            format!("wgpu::BindingResource::Buffer(bindings.{name})")
        }
        BindingKind::Sampler(_) => format!("wgpu::BindingResource::Sampler(bindings.{name})"),
        BindingKind::Texture { .. } => {
            format!("wgpu::BindingResource::TextureView(bindings.{name})")
        }
    }
}

fn binding_type_code(binding: &ReflectedBinding) -> String {
    let min_binding_size = binding.min_binding_size.map_or_else(
        || "None".to_owned(),
        |size| format!("Some(std::num::NonZeroU64::new({size}).unwrap())"),
    );
    match binding.kind {
        BindingKind::UniformBuffer => format!(
            "wgpu::BindingType::Buffer {{ ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: {}, min_binding_size: {min_binding_size} }}",
            binding.dynamic_offset
        ),
        BindingKind::StorageBuffer { read_only } => format!(
            "wgpu::BindingType::Buffer {{ ty: wgpu::BufferBindingType::Storage {{ read_only: {read_only} }}, has_dynamic_offset: {}, min_binding_size: {min_binding_size} }}",
            binding.dynamic_offset
        ),
        BindingKind::Sampler(kind) => format!(
            "wgpu::BindingType::Sampler(wgpu::SamplerBindingType::{})",
            match kind {
                SamplerKind::Filtering => "Filtering",
                SamplerKind::NonFiltering => "NonFiltering",
                SamplerKind::Comparison => "Comparison",
            }
        ),
        BindingKind::Texture {
            sample_type,
            dimension,
            multisampled,
        } => format!(
            "wgpu::BindingType::Texture {{ sample_type: {}, view_dimension: wgpu::TextureViewDimension::{}, multisampled: {multisampled} }}",
            sample_type_code(sample_type),
            dimension_code(dimension)
        ),
    }
}

fn sample_type_code(ty: TextureSampleType) -> String {
    match ty {
        TextureSampleType::Float { filterable } => {
            format!("wgpu::TextureSampleType::Float {{ filterable: {filterable} }}")
        }
        TextureSampleType::Depth => "wgpu::TextureSampleType::Depth".to_owned(),
        TextureSampleType::Sint => "wgpu::TextureSampleType::Sint".to_owned(),
        TextureSampleType::Uint => "wgpu::TextureSampleType::Uint".to_owned(),
    }
}

fn dimension_code(dimension: TextureDimension) -> &'static str {
    match dimension {
        TextureDimension::D1 => "D1",
        TextureDimension::D2 => "D2",
        TextureDimension::D2Array => "D2Array",
        TextureDimension::Cube => "Cube",
        TextureDimension::CubeArray => "CubeArray",
        TextureDimension::D3 => "D3",
    }
}

fn visibility_code(bits: u8) -> String {
    let mut stages = Vec::new();
    if bits & VERTEX != 0 {
        stages.push("wgpu::ShaderStages::VERTEX");
    }
    if bits & FRAGMENT != 0 {
        stages.push("wgpu::ShaderStages::FRAGMENT");
    }
    if bits & COMPUTE != 0 {
        stages.push("wgpu::ShaderStages::COMPUTE");
    }
    stages.join(" | ")
}

fn stage_bit(stage: ShaderStage) -> u8 {
    match stage {
        ShaderStage::Vertex => VERTEX,
        ShaderStage::Fragment => FRAGMENT,
        ShaderStage::Compute => COMPUTE,
    }
}

fn ident(value: &str) -> Result<(), GenerateError> {
    let mut chars = value.chars();
    if !matches!(chars.next(), Some('_' | 'a'..='z' | 'A'..='Z'))
        || !chars.all(|c| matches!(c, '_' | 'a'..='z' | 'A'..='Z' | '0'..='9'))
    {
        return Err(GenerateError::InvalidIdentifier(value.to_owned()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visibility_is_combined_deterministically() {
        assert_eq!(
            visibility_code(VERTEX | FRAGMENT),
            "wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT"
        );
    }

    #[test]
    fn rejects_invalid_identifiers() {
        assert!(ident("scan-image").is_err());
        assert!(ident("scan_image").is_ok());
    }

    #[test]
    fn emits_explicit_texture_properties() {
        assert_eq!(
            sample_type_code(TextureSampleType::Float { filterable: true }),
            "wgpu::TextureSampleType::Float { filterable: true }"
        );
        assert_eq!(dimension_code(TextureDimension::D1), "D1");
    }
}
