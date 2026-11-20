fn windows_resources(original_name: &str) {
    use std::{env,fs,path::PathBuf,process::Command};
    if env::var("CARGO_CFG_TARGET_OS").as_deref()!=Ok("windows"){return;}
    let root=PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let png_path=root.join("crates/metaxtract-gui/assets/icon.png");
    let png=fs::read(&png_path).expect("desktop icon");
    let icon=PathBuf::from(env::var_os("OUT_DIR").expect("resource output")).join("desktop.ico");
    let mut ico=vec![0,0,1,0,1,0,128,128,0,0,1,0,32,0];
    ico.extend_from_slice(&(png.len() as u32).to_le_bytes());
    ico.extend_from_slice(&22u32.to_le_bytes());
    ico.extend_from_slice(&png);
    fs::write(&icon,ico).expect("write ICO container");
    println!("cargo:rerun-if-changed={}",png_path.display());
    println!("cargo:rerun-if-changed=../../native/windows_resources.rs");
    let out=PathBuf::from(env::var_os("OUT_DIR").expect("resource output"));
    let version=env::var("CARGO_PKG_VERSION").expect("package version");
    let numbers:Vec<_>=version.split('.').take(3).map(|v|v.parse::<u16>().expect("numeric version")).collect();
    let numeric=format!("{},{},{},0",numbers[0],numbers[1],numbers[2]);
    let quote=|s:&str|s.replace('\\',"\\\\").replace('"',"\\\"");
    let source=out.join("desktop.rc");
    fs::write(&source,format!(r#"#pragma code_page(65001)
1 ICON "{}"
1 VERSIONINFO
 FILEVERSION {numeric}
 PRODUCTVERSION {numeric}
 FILEFLAGSMASK 0x3fL
 FILEFLAGS 0
 FILEOS 0x40004L
 FILETYPE 1
 FILESUBTYPE 0
BEGIN
 BLOCK "StringFileInfo"
 BEGIN
  BLOCK "040904B0"
  BEGIN
   VALUE "CompanyName", "MetaXtract contributors\0"
   VALUE "FileDescription", "MetaXtract metadata analysis\0"
   VALUE "FileVersion", "{version}\0"
   VALUE "InternalName", "MetaXtract\0"
   VALUE "OriginalFilename", "{original_name}\0"
   VALUE "ProductName", "MetaXtract\0"
   VALUE "ProductVersion", "{version}\0"
  END
 END
 BLOCK "VarFileInfo"
 BEGIN
  VALUE "Translation", 0x0409, 1200
 END
END
"#,quote(&icon.to_string_lossy()))).expect("write desktop resources");
    if env::var("CARGO_CFG_TARGET_ENV").as_deref()==Ok("msvc") {
        let target=out.join("desktop.res");
        let compiler=env::var_os("RC").map(PathBuf::from).unwrap_or_else(||{
            let sdk=env::var_os("WindowsSdkDir").map(PathBuf::from).unwrap_or_else(||PathBuf::from(env::var_os("ProgramFiles(x86)").unwrap_or_else(||"C:\\Program Files (x86)".into())).join("Windows Kits/10"));
            let mut versions:Vec<_>=fs::read_dir(sdk.join("bin")).expect("Windows SDK bin directory").filter_map(Result::ok).map(|e|e.path()).filter(|p|p.is_dir()).collect();
            versions.sort_by_key(|p|p.file_name().unwrap_or_default().to_string_lossy().split('.').filter_map(|s|s.parse::<u32>().ok()).collect::<Vec<_>>());
            versions.into_iter().rev().map(|p|p.join("x64/rc.exe")).find(|p|p.is_file()).expect("Windows SDK resource compiler")
        });
        let status=Command::new(compiler).arg("/nologo").arg("/c65001").arg("/fo").arg(&target).arg(&source).status().expect("run resource compiler");
        assert!(status.success(),"Windows resource compilation failed");println!("cargo:rustc-link-arg={}",target.display());
    } else {
        let target=out.join("desktop.o");let status=Command::new("windres").arg(&source).arg(&target).status().expect("run windres");assert!(status.success(),"Windows resource compilation failed");println!("cargo:rustc-link-arg={}",target.display());
    }
}
