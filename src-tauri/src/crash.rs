//! 崩溃诊断引擎
//!
//! 设计参考了两位前辈的实现，并按本项目（Tauri + Rust）的情况重写：
//! - HMCL `CrashReportAnalyzer`：正则规则集 + 命名捕获组 + 堆栈包名关键词提取
//! - PCL2 `ModCrash.vb`：多原因收集、分层优先级、堆栈关键词反查 Mod 列表、中文可操作建议
//!
//! 与旧实现的本质差异：
//! - 旧实现是 14 条纯子串 `contains` 规则、**命中第一条即返回**，且没有捕获组，
//!   所以永远只能给出「Mod 不兼容」这类笼统结论，说不出是哪个 Mod。
//! - 新实现先结构化解析崩溃报告（Description / 堆栈 / System Details / Mod 区块），
//!   再用带命名捕获组的正则规则集收集**全部**命中原因并按置信度排序，
//!   最后用堆栈包名关键词反查报告中的 Mod 列表，给出可疑模组的真实文件名。
//!
//! 注意：Rust 的 regex 不支持前后预查（lookaround），PCL 里 `(?<=...)` 的写法
//! 在这里全部改成了等价的捕获组形式。

use regex::Regex;
use serde::Serialize;
use std::sync::OnceLock;

// ---------------------------------------------------------------------------
// 对外数据结构
// ---------------------------------------------------------------------------

/// 单条崩溃原因。一次崩溃可能同时命中多条，全部返回给前端展示。
#[derive(Serialize, Clone, Debug, Default)]
pub struct CrashCause {
    pub id: String,
    /// oom | jvm | gl | java_ver | lwjgl | mod | unknown
    pub severity: String,
    pub title: String,
    pub reason: String,
    pub advice: String,
    /// 命中该原因的证据（崩溃报告原文片段）
    pub evidence: String,
    /// 置信度 0-100，用于排序与前端展示
    pub confidence: u8,
}

/// 崩溃报告「System Details」中的一行环境信息
#[derive(Serialize, Clone, Debug, Default)]
pub struct CrashDetail {
    pub key: String,
    pub value: String,
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct CrashDiagnosis {
    /// 主因（置信度最高）的字段，兼容旧前端
    pub severity: String,
    pub title: String,
    pub reason: String,
    pub advice: String,
    /// 崩溃报告中的 Description 或异常摘要
    pub excerpt: String,
    pub exit_code: Option<i32>,
    pub crash_report: Option<String>,
    /// 定位到的模组（名称 / Mod ID / 文件名）
    pub affected_mods: Vec<String>,
    /// 全部命中的原因，按置信度降序
    pub causes: Vec<CrashCause>,
    /// 关键堆栈帧（已去噪，最多 12 条）
    pub stacktrace: Vec<String>,
    /// 环境信息（Minecraft / Java / 内存 / 显卡 / 系统）
    pub details: Vec<CrashDetail>,
    /// 主因置信度
    pub confidence: u8,
}

// ---------------------------------------------------------------------------
// 规则集
// ---------------------------------------------------------------------------

struct RuleSpec {
    id: &'static str,
    severity: &'static str,
    title: &'static str,
    reason: &'static str,
    advice: &'static str,
    /// 正则，命名捕获组会填充到 title/reason/advice 的 `{name}` 占位符
    pattern: &'static str,
    confidence: u8,
}

/// 规则按置信度降序排列（阅读用，实际排序在运行时做）。
/// 占位符说明：{name} 模组名、{id} Mod ID、{class} 类名、{file} 文件、
/// {java} 需要的 Java 版本、{deps} 缺失依赖列表、{reason} 加载器给出的原因。
const RULE_SPECS: &[RuleSpec] = &[
    // ---------------------------------------------------------------- 调试崩溃
    RuleSpec {
        id: "debug_crash",
        severity: "unknown",
        title: "手动触发的调试崩溃",
        reason: "这是你在游戏中按 F3+C 手动触发的调试崩溃，游戏本身没有问题。",
        advice: "游戏运行正常，无需任何修复。",
        pattern: r"Manually triggered debug crash",
        confidence: 100,
    },
    // ---------------------------------------------------------------- 模组：确定
    RuleSpec {
        id: "mod_crash_forge",
        severity: "mod",
        title: "模组 {name} 导致游戏崩溃",
        reason: "模组 {name}（{id}）在加载/初始化阶段抛出异常，导致游戏终止。",
        advice: "请尝试禁用或更新模组 {name}（{id}）后重新启动游戏。若它需要前置模组，请一并检查前置是否安装齐全。",
        pattern: r"LoaderExceptionModCrash: Caught exception from (?P<name>[^\n(]+?)\s*\((?P<id>[^)]+)\)",
        confidence: 95,
    },
    RuleSpec {
        id: "mod_crash_generic",
        severity: "mod",
        title: "模组 {name} 导致游戏崩溃",
        reason: "游戏在加载模组 {name} 时抛出了未捕获的异常。",
        advice: "请尝试禁用或更新该模组后重新启动游戏；若不确定是哪个，可按最近安装顺序逐个禁用排查。",
        pattern: r"Caught exception from (?P<name>[^\n]+)",
        confidence: 84,
    },
    RuleSpec {
        id: "mod_bootstrap_failed",
        severity: "mod",
        title: "模组 {id} 初始化失败",
        reason: "Forge 在创建模组 {id} 的实例时失败。",
        advice: "请更新模组 {id} 到与当前 Forge / 游戏版本匹配的版本，或先将其移出 mods 目录。",
        pattern: r"Failed to create mod instance\. ModID: (?P<id>[^,\s]+)",
        confidence: 90,
    },
    RuleSpec {
        id: "fabric_entrypoint",
        severity: "mod",
        title: "模组 {id} 的入口点执行失败",
        reason: "Fabric 在执行模组 {id} 提供的入口点（entrypoint）时出错。",
        advice: "请更新或移除模组 {id}，并确认它与当前 Fabric Loader / 游戏版本兼容。",
        pattern: r"Could not execute entrypoint stage '[^']*' due to errors, provided by '(?P<id>[^']+)'!",
        confidence: 92,
    },
    RuleSpec {
        id: "mod_config",
        severity: "mod",
        title: "模组 {id} 的配置文件损坏",
        reason: "模组 {id} 在读取配置文件 {file} 时失败。",
        advice: "请删除配置文件 {file}（游戏会在下次启动时重新生成默认配置），然后重新启动游戏。",
        pattern: r"Failed loading config file (?P<file>\S+) of type \S+ for modid (?P<id>\S+)",
        confidence: 90,
    },
    RuleSpec {
        id: "mod_mixin_apply",
        severity: "mod",
        title: "模组 {id} 注入（Mixin）失败",
        reason: "模组 {id} 的 Mixin 注入失败，通常意味着它与其他模组或当前环境不兼容。",
        advice: "请更新或移除模组 {id}。这类错误在混用优化类模组（OptiFine / Sodium 等）时很常见。",
        pattern: r"Mixin apply for mod (?P<id>\S+) failed",
        confidence: 88,
    },
    RuleSpec {
        id: "mod_mixin_from",
        severity: "mod",
        title: "模组 {id} 注入（Mixin）失败",
        reason: "来自模组 {id} 的 Mixin 在应用时失败。",
        advice: "请更新或移除模组 {id}，并确认它支持当前的游戏与加载器版本。",
        pattern: r"from mod (?P<id>[^./\s]+)\] from",
        confidence: 78,
    },
    RuleSpec {
        id: "mod_mixin_generic",
        severity: "mod",
        title: "模组注入（Mixin）失败",
        reason: "部分模组的 Mixin 注入失败，通常是模组之间或模组与环境不兼容。",
        advice: "请逐个禁用最近安装/更新过的模组来定位问题。优化类模组（OptiFine、Sodium、Performant 等）最常引发此类崩溃。",
        pattern: r"(?i)Mixin (?:prepare|apply) failed |MixinApplyError|MixinTransformerError|mixin\.injection\.throwables\.",
        confidence: 72,
    },
    RuleSpec {
        id: "mixinbootstrap_missing",
        severity: "mod",
        title: "缺少 MixinBootstrap",
        reason: "日志显示缺少 org.spongepowered.asm.launch.MixinTweaker，Mixin 引导未安装。",
        advice: "请安装 MixinBootstrap，或重新安装/更新 Forge 与模组加载器。",
        pattern: r"ClassNotFoundException: org\.spongepowered\.asm\.launch\.MixinTweaker",
        confidence: 86,
    },
    // ---------------------------------------------------------------- 模组：重复 / 依赖
    RuleSpec {
        id: "duplicate_mod_named",
        severity: "mod",
        title: "模组重复安装：{name}",
        reason: "同一个模组被安装了多次。",
        advice: "请在 mods 目录中删除重复的模组文件，确保每个模组只保留一份。",
        pattern: r"Found a duplicate mod (?P<name>[^\n]+)",
        confidence: 88,
    },
    RuleSpec {
        id: "duplicate_mod",
        severity: "mod",
        title: "模组重复安装",
        reason: "检测到重复的模组文件（DuplicateModsFoundException / Found duplicate mods）。",
        advice: "请检查 mods 目录，删除重复的模组文件后重启游戏。注意某些模组可能同时存在 Forge 与 Fabric 两个版本。",
        pattern: r"(?i)DuplicateModsFoundException|Found duplicate mods|ModResolutionException: Duplicate",
        confidence: 84,
    },
    RuleSpec {
        id: "mod_resolution_missing",
        severity: "mod",
        title: "缺少前置模组：{dst}",
        reason: "模组 {src} 依赖 {dst}，但没有找到它。",
        advice: "请安装 {dst} 后再启动游戏。可在「内容中心」搜索该名称，并注意选择与游戏版本匹配的版本。",
        pattern: r"ModResolutionException: Could not find required mod: (?P<src>[^\n]+?) requires (?P<dst>[^\n]+)",
        confidence: 90,
    },
    RuleSpec {
        id: "mod_resolution_conflict",
        severity: "mod",
        title: "模组冲突：{a} 与 {b}",
        reason: "模组 {a} 与 {b} 互相冲突，无法同时加载。",
        advice: "请二选一保留（或更新到互相兼容的版本），然后重新启动游戏。",
        pattern: r"ModResolutionException: Found conflicting mods: (?P<a>[^\n]+?) conflicts with (?P<b>[^\n]+)",
        confidence: 90,
    },
    RuleSpec {
        id: "forge_missing_deps",
        severity: "mod",
        title: "缺少模组的必需依赖",
        reason: "Forge 报告以下依赖不满足：{deps}",
        advice: "请安装上述依赖模组（注意版本需与当前游戏版本一致），然后重新启动游戏。",
        pattern: r"Missing or unsupported mandatory dependencies:(?P<deps>(?:[\r\n]+\t[^\n]*)+)",
        confidence: 90,
    },
    RuleSpec {
        id: "fabric_incompatible",
        severity: "mod",
        title: "Fabric 检测到模组不兼容",
        reason: "Fabric 阻止了游戏启动：{reason}",
        advice: "请根据上面的提示处理冲突的模组（通常是版本不匹配或缺少前置）。",
        pattern: r"Incompatible mods found!(?P<reason>[\s\S]{0,600}?)(?:[\r\n]+\tat )",
        confidence: 86,
    },
    RuleSpec {
        id: "fabric_formatted",
        severity: "mod",
        title: "Fabric 加载器阻止了启动",
        reason: "Fabric Loader 抛出了 FormattedException，通常意味着模组版本不匹配或缺少前置。",
        advice: "请查看下方「关键信息」中 Fabric 给出的提示，安装缺失的前置或更新版本不符的模组。",
        pattern: r"(?i)net\.fabricmc\.loader\.impl\.FormattedException|Some of your mods are incompatible with the game or each other",
        confidence: 76,
    },
    RuleSpec {
        id: "forge_failure_message",
        severity: "mod",
        title: "加载器给出的错误：{reason}",
        reason: "Forge 在加载某个模组文件时失败。",
        advice: "请根据上面的错误信息处理对应模组（多为版本不匹配或文件损坏）。",
        pattern: r"\s*Failure message:\s*(?P<reason>[^\n]+)",
        confidence: 70,
    },
    RuleSpec {
        id: "forge_mod_block",
        severity: "mod",
        title: "模组 {id} 加载失败",
        reason: "Forge 在加载 {id}（{file}）时报错：{reason}",
        advice: "请根据上面的提示处理模组 {id}：通常是缺少它要求的前置模组，或前置版本不够新。安装/更新对应前置后重新启动游戏。",
        pattern: r"-- MOD (?P<id>[^\s]+) --[\s\S]{0,400}?Mod File:\s*(?P<file>[^\n]+)[\s\S]{0,400}?Failure message:\s*(?P<reason>[^\n]+)",
        confidence: 90,
    },
    RuleSpec {
        id: "javaagent_failed",
        severity: "jvm",
        title: "Java Agent（-javaagent）加载失败",
        reason: "JVM 无法加载启动参数中指定的 Java Agent（常见于 log4j 补丁、性能优化类代理）。",
        advice: "请检查实例设置里的 JVM 参数，删除或修正 -javaagent 相关配置（这类代理常随旧版安全补丁添加，如今大多已不需要）。",
        pattern: r"processing of -javaagent failed|Error opening zip file or JAR manifest missing|ClassNotFoundException: [\w.\-]*[Aa]gent",
        confidence: 85,
    },
    // ---------------------------------------------------------------- 模组：环境类
    RuleSpec {
        id: "mod_files_decompressed",
        severity: "mod",
        title: "Mod 文件被解压",
        reason: "mods 目录里存在被解压成文件夹的 Mod，Forge 拒绝继续加载。",
        advice: "请删除 mods 目录中已解压的 Mod 文件夹，确保每个 Mod 都是一个 .jar 文件。注意：不要直接把压缩包「解压到当前文件夹」。",
        pattern: r"(?i)The directories below appear to be extracted jar files|Extracted mod jars found, loading will NOT continue",
        confidence: 92,
    },
    RuleSpec {
        id: "mod_name_invalid",
        severity: "mod",
        title: "Mod 文件名包含非法字符",
        reason: "某个 Mod 的文件名（多为中文名）导致 Forge 无法为它生成合法的模块名。",
        advice: "请把该 Mod 的文件名改成只包含英文字母、数字、减号和下划线，然后重新启动游戏。",
        pattern: r"Invalid module name: '[^']*' is not a Java identifier",
        confidence: 86,
    },
    RuleSpec {
        id: "too_many_mods",
        severity: "mod",
        title: "模组过多，超出 ID 上限",
        reason: "安装的模组数量超出了旧版 Forge 的方块/物品 ID 上限。",
        advice: "请安装 JEID 等 ID 扩展修复模组，或删除部分大型模组。",
        pattern: r"(?i)maximum id range exceeded",
        confidence: 80,
    },
    RuleSpec {
        id: "night_config",
        severity: "mod",
        title: "Night Config 解析失败",
        reason: "Night Config（Forge 的配置库）读取配置文件时数据不足，通常是配置文件损坏。",
        advice: "请删除出错模组对应的配置文件（config 目录下），或安装 Night Config Fixes 模组。",
        pattern: r"com\.electronwill\.nightconfig\.core\.io\.ParsingException: Not enough data available",
        confidence: 86,
    },
    RuleSpec {
        id: "corrupt_jar",
        severity: "mod",
        title: "Jar 文件损坏",
        reason: "某个 Jar（多为 Mod 或游戏本体）读取失败，文件可能下载不完整。",
        advice: "请重新下载对应的文件。若是 Mod，请删除后重新下载并确认下载完整。",
        pattern: r"(?i)java\.util\.zip\.ZipException|Invalid or corrupt jarfile|Caused by: java\.io\.IOException: The file appears corrupted",
        confidence: 72,
    },
    // ---------------------------------------------------------------- Java
    RuleSpec {
        id: "openj9",
        severity: "java_ver",
        title: "使用了不被支持的 OpenJ9 虚拟机",
        reason: "OpenJ9 不是 Minecraft 官方支持的 JVM，很多模组在它上面会崩溃。",
        advice: "请在实例设置的 Java 选项里改用 HotSpot JVM（Oracle JDK 或 OpenJDK），不要使用 OpenJ9。",
        pattern: r"(?i)Open J9 is not supported|OpenJ9 is incompatible|\.J9VMInternals\.",
        confidence: 90,
    },
    RuleSpec {
        id: "too_old_java",
        severity: "java_ver",
        title: "Java 版本过低：需要 Java {java}",
        reason: "有 class 文件是用更高版本的 Java 编译的，当前 Java 无法识别。",
        advice: "请安装 Java {java} 或更高版本，并在实例设置中选择它。可在「设置 → Java」里让启动器自动下载。",
        pattern: r"UnsupportedClassVersionError: [^\n]*? version (?P<major>\d+)\.0",
        confidence: 88,
    },
    RuleSpec {
        id: "need_jdk11",
        severity: "java_ver",
        title: "需要使用 Java 11",
        reason: "某些模组（如 ModernUI、部分 Forge 版本）要求 Java 11 及以上。",
        advice: "请在实例设置中改用 Java 11 或更高版本后重新启动游戏。",
        pattern: r"no such method: sun\.misc\.Unsafe\.defineAnonymousClass|The requested compatibility level JAVA_11 could not be set|has been compiled by a more recent version of the Java Runtime \(class file version 55\.0\)",
        confidence: 84,
    },
    RuleSpec {
        id: "java_too_high",
        severity: "java_ver",
        title: "Java 版本过高",
        reason: "当前 Java 版本过高，旧版游戏/Forge 与之不兼容。",
        advice: "请改用更低版本的 Java（如 Java 8 或 Java 11）运行游戏。旧版 Forge（1.16 及更早）建议 Java 8。",
        pattern: r"Unable to make protected final java\.lang\.Class java\.lang\.ClassLoader\.defineClass|because module java\.base does not export|NoSuchFieldException: ucp|Unsupported class file major version|ClassNotFoundException: jdk\.nashorn\.api\.scripting|ClassNotFoundException: java\.lang\.invoke\.LambdaMetafactory",
        confidence: 82,
    },
    RuleSpec {
        id: "jdk_not_jre",
        severity: "java_ver",
        title: "使用了 JDK 而非 JRE",
        reason: "日志显示使用了 JDK（或 Java 9+ 的模块系统），部分旧版游戏与之不兼容。",
        advice: "请改用 JRE（或旧版 JDK 8）运行游戏，可在实例设置的 Java 选项中切换。",
        pattern: r"ClassCastException: (?:java\.base/jdk|class jdk)",
        confidence: 80,
    },
    RuleSpec {
        id: "modlauncher8",
        severity: "java_ver",
        title: "低版本 Forge 与高版本 Java 不兼容",
        reason: "旧版 Forge（ModLauncher 8）在 Java 8u321+ 上会因签名校验 API 变更而崩溃。",
        advice: "请二选一：升级 Forge 到 36.2.26 或更高版本；或改用低于 1.8.0.321 的 Java 8。",
        pattern: r"NoSuchMethodError: (?:'void )?sun\.security\.util\.ManifestEntryVerifier",
        confidence: 86,
    },
    RuleSpec {
        id: "java_param_invalid",
        severity: "jvm",
        title: "Java 启动参数有误",
        reason: "JVM 无法识别某个启动参数，或内存参数超出允许范围。",
        advice: "请检查实例设置中的 JVM 参数与内存分配：删除无效参数，且内存不要超过物理内存。",
        pattern: r"Unrecognized option:|Unrecognized VM option|Invalid maximum heap size|Too small maximum heap",
        confidence: 78,
    },
    // ---------------------------------------------------------------- 内存
    RuleSpec {
        id: "out_of_memory",
        severity: "oom",
        title: "内存不足",
        reason: "Java 堆内存或系统物理内存耗尽，游戏无法继续分配内存。",
        advice: "请到实例设置里降低分配给游戏的内存（一般 4-6 GB 足够），关闭其他占用内存的程序，并减少高清材质、光影和大型模组。",
        pattern: r"(?i)java\.lang\.OutOfMemoryError|There is insufficient memory for the Java Runtime Environment|The system is out of physical RAM or swap space|Out of Memory Error|Failed to allocate memory|native memory allocation \(mmap\) failed",
        confidence: 86,
    },
    RuleSpec {
        id: "java_32bit",
        severity: "jvm",
        title: "使用了 32 位 Java",
        reason: "32 位 Java 最多只能使用约 1 GB 内存，无法满足 Minecraft 的需求。",
        advice: "请在实例设置中改用 64 位 Java；如果你的系统是 32 位的，则需要重装 64 位系统。",
        pattern: r"Could not reserve enough space for 1048576KB object heap|Could not reserve enough space for [0-9]{6,}KB object heap",
        confidence: 82,
    },
    RuleSpec {
        id: "gl_out_of_memory",
        severity: "gl",
        title: "显存不足（OpenGL 内存耗尽）",
        reason: "显卡显存被耗尽，通常由过高分辨率的材质包或光影导致。",
        advice: "请降低材质包分辨率或关闭光影，并在显卡设置中确认游戏使用的是独立显卡。",
        pattern: r"(?i)GL_OUT_OF_MEMORY|Out of memory: allocated|org\.lwjgl\.opengl\.GLException: Out of memory",
        confidence: 74,
    },
    // ---------------------------------------------------------------- 显卡
    RuleSpec {
        id: "opengl_unsupported",
        severity: "gl",
        title: "显卡驱动不支持 OpenGL",
        reason: "驱动报告不支持 OpenGL，无法创建游戏渲染上下文。",
        advice: "请更新显卡驱动到最新版本（或回退到出厂版本）。若是双显卡机型，请确认游戏使用独立显卡运行。",
        pattern: r"(?i)The driver does not appear to support OpenGL|OpenGL is not supported|GLX: Failed to create context",
        confidence: 82,
    },
    RuleSpec {
        id: "graphics_driver",
        severity: "gl",
        title: "显卡驱动 / 像素格式错误",
        reason: "OpenGL 上下文或像素格式创建失败，多为驱动异常。",
        advice: "请更新显卡驱动；若使用核显（尤其是 Intel HD Graphics），请切换到独立显卡后再启动游戏。",
        pattern: r"(?i)Pixel format not accelerated|Couldn't set pixel format|no matching pixel format|Failed to create GL context|GLFW error before init|net\.minecraftforge\.fml\.client\.SplashProgress|org\.lwjgl\.LWJGLException",
        confidence: 78,
    },
    RuleSpec {
        id: "gl_operation_failure",
        severity: "gl",
        title: "光影 / 材质包导致 OpenGL 错误",
        reason: "出现了 OpenGL 1282 无效操作错误，通常由光影或高清材质触发。",
        advice: "请移除当前使用的光影或材质包，或降低其分辨率后重新启动游戏。",
        pattern: r"1282: Invalid operation|Maybe try a lower ?resolution (?:resourcepack|texturepack)\?",
        confidence: 76,
    },
    // ---------------------------------------------------------------- 加载器 / 本体
    RuleSpec {
        id: "forge_incomplete",
        severity: "mod",
        title: "Forge 安装不完整",
        reason: "找不到 Forge 的启动目标或库文件，Forge 本体可能缺失/损坏。",
        advice: "请重新安装一次同版本的 Forge。注意：打包或迁移实例时不要删除 libraries 目录。",
        pattern: r"Cannot find launch target fmlclient|Invalid paths argument, contained no existing paths|Failed to find Minecraft resource version|Could not find net/minecraft/client/Minecraft\.class in classloader",
        confidence: 86,
    },
    RuleSpec {
        id: "optifine_forge",
        severity: "mod",
        title: "OptiFine 与当前 Forge 版本不兼容",
        reason: "OptiFine 与 Forge 版本不匹配，导致类方法签名对不上。",
        advice: "请前往 OptiFine 官网查看它与 Forge 的版本对应关系，严格按对应版本重新安装。较新的 Forge 通常不再需要 OptiFine，可改用 Sodium/Embeddium 等替代方案。",
        pattern: r"NoSuchMethodError: (?:'void net\.minecraft\.client\.renderer\.texture\.SpriteContents|'java\.lang\.String com\.mojang\.blaze3d\.systems\.RenderSystem\.getBackendDescription|'void net\.minecraftforge\.client\.gui\.overlay\.ForgeGui\.renderSelectedItemName|'net\.minecraft\.network\.chat\.FormattedText net\.minecraft\.client\.gui\.Font\.ellipsize|'void net\.minecraft\.client\.renderer\.block\.model\.BakedQuad)",
        confidence: 88,
    },
    RuleSpec {
        id: "shaders_mod_conflict",
        severity: "mod",
        title: "Shaders Mod 与 OptiFine 冲突",
        reason: "Shaders Mod 与 OptiFine 同时安装，而 OptiFine 已内置光影功能。",
        advice: "请删除 Shaders Mod，只保留 OptiFine。",
        pattern: r"Shaders Mod detected\. Please remove it, OptiFine has built-in support for shaders\.",
        confidence: 90,
    },
    RuleSpec {
        id: "optifine_world",
        severity: "mod",
        title: "OptiFine 导致世界无法加载",
        reason: "该问题只在特定版本的 OptiFine 上出现，表现为进入世界时崩溃。",
        advice: "请更换 OptiFine 的版本（升级或降级），或暂时移除 OptiFine。",
        pattern: r"NoSuchMethodError: net\.minecraft\.world\.server\.ChunkManager\$ProxyTicketManager\.shouldForceTicks",
        confidence: 76,
    },
    RuleSpec {
        id: "fabric_old_loader",
        severity: "mod",
        title: "Fabric Loader 版本过旧",
        reason: "缺少 FabricMixinTransformerProxy，说明 Fabric Loader 与 Mixin 版本不匹配。",
        advice: "请把 Fabric Loader 更新到最新版本。",
        pattern: r"NoClassDefFoundError: org/spongepowered/asm/mixin/transformer/FabricMixinTransformerProxy",
        confidence: 82,
    },
    RuleSpec {
        id: "lwjgl_missing",
        severity: "lwjgl",
        title: "本地库（LWJGL / OpenGL）加载失败",
        reason: "无法加载 LWJGL 等本地库，通常是游戏依赖缺失或 Java 架构不匹配。",
        advice: "请尝试重新安装/切换 Java（注意 64 位），或删除实例后重新下载完整依赖。",
        pattern: r"(?i)UnsatisfiedLinkError: Failed to locate library: (?P<name>[^\n]+)|no lwjgl[\w.]* in java\.library\.path|No class found: org/lwjgl|Could not initialize class org\.lwjgl",
        confidence: 84,
    },
    // ---------------------------------------------------------------- 文件 / 校验
    RuleSpec {
        id: "file_already_exists",
        severity: "mod",
        title: "文件已存在：{file}",
        reason: "某个模组在创建配置文件时发现同名文件/目录已存在。",
        advice: "请按上面的路径删除已存在的那个同名文件或文件夹，然后重新启动游戏。",
        pattern: r"FileAlreadyExistsException: (?P<file>[^\n]+)",
        confidence: 82,
    },
    RuleSpec {
        id: "file_changed",
        severity: "mod",
        title: "文件校验失败",
        reason: "游戏或模组文件被修改过，签名/SHA1 校验不通过。",
        advice: "请勿手动修改游戏 Jar 或模组文件；建议重新下载该实例的游戏文件与模组。",
        pattern: r"(?i)SHA1 digest error for (?P<file>[^\n]+)|signer information does not match signer information of other classes in the same package",
        confidence: 82,
    },
    // ---------------------------------------------------------------- JVM 致命错误
    RuleSpec {
        id: "jvm_fatal_error",
        severity: "jvm",
        title: "Java 虚拟机发生致命错误",
        reason: "JVM 自身崩溃并生成了 hs_err_pid 日志（非 Java 层异常）。",
        advice: "这通常是显卡驱动、内存或 Java 本体的问题。请更新显卡驱动、降低内存分配，或换一个 Java 版本试试。",
        pattern: r"# A fatal error has been detected by the Java Runtime Environment|EXCEPTION_ACCESS_VIOLATION",
        confidence: 60,
    },
    // ---------------------------------------------------------------- 世界内容
    RuleSpec {
        id: "entity_crash",
        severity: "mod",
        title: "实体 {type} 导致崩溃",
        reason: "游戏在渲染/更新实体 {type}（位置 {loc}）时崩溃。",
        advice: "若是特定实体导致，可在世界中移除它；若无法进入世界，可尝试删除该世界或使用备份回档。",
        pattern: r"Entity Type: (?P<type>[^\n]+)[\s\S]{0,800}?Entity's Exact location: (?P<loc>[^\n]+)",
        confidence: 70,
    },
    RuleSpec {
        id: "block_crash",
        severity: "mod",
        title: "方块 {type} 导致崩溃",
        reason: "游戏在渲染/更新方块 {type}（位置 {loc}）时崩溃。",
        advice: "可新建一个世界测试：若新世界正常，则是该方块导致，需要移除它或回档到之前的存档备份。",
        pattern: r"Block: (?P<type>[^\n]+)[\s\S]{0,800}?Block location: (?P<loc>[^\n]+)",
        confidence: 70,
    },
    // ---------------------------------------------------------------- 兜底异常类型
    RuleSpec {
        id: "no_such_method",
        severity: "mod",
        title: "方法缺失：{class}",
        reason: "找不到方法 {class}，通常由模组与游戏/其他模组版本不匹配导致。",
        advice: "请更新相关模组到与当前游戏版本匹配的版本。",
        pattern: r"NoSuchMethodError: (?P<class>[^\n]+)",
        confidence: 58,
    },
    RuleSpec {
        id: "no_class_def",
        severity: "mod",
        title: "类缺失：{class}",
        reason: "运行时找不到类 {class}，通常是缺少前置模组或模组版本不匹配。",
        advice: "请安装缺失的前置模组，或更新相关模组到匹配版本。",
        pattern: r"NoClassDefFoundError: (?P<class>[^\n]+)",
        confidence: 58,
    },
    RuleSpec {
        id: "illegal_access",
        severity: "mod",
        title: "非法访问：{class}",
        reason: "类 {class} 试图访问无权访问的类成员。",
        advice: "请更新相关模组；这类问题常见于旧模组搭配新版本 Java。",
        pattern: r"IllegalAccessError: tried to access class [^\n]+ from class (?P<class>[^\n]+)",
        confidence: 58,
    },
    RuleSpec {
        id: "rtss_sodium",
        severity: "mod",
        title: "RivaTuner Statistics Server 与 Sodium 不兼容",
        reason: "RTSS（常随 MSI Afterburner 安装）的钩子与 Sodium 冲突。",
        advice: "请关闭 RivaTuner Statistics Server / MSI Afterburner 的屏幕显示（OSD）功能，或卸载它们后再启动游戏。也可以改用别的帧率显示方式。",
        pattern: r"RivaTuner Statistics Server \(RTSS\) is not compatible with Sodium",
        confidence: 92,
    },
    RuleSpec {
        id: "stack_overflow",
        severity: "mod",
        title: "栈溢出（StackOverflowError）",
        reason: "出现了无限递归调用，常见于模组之间的互相调用循环。",
        advice: "请逐个禁用最近添加的模组来定位；也可能是某个模组的配置异常导致。",
        pattern: r"StackOverflowError",
        confidence: 62,
    },
];

struct CompiledRule {
    spec: &'static RuleSpec,
    re: Regex,
}

static COMPILED: OnceLock<Vec<CompiledRule>> = OnceLock::new();

fn rules() -> &'static [CompiledRule] {
    COMPILED
        .get_or_init(|| {
            RULE_SPECS
                .iter()
                .map(|spec| CompiledRule {
                    spec,
                    // 正则编译失败不应让整个诊断崩掉：退化成永不匹配的空规则
                    re: Regex::new(spec.pattern).unwrap_or_else(|_| Regex::new(r"(?!x)x").unwrap()),
                })
                .collect()
        })
        .as_slice()
}

/// 把模板里的 `{name}` 替换成同名捕获组的内容。
fn fill(tpl: &str, caps: &regex::Captures) -> String {
    let mut out = String::with_capacity(tpl.len());
    let mut rest = tpl;
    while let Some(start) = rest.find('{') {
        let (before, tail) = rest.split_at(start);
        out.push_str(before);
        match tail.find('}') {
            Some(end) => {
                let key = &tail[1..end];
                let value = match key {
                    // UnsupportedClassVersionError 的 major 版本号 → Java 版本（46 → Java 2 … 52 → Java 8）
                    "java" => caps
                        .name("major")
                        .and_then(|m| m.as_str().parse::<u32>().ok())
                        .map(|v| (v.saturating_sub(44)).to_string())
                        .unwrap_or_default(),
                    _ => caps
                        .name(key)
                        .map(|m| m.as_str().trim().to_string())
                        .unwrap_or_default(),
                };
                // 依赖列表等多行内容压成单行，便于界面展示
                let value = value.split_whitespace().collect::<Vec<_>>().join(" ");
                out.push_str(&value);
                rest = &tail[end + 1..];
            }
            None => {
                out.push_str(tail);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

// ---------------------------------------------------------------------------
// 崩溃报告结构化解析
// ---------------------------------------------------------------------------

const STACK_LIMIT: usize = 12;

/// 堆栈包名关键词黑名单：这些是游戏/Java/加载器自身的包名，不能拿来推断模组。
/// 合并了 HMCL 的 PACKAGE_KEYWORD_BLACK_LIST 与 PCL2 的过滤词表。
const KEYWORD_BLACKLIST: &[&str] = &[
    // Java / 虚拟机
    "java", "javax", "sun", "jdk", "com", "org", "net", "io", "nio", "util", "lang", "reflect",
    "zip", "jar", "runtime", "internal", "invoke", "concurrent", "function", "stream", "regex",
    "math", "text", "time", "security", "awt", "image", "beans", "annotation", "crypto", "netty",
    // Minecraft / Mojang
    "minecraft", "mojang", "net", "client", "server", "world", "entity", "block", "item", "tile",
    "tileentity", "blockentity", "gui", "render", "renderer", "model", "texture", "font", "sound",
    "chunk", "biome", "biomes", "gen", "worldgen", "nbt", "network", "packet", "recipe", "advancement",
    "inventory", "particle", "physics", "pathfinding", "ai", "monster", "passive", "player", "screen",
    "loading", "resources", "shader", "glsl", "pipeline", "color", "storage", "level", "state",
    "datafixerupper", "fastutil", "unimi", "dsi", "oshi", "platform", "universal", "optifine",
    // Forge / Fabric / 加载器
    "fml", "forge", "minecraftforge", "neoforge", "neoforged", "cpw", "modlauncher", "launchwrapper",
    "objectweb", "asm", "event", "eventhandler", "handshake", "modapi", "kcauldron", "fabricmc",
    "fabric", "loader", "game", "knot", "launch", "mixin", "mixins", "spongepowered", "injection",
    "transformer", "transformers", "electronwill", "nightconfig", "mumfrey", "lwjgl", "glfw",
    "stb", "jemalloc", "natives", "library", "libraries", "bootstrap", "kotori", "modernui",
    // 通用词
    "core", "common", "config", "compat", "api", "impl", "lib", "main", "mod", "mods", "helper",
    "util", "utils", "base", "fake", "fakes", "init", "preinit", "preload", "setup", "script",
    "scripts", "handler", "handlers", "event", "events", "listener", "listeners", "manager",
    "registry", "registries", "content", "feature", "modules", "module", "service", "systems",
    "general", "machine", "external", "embedded", "override", "assist", "done", "load", "read",
    "file", "files", "pool", "task", "scheduler", "channel", "network", "plugin", "integration",
    "engine", "top", "dev", "mcp", "srg", "repackage", "github", "gitlab", "microsoft", "google",
    "gson", "guava", "apache", "commons", "logging", "slf4j", "log4j", "oshi", "jna", "jnidispatch",
];

struct Parsed {
    description: String,
    stacktrace: Vec<String>,
    details: Vec<CrashDetail>,
    /// 报告中列出模组信息的行（Forge 的 `Mod File:`、Fabric 的 `\t\tmodid : name`）
    mod_lines: Vec<String>,
    /// 从堆栈中提取的可疑包名关键词
    keywords: Vec<String>,
}

fn parse(text: &str) -> Parsed {
    Parsed {
        description: parse_description(text),
        stacktrace: parse_stacktrace(text),
        details: parse_details(text),
        mod_lines: parse_mod_lines(text),
        keywords: Vec::new(),
    }
}

/// 崩溃报告的第一句人话：优先 `Description:`，否则取第一条异常行。
fn parse_description(text: &str) -> String {
    for line in text.lines() {
        let s = line.trim();
        if let Some(v) = s.strip_prefix("Description:") {
            let v = v.trim().replace('\u{a0}', " ");
            // Fabric 的 Description 常是「Initializing game」这类无信息量的阶段名
            if !v.is_empty() && !is_useless_description(&v) {
                return v;
            }
        }
    }
    // 退回：第一条异常/错误行（形如 `net.fabricmc...FormattedException: ...`）
    for line in text.lines() {
        let s = line.trim();
        if s.is_empty() || s.starts_with("at ") || s.starts_with("--") {
            continue;
        }
        if (s.contains("Exception") || s.contains("Error")) && s.contains('.') && !s.starts_with('#') {
            return s.replace('\u{a0}', " ");
        }
    }
    String::new()
}

fn is_useless_description(v: &str) -> bool {
    let l = v.to_lowercase();
    l.contains("loading library")
        || l.contains("initializing game")
        || l.contains("exception in server tick loop")
        || l.contains("rendering screen")
        || l.contains("updating screen events")
        || l.contains("mouseclicked event handler")
}

/// 取前 N 条 `at ...` 堆栈帧，去掉明显的噪音帧（Java 反射、事件总线等）。
fn parse_stacktrace(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut header_taken = false;
    for line in text.lines() {
        let s = line.trim_end();
        let st = s.trim_start();
        if st.starts_with("at ") {
            if out.len() >= STACK_LIMIT {
                break;
            }
            if !is_noise_frame(st) {
                out.push(st.to_string());
            }
        } else if !header_taken
            && !st.is_empty()
            && (st.contains("Exception") || st.contains("Error"))
            && st.contains('.')
            && !st.starts_with('#')
            && !st.starts_with("--")
        {
            // 堆栈顶部的异常头（含 Caused by）
            out.push(st.to_string());
            header_taken = true;
        }
    }
    out
}

/// 反射、事件总线、线程调度这类帧对定位模组没有帮助
fn is_noise_frame(frame: &str) -> bool {
    frame.contains("sun.reflect.")
        || frame.contains("jdk.internal.reflect.")
        || frame.contains("java.lang.reflect.")
        || frame.contains("com.google.common.eventbus.")
        || frame.contains("java.base/")
        || frame.contains("net.minecraftforge.eventbus.")
        || frame.contains("java.util.concurrent.")
        || frame.contains("java.lang.Thread.run")
}

/// 解析 `-- System Details --` 段落里的环境信息。
fn parse_details(text: &str) -> Vec<CrashDetail> {
    let lines: Vec<&str> = text.lines().collect();
    let start = match lines.iter().position(|l| l.contains("-- System Details --")) {
        Some(i) => i + 1,
        None => return Vec::new(),
    };
    let mut out: Vec<CrashDetail> = Vec::new();
    for line in &lines[start..] {
        let l = line.trim_end();
        if l.trim_start().starts_with("-- ") {
            break; // 进入下一个区块
        }
        // 形如 `\tMinecraft Version: 1.20.1`
        let Some(eq) = l.find(':') else { continue };
        let key = l[..eq].trim().trim_start_matches('\t').trim();
        let value = l[eq + 1..].trim();
        if key.is_empty()
            || value.is_empty()
            || key.len() > 40
            || key.contains("Mods")
            || key.contains("Mod List")
            || !key.chars().all(|c| c.is_ascii_alphanumeric() || c == ' ' || c == '-' || c == '/' || c == '(' || c == ')')
        {
            continue;
        }
        if out.iter().any(|d| d.key == key) {
            continue;
        }
        out.push(CrashDetail {
            key: key.to_string(),
            value: value.to_string(),
        });
        if out.len() >= 14 {
            break;
        }
    }

    // 关键项排到前面，方便一眼看到
    let priority = [
        "Minecraft Version",
        "Minecraft Version ID",
        "Operating System",
        "Java Version",
        "Java VM Version",
        "Memory",
        "Graphics Card",
        "Processor",
        "Loaded Shaderpack",
        "Current Language",
        "Backend API",
        "Window System",
    ];
    out.sort_by_key(|d| {
        priority
            .iter()
            .position(|p| *p == d.key)
            .unwrap_or(priority.len())
    });

    // hs_err 日志补充：问题帧与最大堆内存
    out
}

/// 收集报告里所有列出模组信息的行，供堆栈关键词反查。
fn parse_mod_lines(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut in_mod_section = false;
    for line in text.lines() {
        let l = line.trim_end();
        // Forge 错误块：`Mod File: /path/to/xxx.jar`
        if l.contains("Mod File:") {
            out.push(l.trim_start().to_string());
        }
        // `-- MOD xxx --` 标题
        if let Some(rest) = l.trim_start().strip_prefix("-- MOD ") {
            if let Some(name) = rest.strip_suffix(" --") {
                out.push(name.trim().to_string());
            }
        }
        // Fabric Mods 区块：`\t\tmodid : Mod Name version`
        if l.contains("Fabric Mods:") || l.contains("Mod List:") {
            in_mod_section = true;
            continue;
        }
        if in_mod_section {
            if l.trim_start().starts_with("-- ") || (l.starts_with('\t') == false && !l.trim().is_empty()) {
                // 缩进结束或进入新区块
                if !l.trim().is_empty() && !l.starts_with('\t') {
                    in_mod_section = false;
                    continue;
                }
            }
            let t = l.trim_start();
            if t.starts_with('\t') && t.contains(':') {
                out.push(t.trim_start().to_string());
            }
        }
    }
    out
}

/// 从堆栈帧提取可疑包名关键词（HMCL `findKeywordsFromCrashReport` 的做法）。
fn keywords_from_stacktrace(frames: &[String]) -> Vec<String> {
    let mut words: Vec<String> = Vec::new();
    for frame in frames {
        // at a.b.c.D$E$F.method(Source.java:12)
        let method_part = frame
            .trim_start()
            .strip_prefix("at ")
            .unwrap_or(frame)
            .split('(')
            .next()
            .unwrap_or("");
        let method_part = method_part.replace('$', ".");
        // 去掉末尾的方法名，只留包名
        let segments: Vec<&str> = method_part.split('.').collect();
        let pkg = &segments[..segments.len().saturating_sub(1)];
        for seg in pkg.iter().take(4) {
            let w = seg.trim();
            if w.len() <= 2 || w.starts_with("func_") || w.starts_with("m_") || w.starts_with("field_") {
                continue;
            }
            if KEYWORD_BLACKLIST.contains(&w.to_lowercase().as_str()) {
                continue;
            }
            if w.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false) {
                continue;
            }
            if !words.iter().any(|x| x == w) {
                words.push(w.to_string());
            }
        }
        // Forge 模块信息：`{xf:fml:...}` / `{re:classloading}`
        if let (Some(s), Some(e)) = (frame.find('{'), frame.find('}')) {
            if s < e {
                for token in frame[s + 1..e].split(',') {
                    let t = token.trim();
                    if let Some(v) = t.strip_prefix("xf:") {
                        if !KEYWORD_BLACKLIST.contains(&v.to_lowercase().as_str()) && !words.iter().any(|x| x == v) {
                            words.push(v.to_string());
                        }
                    }
                }
            }
        }
    }
    // PCL 的经验：关键词过多说明匹配跑偏了，宁可不猜
    if words.len() > 10 {
        return Vec::new();
    }
    words
}

/// 用堆栈关键词反查报告中的模组列表，返回（模组名/文件名）。
fn attribute_mods(keywords: &[String], mod_lines: &[String]) -> Vec<String> {
    let mut hits: Vec<String> = Vec::new();
    for kw in keywords {
        let kw = kw.to_lowercase().replace('_', "");
        if kw.len() < 3 {
            continue;
        }
        for line in mod_lines {
            let l = line.to_lowercase().replace('_', "");
            if !l.contains(&kw) {
                continue;
            }
            if l.contains("minecraft.jar") || l.contains(" forge-") || l.contains(" mixin-") || l.contains("fabricloader") {
                continue;
            }
            if let Some(name) = extract_mod_name(line) {
                if !hits.iter().any(|h| h == &name) {
                    hits.push(name);
                }
            }
            break; // 一个关键词只取第一个命中的模组行（PCL 的做法）
        }
    }
    hits
}

/// 从一行模组信息里取出可读名称。
fn extract_mod_name(line: &str) -> Option<String> {
    // Forge：`Mod File: /path/to/sodium-1.20-0.5.3.jar` 或 `Mod File: (sodium-1.20-0.5.3.jar)`
    if line.contains("Mod File:") {
        let after = line.split_once("Mod File:")?.1.trim();
        let name = after
            .split(['/', '\\'])
            .last()
            .unwrap_or(after)
            .trim_end_matches([')', ']', ','])
            .trim()
            .to_string();
        if !name.is_empty() {
            return Some(name);
        }
    }
    // Fabric：`\t\tsodium : Sodium 0.5.3` → 取冒号后、去掉末尾版本号
    if let Some((_, after)) = line.split_once(':') {
        let after = after.trim();
        let mut parts: Vec<&str> = after.split_whitespace().collect();
        if parts.len() > 1 {
            parts.pop(); // 末尾一般是版本号
        }
        let name = parts.join(" ").trim().to_string();
        if !name.is_empty() && name != "-" {
            return Some(name);
        }
    }
    // `-- MOD name --`
    let trimmed = line.trim();
    if !trimmed.is_empty() && !trimmed.contains(':') {
        return Some(trimmed.to_string());
    }
    None
}

// ---------------------------------------------------------------------------
// 主入口
// ---------------------------------------------------------------------------

/// 对一段崩溃文本（崩溃报告 / hs_err / 日志拼合）做诊断。
pub fn analyze_text(text: &str, exit_code: Option<i32>) -> CrashDiagnosis {
    let mut parsed = parse(text);
    parsed.keywords = keywords_from_stacktrace(&parsed.stacktrace);

    let mut causes: Vec<CrashCause> = Vec::new();
    for rule in rules() {
        if let Some(caps) = rule.re.captures(text) {
            let m = caps.get(0).map(|m| m.as_str()).unwrap_or("");
            let evidence: String = m.chars().take(240).collect();
            causes.push(CrashCause {
                id: rule.spec.id.to_string(),
                severity: rule.spec.severity.to_string(),
                title: fill(rule.spec.title, &caps),
                reason: fill(rule.spec.reason, &caps),
                advice: fill(rule.spec.advice, &caps),
                evidence,
                confidence: rule.spec.confidence,
            });
        }
    }

    // 模组归因：先用规则捕获组里点名的模组，再用堆栈关键词反查
    let mut affected: Vec<String> = Vec::new();
    for c in &causes {
        for name in named_mods(text, c.id.as_str()) {
            if !affected.iter().any(|a| a == &name) {
                affected.push(name);
            }
        }
    }
    if affected.is_empty() {
        affected = attribute_mods(&parsed.keywords, &parsed.mod_lines);
    }
    // 去重：同一模组可能同时以「名称」「Mod ID」「名称 (id)」三种形式出现，
    // 保留信息量最大的那条（若 A 是 B 的子串，则 A 冗余）。
    let redundant: Vec<String> = affected
        .iter()
        .filter(|a| affected.iter().any(|b| b != *a && b.contains(a.as_str())))
        .cloned()
        .collect();
    affected.retain(|a| !redundant.contains(a));

    // 已经得到高置信结论时，不再叠加「猜测类」原因，避免给用户制造噪音
    // （PCL 的分层思路：精准匹配命中后就不再做堆栈猜测）
    let has_solid_answer = causes.iter().any(|c| c.confidence >= 70);
    if !has_solid_answer {
        if !affected.is_empty() {
            let list = affected.join("、");
            causes.push(CrashCause {
                id: "suspect_mod".into(),
                severity: "mod".into(),
                title: "怀疑是这些模组导致的".into(),
                reason: format!("堆栈信息中出现了与 {} 相关的调用，它可能就是崩溃的元凶。", list),
                advice: format!(
                    "请尝试依次禁用 {}，每禁用一个就启动一次游戏观察是否恢复，以此定位问题模组。",
                    list
                ),
                evidence: parsed.keywords.join(", "),
                confidence: 50,
            });
        } else if !parsed.keywords.is_empty() {
            let list = parsed.keywords.join("、");
            causes.push(CrashCause {
                id: "suspect_keyword".into(),
                severity: "mod".into(),
                title: "未能定位模组，但发现可疑关键词".into(),
                reason: format!("堆栈中出现了关键词 {}，它们通常来自某个模组。", list),
                advice: "如果你知道这些关键词对应哪个模组，可以尝试禁用它；也可以查看下方堆栈与原始报告获取更多线索。".into(),
                evidence: list,
                confidence: 25,
            });
        }
    }

    // 同一条规则可能被重复命中（正则默认只取第一个，这里按 id 去重）
    let mut uniq: Vec<CrashCause> = Vec::new();
    for c in causes {
        if !uniq.iter().any(|u| u.id == c.id) {
            uniq.push(c);
        }
    }
    uniq.sort_by(|a, b| b.confidence.cmp(&a.confidence));
    uniq.truncate(6);

    // 主因 = 置信度最高的一条；没有命中任何规则则给出「未知」结论
    let mut diag = CrashDiagnosis {
        exit_code,
        excerpt: parsed.description.clone(),
        causes: uniq.clone(),
        stacktrace: parsed.stacktrace.clone(),
        details: parsed.details.clone(),
        affected_mods: affected.clone(),
        ..Default::default()
    };

    if let Some(best) = uniq.first() {
        diag.severity = best.severity.clone();
        diag.title = best.title.clone();
        diag.reason = best.reason.clone();
        diag.advice = best.advice.clone();
        diag.confidence = best.confidence;
    } else {
        diag.severity = "unknown".into();
        diag.title = "未能定位崩溃原因".into();
        diag.reason = "内置规则库没有匹配到已知问题，这可能是个例或较新的崩溃。".into();
        diag.advice = "建议按以下顺序排查：① 更新显卡驱动与 Java；② 降低内存分配与材质/光影规格；③ 逐个禁用最近安装的模组。若仍无法解决，请把下面的堆栈和原始报告发给社区求助。".into();
        diag.confidence = 0;
    }

    // hs_err 补充信息：问题帧与厂商驱动
    for extra in hs_err_details(text) {
        if !diag.details.iter().any(|d| d.key == extra.key) {
            diag.details.push(extra);
        }
    }

    diag
}

/// 从规则捕获组里取出「点名」的模组（Mod ID / 模组名 / 文件名）。
fn named_mods(text: &str, rule_id: &str) -> Vec<String> {
    let patterns: &[&str] = match rule_id {
        "mod_crash_forge" => &[
            r"Caught exception from (?P<v>[^\n(]+?)\s*\((?P<i>[^)]+)\)",
        ],
        "mod_crash_generic" => &[r"Caught exception from (?P<v>[^\n]+)"],
        "mod_bootstrap_failed" => &[r"Failed to create mod instance\. ModID: (?P<v>[^,\s]+)"],
        "fabric_entrypoint" => &[r"provided by '(?P<v>[^']+)'!"],
        "mod_config" => &[r"for modid (?P<v>\S+)"],
        "mod_mixin_apply" => &[r"Mixin apply for mod (?P<v>\S+) failed"],
        "mod_mixin_from" => &[r"from mod (?P<v>[^./\s]+)\] from"],
        "forge_mod_block" => &[r"-- MOD (?P<v>[^\s]+) --"],
        "file_already_exists" | "file_changed" | "lwjgl_missing" => &[],
        _ => &[],
    };
    let mut out: Vec<String> = Vec::new();
    for p in patterns {
        if let Ok(re) = Regex::new(p) {
            for caps in re.captures_iter(text) {
                if let Some(v) = caps.name("v") {
                    let v = v.as_str().trim().trim_end_matches('.').to_string();
                    if !v.is_empty() && !out.contains(&v) {
                        out.push(v);
                    }
                }
                if let Some(i) = caps.name("i") {
                    let i = i.as_str().trim().to_string();
                    if !i.is_empty() && !out.contains(&i) {
                        out.push(i);
                    }
                }
            }
        }
    }
    out
}

/// hs_err 日志里的关键补充：问题帧、驱动厂商、最大堆内存。
fn hs_err_details(text: &str) -> Vec<CrashDetail> {
    let mut out = Vec::new();
    if !text.contains("hs_err_pid") && !text.contains("# A fatal error has been detected") {
        return out;
    }
    for line in text.lines() {
        let l = line.trim();
        if let Some(v) = l.strip_prefix("Problematic frame:") {
            out.push(CrashDetail {
                key: "问题帧".into(),
                value: v.trim().to_string(),
            });
        }
        if l.starts_with("# C  [") || l.starts_with("# C [") {
            let v = l.trim_start_matches("# C  [")
                .trim_start_matches("# C [")
                .to_string();
            let vendor = if v.starts_with("ig") {
                "Intel 显卡驱动"
            } else if v.starts_with("atio") {
                "AMD 显卡驱动"
            } else if v.starts_with("nvoglv") {
                "NVIDIA 显卡驱动"
            } else {
                ""
            };
            if !vendor.is_empty() {
                out.push(CrashDetail {
                    key: "崩溃位置".into(),
                    value: format!("{}（{}）", vendor, v),
                });
            }
        }
        if l.contains("-Xmx") {
            if let Some(pos) = l.find("-Xmx") {
                let rest: String = l[pos..].chars().take_while(|c| !c.is_whitespace()).collect();
                out.push(CrashDetail {
                    key: "最大内存参数".into(),
                    value: rest,
                });
            }
        }
    }
    out.truncate(4);
    out
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// 所有规则的正则都必须能编译（防止手滑写坏模式后静默失效）
    #[test]
    fn all_rules_compile() {
        for spec in RULE_SPECS {
            Regex::new(spec.pattern).unwrap_or_else(|e| panic!("规则 {} 的正则非法: {}", spec.id, e));
        }
    }

    /// 模板中的占位符都应有对应的捕获组，否则界面上会显示半截句子
    #[test]
    fn placeholders_have_capture_groups() {
        for spec in RULE_SPECS {
            let re = Regex::new(spec.pattern).unwrap();
            for tpl in [spec.title, spec.reason, spec.advice] {
                let mut rest = tpl;
                while let Some(s) = rest.find('{') {
                    let tail = &rest[s..];
                    let end = tail.find('}').expect("占位符未闭合");
                    let key = &tail[1..end];
                    assert!(
                        re.capture_names().any(|n| n == Some(key)) || key == "java",
                        "规则 {} 的占位符 {{{}}} 没有对应的捕获组",
                        spec.id,
                        key
                    );
                    rest = &tail[end + 1..];
                }
            }
        }
    }

    const FORGE_MOD_CRASH: &str = r#"---- Minecraft Crash Report ----
Time: 1/6/19 2:12 AM
Description: There was a severe problem during mod loading that has caused the game to fail

net.minecraftforge.fml.common.LoaderExceptionModCrash: Caught exception from Better PvP (xaerobetterpvp)
	at sun.nio.fs.WindowsFileCopy.move(Unknown Source)
	at xaero.pvp.BetterPVP.preInit(BetterPVP.java:105)
	at net.minecraftforge.fml.common.FMLModContainer.handleModStateEvent(FMLModContainer.java:624)
	at net.minecraft.client.Minecraft.func_71384_a(Minecraft.java:466)

-- System Details --
Details:
	Minecraft Version: 1.12.2
	Operating System: Windows 10 (amd64) version 10.0
	Java Version: 1.8.0_201, Oracle Corporation
	Memory: 512 MB / 2048 MB
"#;

    #[test]
    fn detects_forge_mod_crash_with_name() {
        let d = analyze_text(FORGE_MOD_CRASH, Some(1));
        assert_eq!(d.severity, "mod");
        assert!(d.title.contains("Better PvP"), "标题应点名模组，实际：{}", d.title);
        assert!(
            d.affected_mods.iter().any(|m| m.contains("Better PvP") || m == "xaerobetterpvp"),
            "应定位到模组，实际：{:?}",
            d.affected_mods
        );
        // 环境信息应被解析出来
        assert!(d.details.iter().any(|x| x.key == "Minecraft Version" && x.value == "1.12.2"));
        assert!(!d.stacktrace.is_empty());
    }

    #[test]
    fn detects_manual_debug_crash() {
        let log = "---- Minecraft Crash Report ----\nDescription: Manually triggered debug crash\n";
        let d = analyze_text(log, None);
        assert_eq!(d.causes.first().map(|c| c.id.as_str()), Some("debug_crash"));
        assert!(d.reason.contains("F3+C"));
    }

    #[test]
    fn detects_out_of_memory() {
        let log = "Exception in thread main java.lang.OutOfMemoryError: Java heap space\n\tat net.minecraft.client.Minecraft.func_71384_a(Minecraft.java:466)";
        let d = analyze_text(log, None);
        assert_eq!(d.severity, "oom");
        assert!(d.title.contains("内存"));
    }

    #[test]
    fn detects_fabric_missing_dependency() {
        let log = r#"net.fabricmc.loader.impl.FormattedException: ModResolutionException: Could not find required mod: sodium requires modmenu
	at net.fabricmc.loader.impl.FabricLoaderImpl.load(FabricLoaderImpl.java:195)"#;
        let d = analyze_text(log, None);
        assert!(
            d.causes.iter().any(|c| c.id == "mod_resolution_missing"),
            "应识别缺失前置，实际：{:?}",
            d.causes.iter().map(|c| c.id.clone()).collect::<Vec<_>>()
        );
        assert!(d.advice.contains("modmenu"), "建议里应出现缺失的模组名：{}", d.advice);
    }

    #[test]
    fn java_version_is_computed_from_major() {
        // class file version 61.0 → Java 17
        let log = "java.lang.UnsupportedClassVersionError: xyz has been compiled by a more recent version of the Java Runtime (class file version 61.0)";
        let d = analyze_text(log, None);
        let c = d
            .causes
            .iter()
            .find(|c| c.id == "too_old_java")
            .expect("应命中 too_old_java");
        assert!(c.title.contains("Java 17"), "实际标题：{}", c.title);
    }

    #[test]
    fn unknown_crash_falls_back_with_guidance() {
        let log = "游戏在启动时发生了一个从未见过的错误 xyzzy42";
        let d = analyze_text(log, None);
        assert_eq!(d.severity, "unknown");
        assert_eq!(d.confidence, 0);
        assert!(d.advice.contains("排查"));
    }

    /// Forge 的「-- MOD xxx --」错误块：应点名模组并给出它要求的前置
    #[test]
    fn detects_forge_mod_error_block() {
        let log = r#"---- Minecraft Crash Report ----
Description: Mod loading error has occurred

java.lang.Exception: Mod Loading has failed

-- MOD iceandfire --
Details:
        Mod File: iceandfire-2.1.9-1.16.5.jar
        Failure message: Mod iceandfire requires citadel 1.8.1 or above
                Currently, citadel is not installed
        Mod Version: 2.1.9-1.16.5
"#;
        let d = analyze_text(log, None);
        assert_eq!(d.severity, "mod");
        assert!(
            d.title.contains("iceandfire"),
            "应点名模组，实际：{}",
            d.title
        );
        assert!(d.reason.contains("citadel 1.8.1"), "应给出前置要求：{}", d.reason);
        assert!(d.affected_mods.iter().any(|m| m == "iceandfire"));
    }

    #[test]
    fn detects_javaagent_failure() {
        let log = "Exception in thread \"main\" java.lang.ClassNotFoundException: org.glavo.log4j.patch.agent.Log4jAgent\nFATAL ERROR in native method: processing of -javaagent failed";
        let d = analyze_text(log, None);
        assert_eq!(d.severity, "jvm");
        assert!(d.title.contains("javaagent"), "实际标题：{}", d.title);
    }

    /// 已有高置信结论时不应再叠加 25% 的「可疑关键词」噪音
    #[test]
    fn no_speculative_cause_when_solid_answer_exists() {
        let log = "Description: Manually triggered debug crash\n\tat xaero.pvp.BetterPVP.preInit(BetterPVP.java:105)";
        let d = analyze_text(log, None);
        assert!(
            !d.causes.iter().any(|c| c.id == "suspect_keyword"),
            "调试崩溃不该再猜测模组：{:?}",
            d.causes.iter().map(|c| c.id.clone()).collect::<Vec<_>>()
        );
    }

    /// 同一模组的不同写法（名称 / Mod ID / 名称 (id)）只保留信息量最大的一条
    #[test]
    fn affected_mods_are_deduplicated() {
        let d = analyze_text(FORGE_MOD_CRASH, Some(1));
        assert!(
            !d.affected_mods.iter().any(|m| m == "xaerobetterpvp")
                && !d.affected_mods.iter().any(|m| m == "Better PvP"),
            "冗余项应被去掉，实际：{:?}",
            d.affected_mods
        );
        assert_eq!(d.affected_mods.len(), 1);
    }

    #[test]
    fn stacktrace_keywords_are_filtered() {
        let frames = vec![
            "at xaero.pvp.BetterPVP.preInit(BetterPVP.java:105)".to_string(),
            "at sun.nio.fs.WindowsFileCopy.move(Unknown Source)".to_string(),
        ];
        let kw = keywords_from_stacktrace(&frames);
        assert!(kw.contains(&"xaero".to_string()), "实际关键词：{:?}", kw);
        assert!(!kw.iter().any(|k| k == "sun" || k == "nio" || k == "java"));
    }
}
