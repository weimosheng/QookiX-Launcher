fn main() {
    // 编译 vendored cubiomes（C 库）+ 自写 C 胶水层。
    // cubiomes 来源：https://github.com/Cubitect/cubiomes （vendored 拷贝，见 vendor/cubiomes/LICENSE）。
    // 注意：务必显式开启优化。默认 debug profile 下 cc 会编译成 -O0 / Od，
    // 种子地图的瓦片生成会慢 5~10 倍（dev 下尤其明显）。
    let cubiomes_dir = std::path::Path::new("vendor/cubiomes");
    cc::Build::new()
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
        .warnings(false)
        .flag("/source-charset:utf-8")
        .compile("qookix-cubiomes");

    println!("cargo:rerun-if-changed=src/cubiomes_bridge.c");
    println!("cargo:rerun-if-changed=vendor/cubiomes");

    tauri_build::build()
}
