use shaderc::{CompileOptions, Compiler, ShaderKind, SourceLanguage, IncludeType, ResolvedInclude, IncludeCallbackResult};
use std::path::PathBuf;
use std::{env, fs};

type Result<T, E = Box<dyn std::error::Error>> = std::result::Result<T, E>;

static NUM_TYPES: &[(&'static str, &'static str)] = &[
    ("bf16", "uint"),
    ("u32", "uint"),
    ("i32", "int"),
    ("f32", "float"),
    ("f64", "double"),
];

static NN_TYPES: &[(&'static str, &'static str)] = &[
    ("bf16", "uint"),
    ("f32", "float"),
];

fn glsl_options() -> CompileOptions<'static> {
    let mut options = CompileOptions::new().unwrap();
    options.set_auto_bind_uniforms(true);
    options.set_source_language(SourceLanguage::GLSL);
    options.set_include_callback(glsl_include_callback);
    options
}

fn glsl_include_callback(name: &str, _include_type: IncludeType, _src_name: &str, _src_depth: usize) -> IncludeCallbackResult {
    match name {
        "buffer_macros.comp" => Ok(ResolvedInclude {
                resolved_name: name.into(),
                content: include_str!("src/glsl/buffer_macros.comp").into()
        }),
        _ => Err(format!("Unrecognized include {:?}!", name))
    }
}

/*
#[allow(unused)]
#[derive(Clone, Copy)]
enum Behavior {
    Enable,
    Require,
    Warn,
    Disable
}

impl Behavior {
    fn to_str(&self) -> &'static str {
        use Behavior::*;
        match self {
            Enable => "enable",
            Require => "require",
            Warn => "warn",
            Disable => "disable",
        }
    }
}

fn glsl_extension(src: &mut String, extension: &str, behavior: Behavior) {
    let version_str = "#version 450";
    let index = src.find(version_str)
        .map(|i| i + version_str.len())
        .expect(src);
    src.insert_str(index, &format!("\n#extension {} : {}\n", extension, behavior.to_str()));
}

fn add_load_store_macros(options: &mut CompileOptions, rust_ty: &str) {
    if rust_ty == "bf16_as_f32" {
        options.add_macro_definition("LOAD(x)",  Some("uintBitsToFloat(uint(x) << 16)"));
        options.add_macro_definition("STORE(x)", Some("uint16_t(floatBitsToUint(x) >> 16)"));
    } else {
        options.add_macro_definition("LOAD(x)",  Some("x"));
        options.add_macro_definition("STORE(x)", Some("x"));
    }
}*/

fn compile_glsl(
    compiler: &mut Compiler,
    src: &str,
    name: &str,
    options: Option<&CompileOptions>,
) -> Result<()> {
    let artifact = compiler.compile_into_spirv(src, ShaderKind::Compute, name, "main", options)?;
    let glsl_path = PathBuf::from(env::var("AUTOGRAPH_DIR")?)
        .join("src")
        .join("shaders")
        .join("glsl");
    eprintln!("{:?}", glsl_path);
    fs::create_dir_all(&glsl_path)?;
    let fpath = glsl_path.join(&name).with_extension("spv");
    fs::write(&fpath, artifact.as_binary_u8())?;
    Ok(())
}

fn glsl_fill(compiler: &mut Compiler) -> Result<()> {
    let options = glsl_options();
    {
        let src = include_str!("src/glsl/fill_u32.comp");
        compile_glsl(compiler, src, "fill_u32", Some(&options))?;
    }
    {
        let src = include_str!("src/glsl/fill_u64.comp");
        compile_glsl(compiler, src, "fill_u64", Some(&options))?;
    }
    Ok(())
}

fn glsl_gemm(compiler: &mut Compiler) -> Result<()> {
    let src = include_str!("src/glsl/gemm.comp");
    for &(rust_ty, c_ty) in NUM_TYPES.iter() {
        let mut options = glsl_options();
        if rust_ty == "bf16" {
            options.add_macro_definition("BF16", None);
        } else if rust_ty == "f64" {
            options.add_macro_definition("F64", None);
        } else {
            options.add_macro_definition("T", Some(c_ty));
        }
        compile_glsl(compiler, src, &format!("gemm_{}", rust_ty), Some(&options))?;
    }
    for &(rust_ty, c_ty) in NN_TYPES.iter() {
        {
            // Relu
            let mut options = glsl_options();
            if rust_ty == "bf16" {
                options.add_macro_definition("BF16", None);
            } else {
                options.add_macro_definition("T", Some(c_ty));
            }
            options.add_macro_definition("RELU", None);
            compile_glsl(
                compiler,
                src,
                &format!("gemm_relu_{}", rust_ty),
                Some(&options),
            )?;
        }
        {
            // Bias
            let mut options = glsl_options();
            if rust_ty == "bf16" {
                options.add_macro_definition("BF16", None);
            } else {
                options.add_macro_definition("T", Some(c_ty));
            }
            options.add_macro_definition("BIAS", None);
            compile_glsl(
                compiler,
                src,
                &format!("gemm_bias_{}", rust_ty),
                Some(&options),
            )?;
        }
        {
            // Bias + Relu
            let mut options = glsl_options();
            if rust_ty == "bf16" {
                options.add_macro_definition("BF16", None);
            } else {
                options.add_macro_definition("T", Some(c_ty));
            }
            options.add_macro_definition("BIAS", None);
            options.add_macro_definition("RELU", None);
            compile_glsl(
                compiler,
                src,
                &format!("gemm_bias_relu_{}", rust_ty),
                Some(&options),
            )?;
        }
    }
    Ok(())
}

pub fn main() -> Result<()> {
    let mut compiler = Compiler::new().unwrap();
    glsl_fill(&mut compiler)?;
    glsl_gemm(&mut compiler)?;
    Ok(())
}
