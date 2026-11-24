#include "shim.h"
#include "php_main.h"
#include "SAPI.h"
#include "zend_exceptions.h"

#define OPCOV_TRY(body) \
	do { \
		bool opcov_completed = true; \
		zend_try { \
			body; \
		} zend_catch { \
			opcov_completed = false; \
		} zend_end_try(); \
		return opcov_completed; \
	} while (0)

const char opcov_module_build_id[] = ZEND_MODULE_BUILD_ID;
const uint8_t opcov_internal_function_type = ZEND_INTERNAL_FUNCTION;

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

bool opcov_arena_create_caught(size_t size, zend_arena **arena)
{
	OPCOV_TRY(*arena = zend_arena_create(size));
}

bool opcov_build_cfg(zend_arena **arena, const zend_op_array *op_array, zend_cfg *cfg)
{
	OPCOV_TRY(zend_build_cfg(arena, op_array, 0, cfg));
}

void opcov_bailout(void)
{
	zend_bailout();
}

bool opcov_compile_file(opcov_compile_file_t compile, zend_file_handle *handle, int type, zend_op_array **result)
{
	OPCOV_TRY(*result = compile(handle, type));
}

bool opcov_compile_string(opcov_compile_string_t compile, zend_string *source, const char *filename, zend_compile_position position, zend_op_array **result)
{
	OPCOV_TRY(*result = compile(source, filename, position));
}

const char *opcov_sapi_name(void)
{
	return sapi_module.name;
}

bool opcov_module_loaded(const char *name, size_t len)
{
	return zend_hash_str_exists(&module_registry, name, len);
}

bool opcov_ini_bool(const char *name, size_t len)
{
	return zend_ini_parse_bool(zend_ini_str(name, len, false));
}

bool opcov_throw(zend_class_entry *class_entry, const char *message, size_t len)
{
	OPCOV_TRY(
		zend_string *copy = zend_string_init(message, len, false);
		zend_throw_exception(class_entry, ZSTR_VAL(copy), 0);
		zend_string_release(copy));
}

bool opcov_pcre_compile(const char *pattern, size_t len, pcre_cache_entry **entry)
{
	OPCOV_TRY(
		zend_string *regex = zend_string_init(pattern, len, false);
		*entry = pcre_get_compiled_regex_cache(regex);
		zend_string_release(regex));
}

bool opcov_pcre_match(pcre_cache_entry *entry, const char *subject, size_t len, int *matched)
{
	OPCOV_TRY(
		zend_string *string = zend_string_init(subject, len, false);
		zval result;
		ZVAL_UNDEF(&result);
		php_pcre_match_impl(entry, string, &result, NULL, false, 0, 0);
		zend_string_release(string);
		*matched = Z_TYPE(result) == IS_LONG ? Z_LVAL(result) > 0 : -1);
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

bool opcov_zval_stringl(zval *target, const char *str, size_t len)
{
	OPCOV_TRY(ZVAL_STRINGL(target, str, len));
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

const HashTable *opcov_parse_array(zend_execute_data *execute_data)
{
	HashTable *array;
	ZEND_PARSE_PARAMETERS_START(1, 1)
		Z_PARAM_ARRAY_HT(array)
	ZEND_PARSE_PARAMETERS_END_EX(return NULL);
	return array;
}

bool opcov_parse_optional_string(zend_execute_data *execute_data, const char **str, size_t *len)
{
	char *value = NULL;
	size_t length = 0;
	ZEND_PARSE_PARAMETERS_START(0, 1)
		Z_PARAM_OPTIONAL
		Z_PARAM_STRING_OR_NULL(value, length)
	ZEND_PARSE_PARAMETERS_END_EX(return false);
	*str = value;
	*len = length;
	return true;
}

bool opcov_parse_path_and_array(zend_execute_data *execute_data, const char **path, size_t *len, const HashTable **array)
{
	char *value;
	HashTable *options = NULL;
	ZEND_PARSE_PARAMETERS_START(1, 2)
		Z_PARAM_PATH(value, *len)
		Z_PARAM_OPTIONAL
		Z_PARAM_ARRAY_HT(options)
	ZEND_PARSE_PARAMETERS_END_EX(return false);
	*path = value;
	*array = options;
	return true;
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

void opcov_string_release(zend_string *string)
{
	zend_string_release(string);
}

bool opcov_array_init(zval *target, uint32_t size)
{
	OPCOV_TRY(array_init_size(target, size));
}

bool opcov_add_assoc_zval(zval *array, const char *key, size_t len, zval *value)
{
	OPCOV_TRY(add_assoc_zval_ex(array, key, len, value));
}

bool opcov_add_index_zval(zval *array, zend_ulong index, zval *value)
{
	OPCOV_TRY(add_index_zval(array, index, value));
}

zend_string *opcov_persistent_key(const char *bytes, size_t len)
{
	zend_string *key = zend_string_init(bytes, len, 1);
	zend_string_hash_val(key);
	GC_MAKE_PERSISTENT_LOCAL(key);
	return key;
}

static void opcov_add_child(HashTable *parent, zend_string *key, uint32_t size, zval *child)
{
	if (size == 0) {
		ZVAL_EMPTY_ARRAY(child);
	} else {
		array_init_size(child, size);
	}
	zend_hash_add_new(parent, key, child);
}

static void opcov_emit_function(HashTable *functions, const opcov_hit_keys *keys, const opcov_function_hits *hits)
{
	zval function, outcomes, loops, flag;
	ZVAL_TRUE(&flag);
	array_init_size(&function, 2);
	zend_symtable_update(functions, hits->key, &function);
	opcov_add_child(Z_ARRVAL(function), keys->outcomes, (uint32_t) hits->outcome_count, &outcomes);
	for (size_t i = 0; i < hits->outcome_count; i++) {
		zend_hash_add_new(Z_ARRVAL(outcomes), hits->outcomes[i], &flag);
	}
	opcov_add_child(Z_ARRVAL(function), keys->loops, (uint32_t) hits->loop_count, &loops);
	for (size_t i = 0; i < hits->loop_count; i++) {
		const opcov_loop_hit *hit = &hits->loops[i];
		zval looped;
		opcov_add_child(Z_ARRVAL(loops), hit->key, 2, &looped);
		if (hit->zero) {
			zend_hash_add_new(Z_ARRVAL(looped), keys->zero, &flag);
		}
		if (hit->second) {
			zend_hash_add_new(Z_ARRVAL(looped), keys->second, &flag);
		}
	}
}

static void opcov_emit_files(zval *target, const opcov_hit_keys *keys, const opcov_file_hits *files, size_t count)
{
	zval flag;
	ZVAL_TRUE(&flag);
	array_init_size(target, (uint32_t) count);
	for (size_t f = 0; f < count; f++) {
		const opcov_file_hits *file = &files[f];
		zval entry, lines, functions;
		array_init_size(&entry, 2);
		zend_symtable_update(Z_ARRVAL_P(target), file->path, &entry);
		opcov_add_child(Z_ARRVAL(entry), keys->lines, (uint32_t) file->line_count, &lines);
		if (file->line_count > 0) {
			zend_hash_real_init_mixed(Z_ARRVAL(lines));
		}
		for (size_t i = 0; i < file->line_count; i++) {
			zend_hash_index_add_new(Z_ARRVAL(lines), file->lines[i], &flag);
		}
		opcov_add_child(Z_ARRVAL(entry), keys->functions, (uint32_t) file->function_count, &functions);
		for (size_t i = 0; i < file->function_count; i++) {
			opcov_emit_function(Z_ARRVAL(functions), keys, &file->functions[i]);
		}
	}
}

bool opcov_emit_hits(zval *target, const opcov_hit_keys *keys, const opcov_file_hits *files, size_t count)
{
	OPCOV_TRY(opcov_emit_files(target, keys, files, count));
}

bool opcov_add_next_index_zval(zval *array, zval *value)
{
	OPCOV_TRY(add_next_index_zval(array, value));
}

void *opcov_map_ptr_at(const HashTable *map, uint32_t index)
{
	const Bucket *bucket = map->arData + index;
	return Z_TYPE(bucket->val) == IS_UNDEF ? NULL : Z_PTR(bucket->val);
}

const zval *opcov_array_value_at(const HashTable *array, uint32_t index)
{
	const zval *value = HT_IS_PACKED(array) ? array->arPacked + index : &array->arData[index].val;
	if (Z_TYPE_P(value) == IS_UNDEF) {
		return NULL;
	}
	ZVAL_DEREF(value);
	return value;
}

bool opcov_array_entry_at(const HashTable *array, uint32_t index, zend_string **key, zend_ulong *number, const zval **value)
{
	const zval *found;
	if (HT_IS_PACKED(array)) {
		found = array->arPacked + index;
		*key = NULL;
		*number = index;
	} else {
		const Bucket *bucket = array->arData + index;
		found = &bucket->val;
		*key = bucket->key;
		*number = bucket->h;
	}
	if (Z_TYPE_P(found) == IS_UNDEF) {
		return false;
	}
	ZVAL_DEREF(found);
	*value = found;
	return true;
}

bool opcov_read_bool(const zval *value, bool *flag)
{
	if (Z_TYPE_P(value) == IS_TRUE) {
		*flag = true;
		return true;
	}
	if (Z_TYPE_P(value) == IS_FALSE) {
		*flag = false;
		return true;
	}
	return false;
}

const char *opcov_type_name(const zval *value)
{
	return zend_zval_value_name(value);
}

const char *opcov_getcwd(char *buffer, size_t size)
{
	return VCWD_GETCWD(buffer, size);
}

const char *opcov_php_version(void)
{
	return php_version();
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

static zval *opcov_first_operand(const zend_execute_data *execute_data, const zend_op *op)
{
	if (op->op1_type == IS_CONST) {
		return RT_CONSTANT(op, op->op1);
	}
	if (op->op1_type & (IS_TMP_VAR | IS_VAR | IS_CV)) {
		return EX_VAR(op->op1.var);
	}
	return NULL;
}

static int opcov_truth(const zval *value)
{
	switch (Z_TYPE_P(value)) {
		case IS_NULL:
		case IS_FALSE:
			return 0;
		case IS_TRUE:
		case IS_RESOURCE:
			return 1;
		case IS_LONG:
			return Z_LVAL_P(value) != 0;
		case IS_DOUBLE:
			return Z_DVAL_P(value) ? 1 : 0;
		case IS_STRING:
			return !(Z_STRLEN_P(value) == 0 || (Z_STRLEN_P(value) == 1 && Z_STRVAL_P(value)[0] == '0'));
		case IS_ARRAY:
			return zend_hash_num_elements(Z_ARRVAL_P(value)) != 0;
		case IS_OBJECT:
			return Z_OBJ_HT_P(value)->cast_object == zend_std_cast_object_tostring ? 1 : -1;
		case IS_REFERENCE:
			return opcov_truth(Z_REFVAL_P(value));
		default:
			return -1;
	}
}

int opcov_branch_truth(const zend_execute_data *execute_data, const zend_op *op)
{
	const zval *value = opcov_first_operand(execute_data, op);
	return value ? opcov_truth(value) : -1;
}

int opcov_constant_truth(const zend_op *op)
{
	return op->op1_type == IS_CONST ? opcov_truth(RT_CONSTANT(op, op->op1)) : -1;
}

int opcov_branch_set(const zend_execute_data *execute_data, const zend_op *op)
{
	const zval *value = opcov_first_operand(execute_data, op);
	if (!value) {
		return -1;
	}
	ZVAL_DEREF(value);
	return Z_TYPE_P(value) > IS_NULL;
}

int opcov_branch_null(const zend_execute_data *execute_data, const zend_op *op)
{
	const zval *value = opcov_first_operand(execute_data, op);
	if (!value) {
		return -1;
	}
	if (op->op1_type == IS_CV && Z_TYPE_P(value) == IS_UNDEF && !(op->extended_value & ZEND_JMP_NULL_BP_VAR_IS)) {
		return -1;
	}
	ZVAL_DEREF(value);
	return Z_TYPE_P(value) <= IS_NULL;
}

int64_t opcov_match_target(const zend_execute_data *execute_data, const zend_op *op, const zend_op *opcodes)
{
	const zval *value = opcov_first_operand(execute_data, op);
	if (!value) {
		return -1;
	}
	ZVAL_DEREF(value);
	HashTable *table = Z_ARRVAL_P(RT_CONSTANT(op, op->op2));
	const zval *jump = NULL;
	if (Z_TYPE_P(value) == IS_LONG) {
		jump = zend_hash_index_find(table, Z_LVAL_P(value));
	} else if (Z_TYPE_P(value) == IS_STRING) {
		jump = zend_hash_find(table, Z_STR_P(value));
	} else if (Z_TYPE_P(value) == IS_UNDEF) {
		return -1;
	}
	zend_long offset = jump ? Z_LVAL_P(jump) : (zend_long) op->extended_value;
	const zend_op *target = ZEND_OFFSET_TO_OPLINE(op, offset);
	return target - opcodes;
}

int opcov_foreach_started(const zend_execute_data *execute_data, const zend_op *op)
{
	zval *iteration = EX_VAR(op->op1.var);
	zval *value = iteration;
	ZVAL_DEREF(value);
	if (Z_TYPE_P(value) == IS_ARRAY && op->opcode == ZEND_FE_FETCH_R) {
		return Z_FE_POS_P(iteration) > 0;
	}
	if (Z_TYPE_P(value) == IS_OBJECT) {
		zend_object_iterator *iterator = zend_iterator_unwrap(value);
		if (iterator) {
			return iterator->index != (zend_ulong) -1;
		}
	}
	uint32_t position = Z_FE_ITER_P(iteration);
	if (position == (uint32_t) -1 || position >= EG(ht_iterators_used)) {
		return -1;
	}
	return EG(ht_iterators)[position].pos > 0;
}

bool opcov_fast_call_threw(const zend_execute_data *execute_data, uint32_t var)
{
	return Z_OBJ_P(EX_VAR(var)) != NULL;
}

uint32_t opcov_fast_call_var(const zend_op_array *op_array, uint32_t finally_op)
{
	for (int index = 0; index < op_array->last_try_catch; index++) {
		const zend_try_catch_element *element = &op_array->try_catch_array[index];
		if (element->finally_op == finally_op && element->finally_end > 0) {
			return op_array->opcodes[element->finally_end].op1.var;
		}
	}
	return (uint32_t) -1;
}
