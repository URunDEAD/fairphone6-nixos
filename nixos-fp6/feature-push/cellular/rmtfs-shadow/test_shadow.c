/*
 * Tests for the rmtfs -r -D persistent shadow (storage.c + shadow.c) with
 * fake partition files. Built and run in the rmtfs-fp6-shadow checkPhase:
 *   cc -I. -o test_shadow test_shadow.c storage.c shadow.c && ./test_shadow
 */
#include <sys/stat.h>
#include <sys/types.h>
#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <limits.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <unistd.h>
#include "rmtfs.h"
#include "shadow.h"

#define PART_LEN (256 * 1024)

static int failures;
static char root[PATH_MAX];
static char parts[PATH_MAX];
static char shadow[PATH_MAX];

#define CHECK(cond, ...) do { \
	if (!(cond)) { \
		failures++; \
		fprintf(stderr, "FAIL %s:%d: ", __FILE__, __LINE__); \
		fprintf(stderr, __VA_ARGS__); \
		fprintf(stderr, "\n"); \
	} \
} while (0)

static void pathf(char *out, const char *fmt, const char *a, const char *b)
{
	snprintf(out, PATH_MAX, fmt, a, b);
}

static void write_file(const char *path, const void *buf, size_t len, mode_t mode)
{
	int fd;

	chmod(path, 0600);
	fd = open(path, O_WRONLY | O_CREAT | O_TRUNC, 0600);
	if (fd < 0 || write(fd, buf, len) != (ssize_t)len) {
		perror(path);
		exit(2);
	}
	close(fd);
	chmod(path, mode);
}

static void *read_file(const char *path, size_t *len)
{
	struct stat st;
	void *buf;
	int fd;

	fd = open(path, O_RDONLY);
	if (fd < 0)
		return NULL;
	if (fstat(fd, &st) || st.st_size < 0 || st.st_size > 64 * 1024 * 1024) {
		close(fd);
		return NULL;
	}
	buf = malloc((size_t)st.st_size + 1);
	if (!buf || read(fd, buf, (size_t)st.st_size) != st.st_size) {
		close(fd);
		free(buf);
		return NULL;
	}
	close(fd);
	*len = st.st_size;
	return buf;
}

static int exists(const char *path)
{
	struct stat st;

	return lstat(path, &st) == 0;
}

static struct rmtfd *start(void)
{
	CHECK(storage_set_persist_dir(shadow) == 0, "set_persist_dir");
	CHECK(storage_init(parts, true, true) == 0, "storage_init");
	return storage_open(0, "/boot/modem_fs1", "");
}

static void read_all(struct rmtfd *fd, uint8_t *out)
{
	size_t off;

	for (off = 0; off < PART_LEN; off += SECTOR_SIZE)
		storage_pread(fd, out + off, SECTOR_SIZE, off);
}

static void test_sha256(void)
{
	static const uint8_t abc[32] = {
		0xba, 0x78, 0x16, 0xbf, 0x8f, 0x01, 0xcf, 0xea, 0x41, 0x41, 0x40, 0xde, 0x5d, 0xae, 0x22, 0x23,
		0xb0, 0x03, 0x61, 0xa3, 0x96, 0x17, 0x7a, 0x9c, 0xb4, 0x10, 0xff, 0x61, 0xf2, 0x00, 0x15, 0xad,
	};
	static const uint8_t empty[32] = {
		0xe3, 0xb0, 0xc4, 0x42, 0x98, 0xfc, 0x1c, 0x14, 0x9a, 0xfb, 0xf4, 0xc8, 0x99, 0x6f, 0xb9, 0x24,
		0x27, 0xae, 0x41, 0xe4, 0x64, 0x9b, 0x93, 0x4c, 0xa4, 0x95, 0x99, 0x1b, 0x78, 0x52, 0xb8, 0x55,
	};
	/* sha256 of 1,000,000 x 'a' */
	static const uint8_t mil[32] = {
		0xcd, 0xc7, 0x6e, 0x5c, 0x99, 0x14, 0xfb, 0x92, 0x81, 0xa1, 0xc7, 0xe2, 0x84, 0xd7, 0x3e, 0x67,
		0xf1, 0x80, 0x9a, 0x48, 0xa4, 0x97, 0x20, 0x0e, 0x04, 0x6d, 0x39, 0xcc, 0xc7, 0x11, 0x2c, 0xd0,
	};
	uint8_t out[32];
	char *a = malloc(1000000);

	shadow_sha256("abc", 3, out);
	CHECK(!memcmp(out, abc, 32), "sha256(abc)");
	shadow_sha256("", 0, out);
	CHECK(!memcmp(out, empty, 32), "sha256(empty)");
	memset(a, 'a', 1000000);
	shadow_sha256(a, 1000000, out);
	CHECK(!memcmp(out, mil, 32), "sha256(1M a)");
	free(a);
}

int main(void)
{
	char part[PATH_MAX], img[PATH_MAX], orig[PATH_MAX], tmp[PATH_MAX], reset[PATH_MAX];
	static uint8_t src[PART_LEN], buf[PART_LEN], expect[PART_LEN];
	uint8_t sector[SECTOR_SIZE];
	struct stat st0, st1;
	struct rmtfd *fd;
	size_t len;
	void *data;
	ino_t ino;
	int i;

	test_sha256();

	snprintf(root, sizeof(root), "%s/rmtfs-shadow-test-XXXXXX", getenv("TMPDIR") ? getenv("TMPDIR") : "/tmp");
	if (!mkdtemp(root)) {
		perror("mkdtemp");
		return 2;
	}
	pathf(parts, "%s/%s", root, "parts");
	pathf(shadow, "%s/%s", root, "shadow");
	mkdir(parts, 0700);
	mkdir(shadow, 0700);
	pathf(part, "%s/%s", parts, "modemst1");
	pathf(img, "%s/%s", shadow, "modemst1.img");
	pathf(orig, "%s/%s", shadow, "original/modemst1.img");
	pathf(tmp, "%s/%s", shadow, "modemst1.img.tmp");
	pathf(reset, "%s/%s", shadow, "RESET");

	srand(42);
	for (i = 0; i < PART_LEN; i++)
		src[i] = rand();
	/* read-only "block device": any write attempt would fail */
	write_file(part, src, PART_LEN, 0444);
	stat(part, &st0);

	/* -D without -r is refused */
	CHECK(storage_set_persist_dir(shadow) == 0, "set dir");
	CHECK(storage_init(parts, false, true) != 0, "-D without -r must be refused");
	CHECK(storage_set_persist_dir("/dev/foo") != 0, "dir under /dev refused");
	CHECK(storage_set_persist_dir("relative") != 0, "relative dir refused");

	/* 1. first start: partition served, original captured, no image yet */
	fd = start();
	CHECK(fd != NULL, "open modemst1");
	read_all(fd, buf);
	CHECK(!memcmp(buf, src, PART_LEN), "first start serves the partition");
	CHECK(exists(orig), "original backup created");
	stat(orig, &st1);
	CHECK((st1.st_mode & 0777) == 0400, "original is 0400 (got %o)", st1.st_mode & 0777);
	CHECK(!exists(img), "no image before any write");
	CHECK(storage_next_flush_ms() == -1, "nothing to flush");

	/* 2. a write is debounced, then flushed */
	memset(sector, 0xA5, sizeof(sector));
	CHECK(storage_pwrite(fd, sector, SECTOR_SIZE, 4 * SECTOR_SIZE) == SECTOR_SIZE, "pwrite");
	i = storage_next_flush_ms();
	CHECK(i > 0 && i <= 2000, "flush scheduled within debounce (%d)", i);
	storage_flush(false);
	CHECK(!exists(img), "not flushed before the debounce expires");
	{
		struct timespec ts = { 2, 100000000 };
		nanosleep(&ts, NULL);
	}
	storage_flush(false);
	CHECK(exists(img), "flushed after the debounce");
	CHECK(!exists(tmp), "no tmp file left");
	stat(img, &st1);
	CHECK((st1.st_mode & 0777) == 0600, "image is 0600");
	CHECK(st1.st_size == SHADOW_HDR_LEN + PART_LEN, "image size");
	CHECK(storage_next_flush_ms() == -1, "clean after flush");
	storage_exit();

	memcpy(expect, src, PART_LEN);
	memset(expect + 4 * SECTOR_SIZE, 0xA5, SECTOR_SIZE);

	/* 3. next start loads the saved shadow */
	fd = start();
	read_all(fd, buf);
	CHECK(!memcmp(buf, expect, PART_LEN), "second start serves the saved shadow");

	/* 4. a write is saved at exit (clean shutdown), atomically replaced */
	ino = st1.st_ino;
	memset(sector, 0x5A, sizeof(sector));
	storage_pwrite(fd, sector, SECTOR_SIZE, 9 * SECTOR_SIZE);
	storage_exit();
	stat(img, &st1);
	CHECK(st1.st_ino != ino, "image replaced by rename (new inode)");
	memset(expect + 9 * SECTOR_SIZE, 0x5A, SECTOR_SIZE);
	fd = start();
	read_all(fd, buf);
	CHECK(!memcmp(buf, expect, PART_LEN), "exit flush persisted");
	storage_exit();

	/* 5. the real partition and the original backup are never modified */
	stat(part, &st1);
	CHECK(st1.st_mtime == st0.st_mtime && st1.st_size == st0.st_size, "partition untouched (meta)");
	data = read_file(part, &len);
	CHECK(data && len == PART_LEN && !memcmp(data, src, PART_LEN), "partition untouched (data)");
	free(data);
	data = read_file(orig, &len);
	CHECK(data && len == SHADOW_HDR_LEN + PART_LEN && !memcmp((char *)data + SHADOW_HDR_LEN, src, PART_LEN),
	      "original holds the first-capture partition");
	free(data);

	/* 6. a stale tmp file does not matter */
	write_file(tmp, "garbage", 7, 0600);
	fd = start();
	read_all(fd, buf);
	CHECK(!memcmp(buf, expect, PART_LEN), "stale tmp ignored on load");
	storage_pwrite(fd, sector, SECTOR_SIZE, 9 * SECTOR_SIZE);
	storage_flush(true);
	CHECK(!exists(tmp), "tmp replaced and renamed");
	storage_exit();

	/* 7. corrupt image -> partition */
	data = read_file(img, &len);
	((uint8_t *)data)[SHADOW_HDR_LEN + 100] ^= 1;
	write_file(img, data, len, 0600);
	free(data);
	fd = start();
	read_all(fd, buf);
	CHECK(!memcmp(buf, src, PART_LEN), "corrupt image ignored");
	storage_exit();

	/* 8. truncated image -> partition */
	{
		uint8_t *good;

		fd = start();
		storage_pwrite(fd, sector, SECTOR_SIZE, 0);
		storage_flush(true);
		storage_exit();
		good = read_file(img, &len);
		write_file(img, good, len - SECTOR_SIZE, 0600);
		fd = start();
		read_all(fd, buf);
		CHECK(!memcmp(buf, src, PART_LEN), "truncated image ignored");
		storage_exit();
		write_file(img, good, len, 0600);
		free(good);
	}

	/* 9. the source partition changed (e.g. Android reflashed it) -> partition */
	src[7] ^= 0xff;
	write_file(part, src, PART_LEN, 0444);
	fd = start();
	read_all(fd, buf);
	CHECK(!memcmp(buf, src, PART_LEN), "image for another source ignored");
	data = read_file(orig, &len);
	CHECK(data && ((uint8_t *)data)[SHADOW_HDR_LEN + 7] == (uint8_t)(src[7] ^ 0xff),
	      "original backup is never replaced");
	free(data);
	storage_exit();

	/* 10. partition size changed -> partition */
	write_file(part, src, PART_LEN / 2, 0444);
	fd = start();
	read_all(fd, buf);
	CHECK(!memcmp(buf, src, PART_LEN / 2), "image for another size ignored");
	storage_exit();
	write_file(part, src, PART_LEN, 0444);

	/* 11. RESET stops saving */
	unlink(img);
	write_file(reset, "", 0, 0600);
	fd = start();
	storage_pwrite(fd, sector, SECTOR_SIZE, 0);
	storage_flush(true);
	storage_exit();
	CHECK(!exists(img), "RESET: nothing saved");
	unlink(reset);

	/* 12. close by the modem flushes; a grown shadow is not saved */
	fd = start();
	storage_pwrite(fd, sector, SECTOR_SIZE, 0);
	storage_close(fd);
	CHECK(exists(img), "close flushes");
	storage_exit();
	unlink(img);
	fd = start();
	storage_pwrite(fd, sector, SECTOR_SIZE, PART_LEN);
	storage_flush(true);
	CHECK(!exists(img), "grown shadow not saved");
	storage_exit();

	/* 13. without -D nothing is written anywhere */
	storage_set_persist_dir(NULL);
	storage_init(parts, true, true);
	fd = storage_open(0, "/boot/modem_fs1", "");
	storage_pwrite(fd, sector, SECTOR_SIZE, 0);
	storage_flush(true);
	storage_exit();
	CHECK(!exists(img), "plain -r writes nothing");

	if (failures) {
		fprintf(stderr, "%d failure(s); left %s\n", failures, root);
		return 1;
	}
	printf("rmtfs shadow tests: all passed\n");
	return 0;
}
