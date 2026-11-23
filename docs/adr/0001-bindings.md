# ADR 0001: Zend engine bindings

- Status: accepted
- Date: 2026-11-23
- Issues: #2 (bindings and scaffold), #18 (panic guard), #16 (test foundation)

## Decision

opcov binds to the Zend engine through its own `opcov-sys` crate:

- `bindgen` runs over the headers of the exact PHP being targeted, found through
  `php-config` (`PHP_CONFIG`, else `php-config` on `PATH`), with an allowlist of
  the engine items opcov uses (`ENGINE_ITEMS` in `crates/opcov-sys/build.rs`).
- A C shim compiled by `cc` (`crates/opcov-sys/shim/shim.c`, about 180 lines plus a 45-line header) exports
  the inline functions, macros and TSRM-indirected globals that bindgen cannot
  emit. It holds accessors only.
- Zend macros that are pure data (`STANDARD_MODULE_HEADER`/`_PROPERTIES`,
  `ZEND_NS_FE`, `ZEND_FE_END`, `ZEND_BEGIN_ARG_WITH_RETURN_TYPE_INFO_EX`) are
  rewritten in Rust in `opcov-sys` (`Module`, `FunctionTable`, `ArgInfo`,
  `function_entry`) on top of the generated struct layouts, as `const fn`s
  evaluated into statics.
- `opcov-ext` (the `cdylib`) hand-writes the module entry, the functions and the
  info hook on top of that. It is the only crate besides `opcov-sys` with
  `unsafe`, and both opt in at the crate root with `#![expect(unsafe_code)]`.

ext-php-rs is not used, neither alone nor in a hybrid.

## Options evaluated

Each option was built into a throwaway extension and loaded into PHP 8.4.27,
8.5.12 and 8.6.0RC4 from nixpkgs, NTS and ZTS (x86_64 Linux, glibc).

### ext-php-rs 0.16.5

- Supports PHP 8.0 to 8.6 (`ext-php-rs-build` 0.2.1 knows API 20260924) and ZTS.
  A spike extension (one function returning a string, one reading
  `ExecutorGlobals`) built and loaded on all six PHP builds.
- Its build script splits `php-config --includes` without trimming, passing
  `-I.../ext/date/lib\n`; through `sccache` (our dev shell's `RUSTC_WRAPPER`)
  that fails the C wrapper's compilation on every PHP build. It works with the
  wrapper disabled.
- Its bindings come from a fixed allowlist (`allowed_bindings.rs`, 436 items).
  The spike failed to compile against `ext_php_rs::ffi::zend_set_user_opcode_handler`,
  `zend_vm_set_opcode_handler`, `zend_build_cfg`, `zend_cfg_identify_loops`,
  `zend_compile_file`, `zend_get_resource_handle` and `zend_register_extension`
  (the last two exist only behind its `observer` feature); `zend_op` and
  `zend_cfg` are not bound at all, and `Zend/Optimizer/zend_cfg.h` is not in
  its wrapper header. Every probe mechanism of #3 to #7 needs exactly these.
- A hybrid would therefore need our own bindgen pass for the internals anyway,
  and the two passes produce distinct Rust types for the same C structs
  (`zend_execute_data`, `zend_op_array`, `zval`), so values could not flow
  between them without casts. It would also pull in `inventory`, `parking_lot`,
  `once_cell`, `bitflags`, `indexmap`, `anyhow` and two proc-macro crates for a
  surface of two functions and an info table.

### Own `opcov-sys` (chosen)

- Builds, loads and passes the `.phpt` suite on PHP 8.4, 8.5 and 8.6, NTS and
  ZTS, with GCC-built PHP (HYBRID VM) and with clang-built PHP 8.4 (CALL VM)
  and 8.5 (TAILCALL VM).
- The generated bindings carry bindgen's layout assertions (size, alignment and
  every field offset, as `const` items) for each engine struct, checked by
  rustc against clang's layout of the exact headers in use. This is the
  per-version `#[repr(C)]` layout check #18 asks for, without
  `static_assertions`.
- Cost: the module entry, arginfo and function table are written by hand
  (about 150 lines in `opcov-sys/src/lib.rs`).

## Macros and inline functions: C shim, not Rust rewrites

A Rust rewrite of `EG()`/`CG()` would have to reproduce TSRM's offset scheme
(`TSRMG_FAST_BULK(executor_globals_offset, ...)` over `tsrm_get_ls_cache()`)
and `ZEND_USER_CODE`, `zend_parse_parameters_none()`, `ZVAL_STRINGL` and
`zend_arena_create/destroy` for every version, and would silently drift when the
engine changes them. The shim compiles the engine's own definitions against the
target headers:

| Shim symbol | Engine definition it wraps |
|---|---|
| `opcov_module_build_id` | `ZEND_MODULE_BUILD_ID` (string-concatenation macro) |
| `opcov_executor_globals()` | `&executor_globals` (NTS), `TSRMG_FAST_BULK(executor_globals_offset, ...)` (ZTS) |
| `opcov_compiler_globals()` | `&compiler_globals` (NTS), `TSRMG_FAST_BULK(compiler_globals_offset, ...)` (ZTS) |
| `opcov_arena_create()`, `opcov_arena_destroy()` | inline `zend_arena_create`, `zend_arena_destroy` |
| `opcov_zval_stringl()` | `ZVAL_STRINGL` |
| `opcov_op_handler()` | `zend_op.handler` read as an address, whatever its type for the VM kind |
| `opcov_parse_parameters_none()` | `zend_parse_parameters_none()` |
| `opcov_is_user_code()` | `ZEND_USER_CODE(function->type)` (an enum constant on 8.6 that bindgen drops) |
| `opcov_is_user_class()` | `ce->type == ZEND_USER_CLASS` (an enum constant on 8.6 that bindgen drops) |
| `opcov_ini_long()`, `opcov_ini_string()` | `zend_ini_long()`, `zend_ini_string_ex()`, whose `orig` parameter is `int` before 8.6 and `bool` since |
| `opcov_parse_path()`, `opcov_parse_array()` | `ZEND_PARSE_PARAMETERS_START(1, 1)` with `Z_PARAM_PATH` / `Z_PARAM_ARRAY_HT` |
| `opcov_zval_null()`, `opcov_zval_bool()`, `opcov_zval_long()` | `ZVAL_NULL`, `ZVAL_BOOL`, `ZVAL_LONG` |
| `opcov_read_string()`, `opcov_read_long()` | `Z_TYPE_P` check plus `Z_STRVAL_P`/`Z_STRLEN_P`, `Z_LVAL_P` |
| `opcov_array_init()` | `array_init_size` |
| `opcov_add_index_zval()`, `opcov_add_next_index_zval()` | inline `add_index_zval`, `add_next_index_zval` |
| `opcov_map_ptr_at()` | `Z_PTR` of bucket `n` of a hash map, null for `IS_UNDEF` (iterating `CG(function_table)`, `CG(class_table)`, a class's methods and properties) |
| `opcov_array_value_at()` | element `n` of a packed or hash array, dereferenced, null for `IS_UNDEF` |
| `opcov_string_init()`, `opcov_string_release()` | inline `zend_string_init`, `zend_string_release` |
| `opcov_register_class()` | `INIT_CLASS_ENTRY_EX` plus `zend_register_internal_class_ex` |

`shim.h` also declares four PCRE functions by hand (`pcre_get_compiled_regex_cache`,
`php_pcre_pce_incref`, `php_pcre_pce_decref`, `php_pcre_match_impl`, with
`pcre_cache_entry` as an opaque type) instead of including
`ext/pcre/php_pcre.h`: with an external PCRE2 (nixpkgs, most distributions)
that header includes `pcre2.h`, which `php-config --includes` does not list.
The four signatures are identical in the 8.4, 8.5 and 8.6 headers.

The ZTS accessors call `tsrm_get_ls_cache()` on every use instead of caching it
in a `ZEND_TSRMLS_CACHE` thread-local. The probe callback (#5) needs none of
them: it reads the frame the VM passes and the op array's reserved slot, and
checks thread ownership only once a second thread has started.

## Panics: `panic = "unwind"` and a guard at every entry point

#2 asked for `panic = "abort"` in the release profile and `catch_unwind` at
every `extern "C"` entry point; #18 requires a panic to leave PHP running. Under
`panic = "abort"`, `catch_unwind` never catches anything and a panic kills the
PHP process, so the two cannot both hold. The release profile now uses
`panic = "unwind"`, and `opcov-ext` fails to compile (`compile_error!` under
`cfg(panic = "abort")`) if anyone switches it back.

Every engine-facing function (`get_module`, the info hook, every PHP function)
runs its body through `guard::guard`, which wraps it in `catch_unwind`. On a
panic it records the first reason (entry point and panic message) in a
process-wide `OnceLock`, returns a neutral value to the engine, and PHP carries
on. `php --ri opcov` shows `Collection => disabled: panic in <entry>: <message>`.
After recording the reason the guard puts the original handler back on every
armed probe; the probe callback then only forwards, and new op arrays get
their real handlers but no probes. The guard covers module and request
startup and shutdown, the compile hooks, the `op_array_dtor` hook, the probe
callback and every PHP function.

Two constraints found while proving it:

- The Cranelift backend does not implement unwinding: built with it,
  `catch_unwind` in the extension catches nothing and the guard's unit tests
  fail. The dev profile therefore uses LLVM again (`nix/toolchain.nix` no longer
  sets `CARGO_PROFILE_DEV_CODEGEN_BACKEND=cranelift`). Cargo cannot select a
  backend per package from the environment, and per-package profile settings
  need `cargo-features = ["codegen-backend"]`, which Dependabot's stable cargo
  cannot parse. A `.cargo/config.toml` override for `opcov-ext` alone would bring
  Cranelift back for the other crates.
- Engine calls that can raise a fatal error `longjmp` through Rust frames
  (`zend_bailout`). `catch_unwind` does not see that, and any Rust value with a
  destructor between the engine and the `longjmp` target leaks or is skipped.
  Code calling such engine functions must hold no owning values across the call.

Every `unsafe` block carries a `// SAFETY:` comment, enforced by
`clippy::undocumented_unsafe_blocks` (denied workspace-wide); the generated
bindings are the only exemption (`#[expect]` on the `bindings` module).

## The probe-label trick on every VM kind

The `engine-selftest` cargo feature adds `\opcov\engine_selftest()` and
`\opcov\selftest_panic()`; the release package never has them, the `.phpt`
suite runs against a build with them. The self-test, run from
`tests/phpt/engine/selftest.phpt`, proves on each PHP build:

1. Probe label: with a user handler registered for `ZEND_NOP`,
   `zend_vm_set_opcode_handler()` gives a NOP the `ZEND_USER_OPCODE` handler,
   which differs from the real NOP handler; after restoring the previous user
   handler the NOP gets its real handler back.
2. `zend_compile_file` and `zend_compile_string` are set.
3. `EG(current_execute_data)` is the self-test's own frame and
   `EG(function_table) == CG(function_table)`, through the shim accessors (ZTS
   included).
4. `zend_get_resource_handle()` returns a slot.
5. `zend_register_extension()` registers an extension that
   `zend_get_extension()` then finds.
6. `zend_build_cfg`, `zend_cfg_build_predecessors`,
   `zend_cfg_compute_dominators_tree` and `zend_cfg_identify_loops` on the
   calling script's op array find its one loop header.

Results (x86_64 Linux, nixpkgs):

| PHP | Compiler | VM kind | Load + `.phpt` | Self-test |
|---|---|---|---|---|
| 8.4.27 NTS / ZTS | GCC | HYBRID | pass | all ok |
| 8.5.12 NTS / ZTS | GCC | HYBRID | pass | all ok |
| 8.6.0RC4 NTS / ZTS | GCC | HYBRID | pass | all ok |
| 8.4.27 NTS | clang | CALL | pass | all ok |
| 8.5.12 NTS | clang | TAILCALL | pass | all ok |

Findings:

- The VM kind is decided when PHP is compiled: HYBRID needs GCC's global
  register variables (`HAVE_GCC_GLOBAL_REGS`). A clang-built PHP 8.4 runs the
  CALL VM; a clang-built 8.5 or 8.6 runs the TAILCALL VM (new in 8.5, needs
  `musttail` and `preserve_none`). macOS PHP is clang-built, so macOS CI
  exercises CALL/TAILCALL rather than HYBRID.
- On the TAILCALL VM `zend_vm_opcode_handler_t` is a `preserve_none` function
  pointer, a calling convention Rust cannot express and bindgen panics on. The
  bindings declare it as `*const c_void`: opcov only stores, compares and
  restores handler addresses and never calls a handler from Rust.
- `\opcov` reports the VM of the running engine (`zend_vm_kind()`), not the one
  the headers imply.

## Build-time PHP discovery

`opcov-sys/build.rs` runs `php-config --version`, `--vernum`, `--includes` and
`--include-dir`, accepts 8.4, 8.5 and 8.6 only, detects ZTS from
`main/php_config.h` and fails with a message naming the command, the PHP version
found and `PHP_CONFIG`. It exposes `cfg(php_version = "8.4" | "8.5" | "8.6")` and
`cfg(php_zts)` to `opcov-sys`; `opcov-ext` needs neither.

## Engine symbols used

Undefined dynamic symbols of `opcov.so` (from `nm -D --undefined-only`, PHP 8.5
NTS and ZTS, release features), resolved against the PHP binary (libc symbols
left out):

| Symbol | Used for |
|---|---|
| `executor_globals` (NTS), `executor_globals_offset` (ZTS) | `EG()`: current frame, `error_reporting` |
| `compiler_globals` (NTS), `compiler_globals_offset` (ZTS) | `CG()`: `compiler_options`, `function_table`, `class_table` |
| `tsrm_get_ls_cache` (ZTS) | `EG()`/`CG()` on ZTS |
| `_emalloc`, `_efree`, `__zend_malloc`, `zend_string_init_interned`, `std_object_handlers` | inside the shim's string, class-entry and arena wrappers |
| `_zend_new_array`, `zend_hash_index_update`, `zend_hash_next_index_insert`, `add_assoc_zval_ex` | building the arrays the PHP functions return |
| `zend_parse_arg_str_slow`, `zend_wrong_parameter_error`, `zend_wrong_parameters_count_error`, `zend_wrong_parameters_none_error` | inside the shim's argument parsing |
| `zend_register_ini_entries_ex`, `zend_unregister_ini_entries_ex`, `zend_ini_long`, `zend_ini_string_ex`, `display_ini_entries` | the four INI settings and `php --ri` |
| `zend_register_internal_class_ex`, `spl_ce_RuntimeException`, `spl_ce_LogicException`, `zend_throw_exception` | `\opcov\CannotCollectException` and the `LogicException`s of the API |
| `php_info_print_table_start`, `_row`, `_end` | `php --ri opcov` |
| `zend_vm_kind` | VM kind in `php --ri` and the self-test |
| `zend_set_user_opcode_handler`, `zend_get_user_opcode_handler` | registering the probe callback for every opcode, saving and restoring other extensions' handlers |
| `zend_vm_set_opcode_handler` | the probe label, and the real handlers after `pass_two()` |
| `zend_compile_file`, `zend_compile_string` | the compile hooks (#3, #5) |
| `zend_resolve_path` | resolving a file before compilation for the jump-table decision |
| `zend_execute_ex`, `execute_ex` | detecting the stock executor for declarations-only files |
| `zend_get_resource_handle` | the `op_array.reserved[]` slot that maps an op array to its unit |
| `zend_register_extension` | the runtime `zend_extension` whose `op_array_dtor` frees units |
| `zend_build_cfg`, `zend_cfg_build_predecessors`, `zend_cfg_compute_dominators_tree`, `zend_cfg_identify_loops` | the CFG snapshot (#4) |
| `pcre_get_compiled_regex_cache`, `php_pcre_pce_incref`, `php_pcre_pce_decref`, `php_pcre_match_impl` | `opcov.exclude_pattern` |
| `zend_get_extension` | self-test only (`engine-selftest`) |

Constants and types taken from the headers at compile time:
`ZEND_MODULE_API_NO`, `ZEND_DEBUG`, `PHP_API_VERSION`, `PHP_VERSION`,
`PHP_VERSION_ID`, `ZEND_NOP`, `ZEND_USER_OPCODE`, `ZEND_USER_OPCODE_CONTINUE`,
`ZEND_USER_OPCODE_DISPATCH`, `ZEND_VM_KIND_*`, `IS_UNUSED`, `IS_CONST`,
`IS_TMP_VAR`, `IS_VAR`, `IS_CV`, `MAY_BE_ARRAY`, `MAY_BE_STRING`,
`MAY_BE_VOID`, `ZEND_ACC_ABSTRACT`, `ZEND_ACC_ANON_CLASS`, `ZEND_ACC_CLOSURE`,
`ZEND_ACC_DONE_PASS_TWO`, `ZEND_BB_START`, `ZEND_BB_CATCH`,
`ZEND_BB_FINALLY`, `ZEND_BB_FINALLY_END`, `ZEND_BB_RECV_ENTRY`,
`ZEND_BB_REACHABLE`, `ZEND_BB_LOOP_HEADER`, `ZEND_COMPILE_NO_JUMPTABLES`,
`ZEND_INI_SYSTEM`, `ZEND_INI_PERDIR`, `ZEND_PROPERTY_HOOK_COUNT`, `SUCCESS`,
and the structs `zend_module_entry`, `zend_function_entry`,
`zend_internal_arg_info`, `zend_ini_entry_def`, `zend_execute_data`,
`zend_function`, `zend_op`, `zend_op_array`, `zend_class_entry`,
`zend_property_info`, `zend_file_handle`, `zend_string`, `HashTable`,
`zend_cfg`, `zend_basic_block`, `zend_extension`, `zval` and the engine
globals.

## Generated code and lints

The bindings are included in a private `bindings` module with one `#[expect]`
listing the lints bindgen output trips (naming, `unsafe_code`, the layout
assertions' indexing and arithmetic, bindgen's own `#[allow]`s, implicit
returns). The list is identical for all eight PHP builds above; libc structs
that differ between glibc and macOS (`FILE`, `sigaction`, `jmp_buf`) are
generated opaque so their fields cannot change that set. If a lint fires on one
platform only, the `expect` turns into `unfulfilled_lint_expectations` there;
the narrowest sound fix is then to make the offending libc type opaque too.
