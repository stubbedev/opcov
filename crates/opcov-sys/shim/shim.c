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

bool opcov_is_user_code(const zend_function *function)
{
	return ZEND_USER_CODE(function->type);
}
