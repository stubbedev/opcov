#ifndef OPCOV_SHIM_H
#define OPCOV_SHIM_H

#include "php.h"
#include "zend_arena.h"

typedef struct _pcre_cache_entry pcre_cache_entry;

PHPAPI pcre_cache_entry *pcre_get_compiled_regex_cache(zend_string *regex);
PHPAPI void php_pcre_pce_incref(pcre_cache_entry *pce);
PHPAPI void php_pcre_pce_decref(pcre_cache_entry *pce);
PHPAPI void php_pcre_match_impl(pcre_cache_entry *pce, zend_string *subject_str, zval *return_value,
	zval *subpats, bool global, zend_long flags, zend_off_t start_offset);

extern const char opcov_module_build_id[];

zend_executor_globals *opcov_executor_globals(void);
zend_compiler_globals *opcov_compiler_globals(void);
zend_arena *opcov_arena_create(size_t size);
void opcov_arena_destroy(zend_arena *arena);
void opcov_zval_null(zval *target);
void opcov_zval_bool(zval *target, bool value);
void opcov_zval_long(zval *target, zend_long value);
void opcov_zval_stringl(zval *target, const char *str, size_t len);
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
bool opcov_is_user_code(const zend_function *function);
bool opcov_is_user_class(const zend_class_entry *class_entry);
zend_long opcov_ini_long(const char *name, size_t len);
const char *opcov_ini_string(const char *name, size_t len);
zend_string *opcov_string_init(const char *str, size_t len, bool persistent);
void opcov_string_release(zend_string *string);
void opcov_array_init(zval *target, uint32_t size);
void opcov_add_index_zval(zval *array, zend_ulong index, zval *value);
void opcov_add_next_index_zval(zval *array, zval *value);
void *opcov_map_ptr_at(const HashTable *map, uint32_t index);
zval *opcov_array_value_at(HashTable *array, uint32_t index);
bool opcov_read_string(const zval *value, const char **str, size_t *len);
bool opcov_read_long(const zval *value, zend_long *number);
zend_class_entry *opcov_register_class(const char *name, size_t len, zend_class_entry *parent);

#endif
