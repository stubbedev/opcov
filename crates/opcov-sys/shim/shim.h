#ifndef OPCOV_SHIM_H
#define OPCOV_SHIM_H

#include "php.h"
#include "zend_arena.h"
#include "Optimizer/zend_cfg.h"

typedef struct _pcre_cache_entry pcre_cache_entry;

PHPAPI pcre_cache_entry *pcre_get_compiled_regex_cache(zend_string *regex);
PHPAPI void php_pcre_pce_incref(pcre_cache_entry *pce);
PHPAPI void php_pcre_pce_decref(pcre_cache_entry *pce);
PHPAPI void php_pcre_match_impl(pcre_cache_entry *pce, zend_string *subject_str, zval *return_value,
	zval *subpats, bool global, zend_long flags, zend_off_t start_offset);

extern const char opcov_module_build_id[];
extern const uint8_t opcov_internal_function_type;

typedef zend_op_array *(*opcov_compile_file_t)(zend_file_handle *handle, int type);
typedef zend_op_array *(*opcov_compile_string_t)(zend_string *source, const char *filename, zend_compile_position position);

zend_executor_globals *opcov_executor_globals(void);
zend_compiler_globals *opcov_compiler_globals(void);
zend_arena *opcov_arena_create(size_t size);
bool opcov_arena_create_caught(size_t size, zend_arena **arena);
bool opcov_build_cfg(zend_arena **arena, const zend_op_array *op_array, zend_cfg *cfg);
void opcov_bailout(void);
bool opcov_compile_file(opcov_compile_file_t compile, zend_file_handle *handle, int type, zend_op_array **result);
bool opcov_compile_string(opcov_compile_string_t compile, zend_string *source, const char *filename, zend_compile_position position, zend_op_array **result);
const char *opcov_sapi_name(void);
bool opcov_module_loaded(const char *name, size_t len);
bool opcov_ini_bool(const char *name, size_t len);
bool opcov_throw(zend_class_entry *class_entry, const char *message, size_t len);
bool opcov_pcre_compile(const char *pattern, size_t len, pcre_cache_entry **entry);
bool opcov_pcre_match(pcre_cache_entry *entry, const char *subject, size_t len, int *matched);
bool opcov_add_assoc_zval(zval *array, const char *key, size_t len, zval *value);
void opcov_arena_destroy(zend_arena *arena);
void opcov_zval_null(zval *target);
void opcov_zval_bool(zval *target, bool value);
void opcov_zval_long(zval *target, zend_long value);
bool opcov_zval_stringl(zval *target, const char *str, size_t len);
const void *opcov_op_handler(const zend_op *op);
bool opcov_parse_parameters_none(zend_execute_data *execute_data);
bool opcov_parse_path(zend_execute_data *execute_data, const char **path, size_t *len);
bool opcov_parse_array(zend_execute_data *execute_data, HashTable **array);
bool opcov_parse_optional_string(zend_execute_data *execute_data, const char **str, size_t *len);
bool opcov_parse_path_and_array(zend_execute_data *execute_data, const char **path, size_t *len, HashTable **array);
bool opcov_array_entry_at(HashTable *array, uint32_t index, zend_string **key, zend_ulong *number, zval **value);
bool opcov_read_bool(const zval *value, bool *flag);
const char *opcov_type_name(const zval *value);
const char *opcov_getcwd(char *buffer, size_t size);
const char *opcov_php_version(void);
bool opcov_is_user_class(const zend_class_entry *class_entry);
zend_long opcov_ini_long(const char *name, size_t len);
const char *opcov_ini_string(const char *name, size_t len);
void opcov_string_release(zend_string *string);
bool opcov_array_init(zval *target, uint32_t size);
bool opcov_add_index_zval(zval *array, zend_ulong index, zval *value);

typedef struct opcov_loop_hit {
	zend_string *key;
	bool zero;
	bool second;
} opcov_loop_hit;

typedef struct opcov_function_hits {
	zend_string *key;
	zend_string *const *outcomes;
	size_t outcome_count;
	const opcov_loop_hit *loops;
	size_t loop_count;
} opcov_function_hits;

typedef struct opcov_file_hits {
	zend_string *path;
	const uint32_t *lines;
	size_t line_count;
	const opcov_function_hits *functions;
	size_t function_count;
} opcov_file_hits;

typedef struct opcov_hit_keys {
	zend_string *lines;
	zend_string *functions;
	zend_string *outcomes;
	zend_string *loops;
	zend_string *zero;
	zend_string *second;
} opcov_hit_keys;

zend_string *opcov_persistent_key(const char *bytes, size_t len);
bool opcov_emit_hits(zval *target, const opcov_hit_keys *keys, const opcov_file_hits *files, size_t count);
bool opcov_add_next_index_zval(zval *array, zval *value);
void *opcov_map_ptr_at(const HashTable *map, uint32_t index);
zval *opcov_array_value_at(HashTable *array, uint32_t index);
bool opcov_read_string(const zval *value, const char **str, size_t *len);
bool opcov_read_long(const zval *value, zend_long *number);
zend_class_entry *opcov_register_class(const char *name, size_t len, zend_class_entry *parent);
int opcov_branch_truth(const zend_execute_data *execute_data, const zend_op *op);
int opcov_constant_truth(const zend_op *op);
int opcov_branch_set(const zend_execute_data *execute_data, const zend_op *op);
int opcov_branch_null(const zend_execute_data *execute_data, const zend_op *op);
int64_t opcov_match_target(const zend_execute_data *execute_data, const zend_op *op, const zend_op *opcodes);
int opcov_foreach_started(const zend_execute_data *execute_data, const zend_op *op);
bool opcov_fast_call_threw(const zend_execute_data *execute_data, uint32_t var);
uint32_t opcov_fast_call_var(const zend_op_array *op_array, uint32_t finally_op);

#endif
