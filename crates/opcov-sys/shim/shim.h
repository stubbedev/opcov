#ifndef OPCOV_SHIM_H
#define OPCOV_SHIM_H

#include "php.h"
#include "zend_arena.h"

extern const char opcov_module_build_id[];

zend_executor_globals *opcov_executor_globals(void);
zend_compiler_globals *opcov_compiler_globals(void);
zend_arena *opcov_arena_create(size_t size);
void opcov_arena_destroy(zend_arena *arena);
void opcov_zval_stringl(zval *target, const char *str, size_t len);
const void *opcov_op_handler(const zend_op *op);
bool opcov_parse_parameters_none(zend_execute_data *execute_data);
bool opcov_is_user_code(const zend_function *function);

#endif
