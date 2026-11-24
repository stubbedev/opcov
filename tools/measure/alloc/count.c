#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <pthread.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

extern void *__libc_malloc(size_t size);
extern void *__libc_calloc(size_t count, size_t size);
extern void *__libc_realloc(void *pointer, size_t size);
extern void *__libc_memalign(size_t alignment, size_t size);
extern void __libc_free(void *pointer);

enum counter { MALLOC, CALLOC, REALLOC, ALIGNED, FREE, BYTES, COUNTERS };

static unsigned long long counters[COUNTERS];
static char prefix[4000];

static void count(enum counter which, unsigned long long bytes)
{
	__atomic_fetch_add(&counters[which], 1, __ATOMIC_RELAXED);
	__atomic_fetch_add(&counters[BYTES], bytes, __ATOMIC_RELAXED);
}

static void forked(void)
{
	for (int i = 0; i < COUNTERS; i++) {
		__atomic_store_n(&counters[i], 0, __ATOMIC_RELAXED);
	}
}

__attribute__((constructor)) static void start(void)
{
	const char *path = getenv("OPCOV_MEASURE_ALLOC_OUTPUT");
	if (path == NULL || strlen(path) >= sizeof(prefix)) {
		return;
	}
	strcpy(prefix, path);
	pthread_atfork(NULL, NULL, forked);
}

__attribute__((destructor)) static void finish(void)
{
	if (prefix[0] == '\0') {
		return;
	}
	char output[4096];
	snprintf(output, sizeof(output), "%s.%ld", prefix, (long) getpid());
	char line[512];
	int length = snprintf(line, sizeof(line),
		"{\"malloc\": %llu, \"calloc\": %llu, \"realloc\": %llu, \"aligned\": %llu, \"free\": %llu, \"bytes\": %llu}\n",
		__atomic_load_n(&counters[MALLOC], __ATOMIC_RELAXED),
		__atomic_load_n(&counters[CALLOC], __ATOMIC_RELAXED),
		__atomic_load_n(&counters[REALLOC], __ATOMIC_RELAXED),
		__atomic_load_n(&counters[ALIGNED], __ATOMIC_RELAXED),
		__atomic_load_n(&counters[FREE], __ATOMIC_RELAXED),
		__atomic_load_n(&counters[BYTES], __ATOMIC_RELAXED));
	int fd = open(output, O_WRONLY | O_CREAT | O_TRUNC | O_CLOEXEC, 0644);
	if (fd < 0) {
		return;
	}
	ssize_t written = write(fd, line, (size_t) length);
	close(fd);
	if (written != length) {
		unlink(output);
	}
}

void *malloc(size_t size)
{
	count(MALLOC, size);
	return __libc_malloc(size);
}

void *calloc(size_t count_, size_t size)
{
	count(CALLOC, (unsigned long long) count_ * size);
	return __libc_calloc(count_, size);
}

void *realloc(void *pointer, size_t size)
{
	count(REALLOC, size);
	return __libc_realloc(pointer, size);
}

void free(void *pointer)
{
	if (pointer != NULL) {
		__atomic_fetch_add(&counters[FREE], 1, __ATOMIC_RELAXED);
	}
	__libc_free(pointer);
}

void *memalign(size_t alignment, size_t size)
{
	count(ALIGNED, size);
	return __libc_memalign(alignment, size);
}

void *aligned_alloc(size_t alignment, size_t size)
{
	count(ALIGNED, size);
	return __libc_memalign(alignment, size);
}

int posix_memalign(void **result, size_t alignment, size_t size)
{
	if (alignment % sizeof(void *) != 0 || (alignment & (alignment - 1)) != 0) {
		return EINVAL;
	}
	count(ALIGNED, size);
	void *pointer = __libc_memalign(alignment, size);
	if (pointer == NULL) {
		return ENOMEM;
	}
	*result = pointer;
	return 0;
}

void *valloc(size_t size)
{
	count(ALIGNED, size);
	return __libc_memalign((size_t) sysconf(_SC_PAGESIZE), size);
}
