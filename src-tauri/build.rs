fn main() {
    // 编译 vendored cubiomes（C 库）+ 自写 C 胶水层。
    // cubiomes 来源：https://github.com/Cubitect/cubiomes （vendored 拷贝，见 vendor/cubiomes/LICENSE）。
    // 注意：务必显式开启优化。默认 debug profile 下 cc 会编译成 -O0 / Od，
    // 种子地图的瓦片生成会慢 5~10 倍（dev 下尤其明显）。
    let cubiomes_dir = std::path::Path::new("vendor/cubiomes");
    let mut cubiomes = cc::Build::new();
    cubiomes
        .include(cubiomes_dir)
        .include(cubiomes_dir.join("tables"))
        .file("src/cubiomes_bridge.c")
        .files([
            "vendor/cubiomes/biomenoise.c",
            "vendor/cubiomes/biomes.c",
            "vendor/cubiomes/finders.c",
            "vendor/cubiomes/generator.c",
            "vendor/cubiomes/layers.c",
            "vendor/cubiomes/noise.c",
            "vendor/cubiomes/quadbase.c",
            "vendor/cubiomes/util.c",
        ])
        .opt_level(3)
        .warnings(false);

    // 源文件（含中文注释）是 UTF-8。MSVC 默认按系统代码页解析，需要用
    // /source-charset:utf-8 显式指定；这是 MSVC 专有选项，GCC/Clang 会把它当成
    // 输入文件路径，报 "linker input file not found: /source-charset:utf-8"。
    // 非 MSVC 编译器默认就按 UTF-8 解析源码，无需该选项。
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        cubiomes.flag("/source-charset:utf-8");
    }

    cubiomes.compile("qookix-cubiomes");

    println!("cargo:rerun-if-changed=src/cubiomes_bridge.c");
    println!("cargo:rerun-if-changed=vendor/cubiomes");

    tauri_build::build()
}
