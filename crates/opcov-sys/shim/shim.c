#include "shim.h"

const char opcov_module_build_id[] = ZEND_MODULE_BUILD_ID;

zend_executor_globals *opcov_executor_globals(void)
{
#ifdef ZTS
	return TSRMG_FAST_BULK(executor_globals_offset, zend_executor_globals *);
#else
	return &executor_globals;
#endif
}

zend_compiler_globals *opcov_compiler_globals(void)
{
#ifdef ZTS
	return TSRMG_FAST_BULK(compiler_globals_offset, zend_compiler_globals *);
#else
	return &compiler_globals;
#endif
}

zend_arena *opcov_arena_create(size_t size)
{
	return zend_arena_create(size);
}

void opcov_arena_destroy(zend_arena *arena)
{
	zend_arena_destroy(arena);
}

void opcov_zval_null(zval *target)
{
	ZVAL_NULL(target);
}

void opcov_zval_bool(zval *target, bool value)
{
	ZVAL_BOOL(target, value);
}

void opcov_zval_long(zval *target, zend_long value)
{
	ZVAL_LONG(target, value);
}

void opcov_zval_stringl(zval *target, const char *str, size_t len)
{
	ZVAL_STRINGL(target, str, len);
}

const void *opcov_op_handler(const zend_op *op)
{
	return (const void *) op->handler;
}

bool opcov_parse_parameters_none(zend_execute_data *execute_data)
{
	return zend_parse_parameters_none() == SUCCESS;
}

bool opcov_parse_path(zend_execute_data *execute_data, const char **path, size_t *len)
{
	char *value;
	ZEND_PARSE_PARAMETERS_START(1, 1)
		Z_PARAM_PATH(value, *len)
	ZEND_PARSE_PARAMETERS_END_EX(return false);
	*path = value;
	return true;
}

bool opcov_parse_array(zend_execute_data *execute_data, HashTable **array)
{
	ZEND_PARSE_PARAMETERS_START(1, 1)
		Z_PARAM_ARRAY_HT(*array)
	ZEND_PARSE_PARAMETERS_END_EX(return false);
	return true;
}

bool opcov_is_user_code(const zend_function *function)
{
	return ZEND_USER_CODE(function->type);
}

bool opcov_is_user_class(const zend_class_entry *class_entry)
{
	return class_entry->type == ZEND_USER_CLASS;
}

zend_long opcov_ini_long(const char *name, size_t len)
{
	return zend_ini_long(name, len, 0);
}

const char *opcov_ini_string(const char *name, size_t len)
{
	return zend_ini_string_ex(name, len, 0, NULL);
}

zend_string *opcov_string_init(const char *str, size_t len, bool persistent)
{
	return zend_string_init(str, len, persistent);
}

void opcov_string_release(zend_string *string)
{
	zend_string_release(string);
}

void opcov_array_init(zval *target, uint32_t size)
{
	array_init_size(target, size);
}

void opcov_add_index_zval(zval *array, zend_ulong index, zval *value)
{
	add_index_zval(array, index, value);
}

void opcov_add_next_index_zval(zval *array, zval *value)
{
	add_next_index_zval(array, value);
}

void *opcov_map_ptr_at(const HashTable *map, uint32_t index)
{
	const Bucket *bucket = map->arData + index;
	return Z_TYPE(bucket->val) == IS_UNDEF ? NULL : Z_PTR(bucket->val);
}

zval *opcov_array_value_at(HashTable *array, uint32_t index)
{
	zval *value = HT_IS_PACKED(array) ? array->arPacked + index : &array->arData[index].val;
	if (Z_TYPE_P(value) == IS_UNDEF) {
		return NULL;
	}
	ZVAL_DEREF(value);
	return value;
}

bool opcov_read_string(const zval *value, const char **str, size_t *len)
{
	if (Z_TYPE_P(value) != IS_STRING) {
		return false;
	}
	*str = Z_STRVAL_P(value);
	*len = Z_STRLEN_P(value);
	return true;
}

bool opcov_read_long(const zval *value, zend_long *number)
{
	if (Z_TYPE_P(value) != IS_LONG) {
		return false;
	}
	*number = Z_LVAL_P(value);
	return true;
}

zend_class_entry *opcov_register_class(const char *name, size_t len, zend_class_entry *parent)
{
	zend_class_entry entry;
	INIT_CLASS_ENTRY_EX(entry, name, len, NULL);
	return zend_register_internal_class_ex(&entry, parent);
}
