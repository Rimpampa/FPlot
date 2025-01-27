use shaderc;
use std::env;
use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

fn main() {
    // Tell the build script to only run again if we change our source shaders
    let shaders_source_dir = Path::new("src/renderer/vk/shaders");
    println!(
        "cargo:rerun-if-changed={}",
        shaders_source_dir.to_str().unwrap()
    );

    // Create destination path
    let mut out_dir = PathBuf::new();
    out_dir.push(
        std::env::var_os("CARGO_MANIFEST_DIR")
            .unwrap()
            .to_str()
            .unwrap(),
    );
    out_dir.push("assets");
    out_dir.push("shaders-spirv");
    std::fs::create_dir_all(out_dir.as_path());

    // Create the compiler
    let mut compiler = shaderc::Compiler::new().unwrap();

    let err = compile_recursively(shaders_source_dir, out_dir.as_path(), &mut compiler);
    if err {
        panic!("Some shaders did not compile")
    }
}

fn compile_recursively<T: AsRef<Path>>(
    source_dir: T,
    out_dir: T,
    compiler: &mut shaderc::Compiler,
) -> bool {
    let mut is_there_an_error = false;
    for entry in std::fs::read_dir(source_dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_file() {
            let shader_kind = match path.extension().unwrap().to_str() {
                Some("vert") => shaderc::ShaderKind::Vertex,
                Some("frag") => shaderc::ShaderKind::Fragment,
                _ => {
                    continue;
                }
            };
            let mut shader_file = File::open(&path).unwrap();
            let mut shader_contents = String::new();
            shader_file.read_to_string(&mut shader_contents);
            let compilation_result = compiler.compile_into_spirv(
                &shader_contents,
                shader_kind,
                path.to_str().unwrap(),
                "main",
                None,
            );
            match compilation_result {
                Ok(v) => {
                    println!("Shader {} compiled successfully", path.to_str().unwrap());
                    let mut new_shader_name = OsString::from(path.file_name().unwrap());
                    new_shader_name.push(".spirv");
                    let new_shader_path = PathBuf::from(out_dir.as_ref()).join(new_shader_name);
                    let mut shader_binary_file = File::create(new_shader_path);
                    shader_binary_file
                        .expect("Cannot create shader file")
                        .write_all(v.as_binary_u8());
                }
                Err(v) => {
                    eprintln!("Shader {}", v);
                    is_there_an_error = true;
                }
            }
        } else {
            is_there_an_error |= compile_recursively(path.as_path(), out_dir.as_ref(), compiler);
        }
    }
    return is_there_an_error;
}
