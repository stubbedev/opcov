use opcov_core::cfg::PhpVersion;
use std::{
    env,
    ffi::OsString,
    fmt, fs, io,
    path::{Path, PathBuf},
    process::{Command, ExitStatus},
};

const PHP_CONFIG_ENV: &str = "PHP_CONFIG";
const ALLOW_UNRELEASED_ENV: &str = "OPCOV_ALLOW_UNRELEASED_PHP";
const DEFAULT_PHP_CONFIG: &str = "php-config";
const SHIM_DIR: &str = "shim";
const SHIM_SOURCE: &str = "shim/shim.c";
const WRAPPER_HEADER: &str = "shim/wrapper.h";

const ENGINE_ITEMS: &[&str] = &[
    "opcov_.*",
    "display_ini_entries",
    "php_info_print_table_end",
    "php_info_print_table_row",
    "php_info_print_table_start",
    "php_pcre_pce_decref",
    "php_pcre_pce_incref",
    "spl_ce_LogicException",
    "spl_ce_RuntimeException",
    "zend_ce_type_error",
    "zend_ce_value_error",
    "zend_build_cfg",
    "zend_cfg_build_predecessors",
    "zend_cfg_compute_dominators_tree",
    "zend_cfg_identify_loops",
    "zend_compile_file",
    "zend_compile_string",
    "zend_get_extension",
    "zend_get_resource_handle",
    "zend_get_user_opcode_handler",
    "zend_register_extension",
    "zend_register_ini_entries_ex",
    "zend_resolve_path",
    "zend_set_user_opcode_handler",
    "zend_throw_exception_hook",
    "zend_object",
    "zend_unregister_ini_entries_ex",
    "zend_vm_kind",
    "zend_vm_set_opcode_handler",
    "IS_CONST",
    "IS_CV",
    "IS_TMP_VAR",
    "IS_UNUSED",
    "IS_VAR",
    "IS_SMART_BRANCH_JMPZ",
    "IS_SMART_BRANCH_JMPNZ",
    "MAY_BE_ARRAY",
    "MAY_BE_NULL",
    "MAY_BE_STRING",
    "MAY_BE_VOID",
    "PHP_API_VERSION",
    "PHP_VERSION",
    "PHP_VERSION_ID",
    "ZEND_ACC_ANON_CLASS",
    "ZEND_ACC_CLOSURE",
    "ZEND_ACC_DONE_PASS_TWO",
    "ZEND_BB_CATCH",
    "ZEND_BB_FINALLY",
    "ZEND_BB_FINALLY_END",
    "ZEND_BB_LOOP_HEADER",
    "ZEND_BB_REACHABLE",
    "ZEND_BB_RECV_ENTRY",
    "ZEND_BB_START",
    "ZEND_COMPILE_NO_JUMPTABLES",
    "ZEND_DEBUG",
    "ZEND_INI_PERDIR",
    "ZEND_INI_SYSTEM",
    "ZEND_MODULE_API_NO",
    "ZEND_NOP",
    "ZEND_PROPERTY_HOOK_COUNT",
    "ZEND_USER_OPCODE",
    "ZEND_USER_OPCODE_CONTINUE",
    "ZEND_USER_OPCODE_DISPATCH",
    "ZEND_VM_KIND_.*",
    "MAXPATHLEN",
    "zend_basic_block",
    "zend_cfg",
    "zend_execute_data",
    "zend_extension",
    "zend_file_handle",
    "zend_function",
    "zend_function_entry",
    "zend_ini_entry_def",
    "zend_internal_arg_info",
    "zend_module_entry",
    "zend_op",
    "zend_op_array",
    "zend_result",
    "zval",
];

const OPAQUE_LIBC_TYPES: &str = "FILE|_IO_.*|__sFILE.*|sigaction|sigjmp_buf|jmp_buf|__jmp_buf.*";
const OPAQUE_LIBC_INNER_TYPES: &str = "sigaction__bindgen_ty_.*";

const OPCODE_HANDLER_TYPE: &str = "zend_vm_opcode_handler_t";

#[derive(Debug)]
struct MacroTypes;

impl bindgen::callbacks::ParseCallbacks for MacroTypes {
    fn int_macro(&self, name: &str, _value: i64) -> Option<bindgen::callbacks::IntKind> {
        if matches!(
            name,
            "IS_CONST"
                | "IS_CV"
                | "IS_SMART_BRANCH_JMPZ"
                | "IS_SMART_BRANCH_JMPNZ"
                | "IS_TMP_VAR"
                | "IS_UNUSED"
                | "IS_VAR"
                | "ZEND_INI_PERDIR"
                | "ZEND_INI_SYSTEM"
                | "ZEND_NOP"
                | "ZEND_USER_OPCODE"
        ) {
            return Some(bindgen::callbacks::IntKind::U8);
        }
        if name.starts_with("ZEND_USER_OPCODE_") || name.starts_with("ZEND_VM_KIND_") {
            return Some(bindgen::callbacks::IntKind::Int);
        }
        return None;
    }
}

fn supported_versions() -> String {
    return PhpVersion::ALL.map(PhpVersion::as_str).join(", ");
}

#[derive(Clone, Copy)]
enum Target {
    Released(PhpVersion),
    Unreleased { version_id: u32 },
}

impl Target {
    const fn engine(self) -> PhpVersion {
        return match self {
            Self::Released(version) => version,
            Self::Unreleased { .. } => PhpVersion::NEWEST,
        };
    }
}

struct Php {
    target: Target,
    zts: bool,
    include_dirs: Vec<PathBuf>,
    config_header: PathBuf,
}

enum Error {
    Spawn {
        command: OsString,
        source: io::Error,
    },
    Failed {
        command: OsString,
        argument: &'static str,
        status: ExitStatus,
        stderr: String,
    },
    NotUtf8 {
        command: OsString,
        argument: &'static str,
    },
    Vernum {
        command: OsString,
        output: String,
    },
    Unsupported {
        command: OsString,
        version: String,
    },
    AllowUnreleased {
        value: OsString,
    },
    NoIncludes {
        command: OsString,
    },
    ReadHeader {
        path: PathBuf,
        source: io::Error,
    },
    OutDir,
    Bindgen(bindgen::BindgenError),
    WriteBindings {
        path: PathBuf,
        source: io::Error,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        return match self {
            Self::Spawn { command, source } => write!(
                f,
                "cannot run {} ({source}): install PHP {} with its development headers, or set {PHP_CONFIG_ENV} to the path of its php-config",
                command.display(),
                supported_versions()
            ),
            Self::Failed {
                command,
                argument,
                status,
                stderr,
            } => write!(
                f,
                "{} {argument} failed with {status}: {}",
                command.display(),
                stderr.trim()
            ),
            Self::NotUtf8 { command, argument } => {
                write!(
                    f,
                    "{} {argument} printed non-UTF-8 output",
                    command.display()
                )
            }
            Self::Vernum { command, output } => write!(
                f,
                "{} --vernum printed {output:?}, expected a number such as 80412",
                command.display()
            ),
            Self::Unsupported { command, version } => write!(
                f,
                "{} reports PHP {version}, which opcov does not support; supported: {}; set {PHP_CONFIG_ENV} to the php-config of a supported PHP, or {ALLOW_UNRELEASED_ENV}=1 to build against an unreleased PHP newer than {}",
                command.display(),
                supported_versions(),
                PhpVersion::NEWEST
            ),
            Self::AllowUnreleased { value } => write!(
                f,
                "{ALLOW_UNRELEASED_ENV} must be 1 or unset, got {}",
                value.display()
            ),
            Self::NoIncludes { command } => {
                write!(
                    f,
                    "{} --includes printed no -I directories",
                    command.display()
                )
            }
            Self::ReadHeader { path, source } => {
                write!(f, "cannot read {} to detect ZTS: {source}", path.display())
            }
            Self::OutDir => write!(f, "cargo did not set OUT_DIR for the build script"),
            Self::Bindgen(source) => {
                write!(f, "bindgen failed on the PHP headers: {source}")
            }
            Self::WriteBindings { path, source } => {
                write!(f, "cannot write {}: {source}", path.display())
            }
        };
    }
}

impl fmt::Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        return fmt::Display::fmt(self, f);
    }
}

fn php_config_command() -> OsString {
    println!("cargo::rerun-if-env-changed={PHP_CONFIG_ENV}");
    return env::var_os(PHP_CONFIG_ENV).unwrap_or_else(|| {
        println!("cargo::rerun-if-env-changed=PATH");
        return OsString::from(DEFAULT_PHP_CONFIG);
    });
}

fn query(command: &OsString, argument: &'static str) -> Result<String, Error> {
    let output = Command::new(command)
        .arg(argument)
        .output()
        .map_err(|source| {
            return Error::Spawn {
                command: command.clone(),
                source,
            };
        })?;
    if !output.status.success() {
        return Err(Error::Failed {
            command: command.clone(),
            argument,
            status: output.status,
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }
    let stdout = String::from_utf8(output.stdout).map_err(|_utf8| {
        return Error::NotUtf8 {
            command: command.clone(),
            argument,
        };
    })?;
    return Ok(stdout.trim().to_owned());
}

fn detect_zts(config_header: &Path) -> Result<bool, Error> {
    let header = fs::read_to_string(config_header).map_err(|source| {
        return Error::ReadHeader {
            path: config_header.to_owned(),
            source,
        };
    })?;
    return Ok(header
        .lines()
        .any(|line| return line.split_whitespace().eq(["#define", "ZTS", "1"])));
}

fn discover() -> Result<Php, Error> {
    let command = php_config_command();
    let version = query(&command, "--version")?;
    let vernum_output = query(&command, "--vernum")?;
    let vernum = vernum_output.parse::<u32>().map_err(|_parse| {
        return Error::Vernum {
            command: command.clone(),
            output: vernum_output.clone(),
        };
    })?;
    let target = match PhpVersion::from_version_id(vernum) {
        Some(released) => Target::Released(released),
        None if vernum > PhpVersion::NEWEST.version_id() && allow_unreleased()? => {
            Target::Unreleased { version_id: vernum }
        }
        None => {
            return Err(Error::Unsupported { command, version });
        }
    };
    let include_dirs: Vec<PathBuf> = query(&command, "--includes")?
        .split_whitespace()
        .filter_map(|flag| return flag.strip_prefix("-I").map(PathBuf::from))
        .collect();
    if include_dirs.is_empty() {
        return Err(Error::NoIncludes { command });
    }
    let config_header = PathBuf::from(query(&command, "--include-dir")?)
        .join("main")
        .join("php_config.h");
    let zts = detect_zts(&config_header)?;
    return Ok(Php {
        target,
        zts,
        include_dirs,
        config_header,
    });
}

fn allow_unreleased() -> Result<bool, Error> {
    println!("cargo::rerun-if-env-changed={ALLOW_UNRELEASED_ENV}");
    return match env::var_os(ALLOW_UNRELEASED_ENV) {
        None => Ok(false),
        Some(value) if value == "1" => Ok(true),
        Some(value) => Err(Error::AllowUnreleased { value }),
    };
}

fn emit_cfgs(php: &Php) {
    let versions = PhpVersion::ALL
        .map(|version| return format!("\"{version}\""))
        .join(", ");
    println!("cargo::rustc-check-cfg=cfg(php_zts)");
    println!("cargo::rustc-check-cfg=cfg(php_version, values({versions}))");
    println!("cargo::rustc-cfg=php_version=\"{}\"", php.target.engine());
    if let Target::Unreleased { version_id } = php.target {
        println!(
            "cargo::warning=building against unreleased PHP {version_id} with the engine model of PHP {} ({ALLOW_UNRELEASED_ENV}=1)",
            PhpVersion::NEWEST
        );
    }
    if php.zts {
        println!("cargo::rustc-cfg=php_zts");
    }
}

fn compile_shim(php: &Php) {
    println!("cargo::rerun-if-changed={SHIM_SOURCE}");
    println!("cargo::rerun-if-changed={}", php.config_header.display());
    cc::Build::new()
        .file(SHIM_SOURCE)
        .include(SHIM_DIR)
        .includes(&php.include_dirs)
        .opt_level(2)
        .warnings(true)
        .extra_warnings(true)
        .compile("opcov_shim");
}

fn generate_bindings(php: &Php, out_dir: &Path) -> Result<(), Error> {
    let builder = bindgen::Builder::default()
        .header(WRAPPER_HEADER)
        .clang_arg(format!("-I{SHIM_DIR}"))
        .clang_args(
            php.include_dirs
                .iter()
                .map(|dir| return format!("-I{}", dir.display())),
        )
        .rust_edition(bindgen::RustEdition::Edition2024)
        .opaque_type(OPAQUE_LIBC_TYPES)
        .blocklist_type(OPAQUE_LIBC_INNER_TYPES)
        .blocklist_type(OPCODE_HANDLER_TYPE)
        .raw_line(format!(
            "pub type {OPCODE_HANDLER_TYPE} = *const ::core::ffi::c_void;"
        ))
        .raw_line(format!(
            "pub const OPCOV_ENGINE_VERSION_ID: u32 = {};",
            php.target.engine().version_id()
        ))
        .generate_cstr(true)
        .derive_default(true)
        .merge_extern_blocks(true)
        .wrap_unsafe_ops(true)
        .formatter(bindgen::Formatter::Prettyplease)
        .prepend_enum_name(false)
        .parse_callbacks(Box::new(MacroTypes))
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()));
    let builder = ENGINE_ITEMS
        .iter()
        .fold(builder, |builder, item| return builder.allowlist_item(item));
    let path = out_dir.join("bindings.rs");
    return builder
        .generate()
        .map_err(Error::Bindgen)?
        .write_to_file(&path)
        .map_err(|source| return Error::WriteBindings { path, source });
}

fn main() -> Result<(), Error> {
    let php = discover()?;
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").ok_or(Error::OutDir)?);
    emit_cfgs(&php);
    compile_shim(&php);
    return generate_bindings(&php, &out_dir);
}
