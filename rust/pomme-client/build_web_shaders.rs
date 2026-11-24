// Browser translation of the same shader sources the native renderer uses.
use naga::{
    AddressSpace, ImageClass, ImageDimension, ResourceBinding, ScalarKind, StorageAccess, TypeInner,
    back::wgsl::{WriterFlags, write_string},
    front::spv::{Options, parse_u8_slice},
    valid::{Capabilities, ValidationFlags, Validator},
};

/// Former push constants become a uniform at this placeholder group; the
/// browser backend renumbers it to the first group a pipeline leaves free.
const PUSH_UNIFORM: ResourceBinding = ResourceBinding { group: 9, binding: 0 };

/// WGSL, headed by a `// pomme-bindings` comment listing each binding as
/// `group:binding:kind`, which WebGPU bind group layouts must name exactly.
pub fn translate(bytes: &[u8]) -> Result<String, String> {
    let spv = without_write_only_decoration(bytes)?;
    let mut module = parse_u8_slice(&spv, &Options {
        strict_capabilities: false,
        ..Default::default()
    }).map_err(|e| e.to_string())?;
    for (_, var) in module.global_variables.iter_mut() {
        if var.space == AddressSpace::Immediate {
            var.space = AddressSpace::Uniform;
            var.binding = Some(PUSH_UNIFORM.clone());
        }
    }
    let mut header = String::from("// pomme-bindings");
    for (_, var) in module.global_variables.iter() {
        let Some(binding) = &var.binding else { continue };
        if *binding == PUSH_UNIFORM {
            continue;
        }
        let kind = match (var.space, &module.types[var.ty].inner) {
            (AddressSpace::Uniform, _) => "u".to_owned(),
            (AddressSpace::Storage { access }, _) if access.contains(StorageAccess::STORE) => "sw".to_owned(),
            (AddressSpace::Storage { .. }, _) => "sr".to_owned(),
            (_, TypeInner::Sampler { comparison: true }) => "sc".to_owned(),
            (_, TypeInner::Sampler { comparison: false }) => "sf".to_owned(),
            (_, TypeInner::Image { dim, arrayed, class }) => {
                let sample = match class {
                    ImageClass::Depth { .. } => "td",
                    ImageClass::Sampled { kind: ScalarKind::Uint, .. } => "tu",
                    ImageClass::Sampled { kind: ScalarKind::Sint, .. } => "ti",
                    _ => "tf",
                };
                let dim = match (dim, arrayed) {
                    (ImageDimension::D1, _) => "1d",
                    (ImageDimension::D2, false) => "2d",
                    (ImageDimension::D2, true) => "2d-array",
                    (ImageDimension::D3, _) => "3d",
                    (ImageDimension::Cube, false) => "cube",
                    (ImageDimension::Cube, true) => "cube-array",
                };
                format!("{sample}:{dim}")
            }
            _ => continue,
        };
        header += &format!(" {}:{}:{kind}", binding.group, binding.binding);
    }
    let info = Validator::new(ValidationFlags::all(), Capabilities::all())
        .validate(&module).map_err(|e| e.emit_to_string(""))?;
    let wgsl = write_string(&module, &info, WriterFlags::empty()).map_err(|e| e.to_string())?;
    Ok(format!("{header}\n{wgsl}"))
}

fn without_write_only_decoration(bytes: &[u8]) -> Result<Vec<u8>, String> {
    const NON_READABLE: u32 = 25;
    const MAGIC: [u8; 4] = 0x0723_0203u32.to_le_bytes();
    if bytes.len() < 20 || !bytes.len().is_multiple_of(4) || !bytes.starts_with(&MAGIC) {
        return Err("expected little-endian SPIR-V".into());
    }
    let mut output = bytes[..20].to_vec();
    let mut index = 20;
    while index < bytes.len() {
        let word = u32::from_le_bytes(bytes[index..index + 4].try_into().unwrap());
        let word_count = (word >> 16) as usize;
        let end = index + word_count * 4;
        if word_count == 0 || end > bytes.len() {
            return Err("invalid SPIR-V instruction length".into());
        }
        // OpDecorate / OpMemberDecorate. WGSL storage buffers may be
        // read-only or read/write, but not write-only.
        let decoration = match word & 0xffff {
            71 => Some(2),
            72 => Some(3),
            _ => None,
        };
        let remove = decoration.is_some_and(|i| {
            i < word_count && bytes[index + i * 4..index + (i + 1) * 4] == NON_READABLE.to_le_bytes()
        });
        if !remove {
            output.extend_from_slice(&bytes[index..end]);
        }
        index = end;
    }
    Ok(output)
}
