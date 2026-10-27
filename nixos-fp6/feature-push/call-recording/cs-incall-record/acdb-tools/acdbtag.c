/* Read-only ACDB queries for the in-call record / in-call music graphs
 * (companion to ../../../cs-call-audio/topology/acdb-tools/acdbq.c; same
 * build: compile with that directory's logstub.c against an AudioReach
 * graphservices checkout, see ../gen-incall-m4.py header).
 *
 * usage:
 *   acdbtag <file.acdb> <file.qwsp> tags  key=value ...
 *       ACDB_CMD_GET_TAGS_FROM_GKV: every module tag used by the usecase
 *       and the module instances carrying it
 *   acdbtag <file.acdb> <file.qwsp> tagdata <sgid> <tag> key=value ...
 *       ACDB_CMD_GET_MODULE_TAG_DATA: the SET_CFG params a client sends
 *       for <tag> with that tag key vector (e.g. TAG_STREAM_MUX_DEMUX
 *       0xC0000027 with 0xBD000000=4)
 *   acdbtag <file.acdb> <file.qwsp> cal <sgid> [key=value ...]
 *       ACDB_CMD_GET_SUBGRAPH_CALIBRATION_DATA_NONPERSIST with an
 *       (optional) calibration key vector
 * Output: text, one APM param per line (iid, pid, size, payload words).
 * Raw output is derived from proprietary data: keep it out of git. */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "acdb.h"

static int init(char **argv)
{
	AcdbDataFiles files = { 0 };
	int i;
	files.num_files = 2;
	for (i = 0; i < 2; i++) {
		strncpy(files.acdbFiles[i].fileName, argv[1 + i], MAX_FILENAME_LENGTH - 1);
		files.acdbFiles[i].fileNameLen = strlen(files.acdbFiles[i].fileName);
	}
	return acdb_init(&files, NULL);
}

static int kvs(int argc, char **argv, int from, AcdbKeyValuePair *kv)
{
	int n = 0;
	for (int i = from; i < argc && n < 32; i++)
		if (sscanf(argv[i], "%x=%x", &kv[n].key, &kv[n].value) == 2)
			n++;
	return n;
}

static void params(const uint8_t *b, uint32_t n)
{
	uint32_t o = 0;
	while (o + 12 <= n) {
		const uint32_t *w = (const uint32_t *)(b + o);
		uint32_t sz = w[2], hdr = 16;
		/* tag data / cal blobs: iid, pid, size, error, payload */
		printf("  iid=0x%x pid=0x%08x size=%u:", w[0], w[1], sz);
		for (uint32_t k = 0; k < (sz + 3) / 4 && k < 48; k++)
			printf(" 0x%x", w[4 + k]);
		printf("%s\n", (sz + 3) / 4 > 48 ? " ..." : "");
		o += hdr + ((sz + 7) & ~7u);
	}
}

int main(int argc, char **argv)
{
	AcdbKeyValuePair kv[32];
	int rc;
	if (argc < 4 || init(argv)) {
		fprintf(stderr, "usage: see source; acdb_init failed\n");
		return 1;
	}
	if (!strcmp(argv[3], "tags")) {
		AcdbGraphKeyVector gkv = { kvs(argc, argv, 4, kv), kv };
		AcdbCmdGetTagsFromGkvReq rq = { &gkv };
		AcdbCmdGetTagsFromGkvRsp rs = { 0 };
		rc = acdb_ioctl(ACDB_CMD_GET_TAGS_FROM_GKV, &rq, sizeof rq, &rs, sizeof rs);
		if (rc || !rs.list_size) { printf("tags rc=%d\n", rc); return 2; }
		rs.tag_module_list = calloc(1, rs.list_size);
		rc = acdb_ioctl(ACDB_CMD_GET_TAGS_FROM_GKV, &rq, sizeof rq, &rs, sizeof rs);
		printf("tags rc=%d n=%u\n", rc, rs.num_tags);
		uint8_t *p = (uint8_t *)rs.tag_module_list;
		for (uint32_t t = 0; t < rs.num_tags; t++) {
			AcdbTagModule *tm = (AcdbTagModule *)p;
			printf("TAG 0x%08x:", tm->tag_id);
			for (uint32_t m = 0; m < tm->num_modules; m++)
				printf(" 0x%08x/0x%x", tm->modules[m].module_id, tm->modules[m].module_iid);
			printf("\n");
			p += 8 + 8 * tm->num_modules;
		}
	} else if (!strcmp(argv[3], "tagdata") && argc >= 6) {
		uint32_t sg = strtoul(argv[4], 0, 16);
		AcdbSgIdModuleTag q = { 1, &sg, { (uint32_t)strtoul(argv[5], 0, 16),
				{ kvs(argc, argv, 6, kv), kv } } };
		AcdbBlob b = { 0, NULL };
		rc = acdb_ioctl(ACDB_CMD_GET_MODULE_TAG_DATA, &q, sizeof q, &b, sizeof b);
		if (rc || !b.buf_size) { printf("tagdata rc=%d size=%u\n", rc, b.buf_size); return 2; }
		b.buf = calloc(1, b.buf_size);
		rc = acdb_ioctl(ACDB_CMD_GET_MODULE_TAG_DATA, &q, sizeof q, &b, sizeof b);
		printf("tagdata rc=%d size=%u\n", rc, b.buf_size);
		params(b.buf, b.buf_size);
	} else if (!strcmp(argv[3], "cal") && argc >= 5) {
		uint32_t sg = strtoul(argv[4], 0, 16);
		int n = kvs(argc, argv, 5, kv);
		AcdbSgIdCalKeyVector q = { 1, &sg, { 0, NULL }, { n, kv } };
		AcdbBlob b = { 0, NULL };
		rc = acdb_ioctl(ACDB_CMD_GET_SUBGRAPH_CALIBRATION_DATA_NONPERSIST, &q, sizeof q, &b, sizeof b);
		if (rc || !b.buf_size) { printf("cal rc=%d size=%u\n", rc, b.buf_size); return 2; }
		b.buf = calloc(1, b.buf_size);
		rc = acdb_ioctl(ACDB_CMD_GET_SUBGRAPH_CALIBRATION_DATA_NONPERSIST, &q, sizeof q, &b, sizeof b);
		printf("cal rc=%d size=%u\n", rc, b.buf_size);
		params(b.buf, b.buf_size);
	} else {
		fprintf(stderr, "unknown command\n");
		return 1;
	}
	return 0;
}
