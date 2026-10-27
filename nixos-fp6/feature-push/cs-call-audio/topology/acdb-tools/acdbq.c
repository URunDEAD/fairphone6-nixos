/* Query an AudioReach ACDB for the graph of a key vector and dump each
 * subgraph's SPF (GRAPH_OPEN) blob and the inter-subgraph connection blob.
 * usage: acdbq <file.acdb> <file.qwsp> <outprefix> key=value ... */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "acdb.h"

static void dump(const char *p, const char *suf, void *b, uint32_t n)
{
	char fn[512]; snprintf(fn, sizeof fn, "%s%s", p, suf);
	FILE *f = fopen(fn, "wb"); fwrite(b, 1, n, f); fclose(f);
}

int main(int argc, char **argv)
{
	AcdbDataFiles files = { 0 };
	AcdbKeyValuePair kv[32]; int nkv = 0, i, rc;
	files.num_files = 2;
	for (i = 0; i < 2; i++) {
		strncpy(files.acdbFiles[i].fileName, argv[1 + i], MAX_FILENAME_LENGTH - 1);
		files.acdbFiles[i].fileNameLen = strlen(files.acdbFiles[i].fileName);
	}
	for (i = 4; i < argc; i++) {
		sscanf(argv[i], "%x=%x", &kv[nkv].key, &kv[nkv].value); nkv++;
	}
	rc = acdb_init(&files, NULL);
	if (rc) { fprintf(stderr, "acdb_init %d\n", rc); return 1; }

	AcdbGraphKeyVector gkv = { nkv, kv };
	AcdbGetGraphRsp gr = { 0 };
	rc = acdb_ioctl(ACDB_CMD_GET_GRAPH, &gkv, sizeof gkv, &gr, sizeof gr);
	if (rc || !gr.size) { printf("GET_GRAPH rc=%d size=%u\n", rc, gr.size); return 2; }
	gr.subgraphs = calloc(1, gr.size);
	rc = acdb_ioctl(ACDB_CMD_GET_GRAPH, &gkv, sizeof gkv, &gr, sizeof gr);
	printf("GET_GRAPH rc=%d num_sg=%u\n", rc, gr.num_subgraphs);
	uint32_t *sgids = calloc(gr.num_subgraphs, 4);
	uint8_t *p = (uint8_t *)gr.subgraphs;
	for (i = 0; i < (int)gr.num_subgraphs; i++) {
		AcdbSubgraph *s = (AcdbSubgraph *)p;
		sgids[i] = s->sg_id;
		printf("SG 0x%08x ->", s->sg_id);
		for (uint32_t j = 0; j < s->num_dst_sgids; j++) printf(" 0x%08x", s->dst_sg_ids[j]);
		printf("\n");
		p += 8 + 4 * s->num_dst_sgids;
	}
	dump(argv[3], ".graph.bin", gr.subgraphs, gr.size);

	AcdbSgIdGraphKeyVector sq = { gr.num_subgraphs, sgids, gkv };
	AcdbGetSubgraphDataRsp sd; memset(&sd, 0, sizeof sd);
	rc = acdb_ioctl(ACDB_CMD_GET_SUBGRAPH_DATA, &sq, sizeof sq, &sd, sizeof sd);
	printf("SG_DATA sizes rc=%d spf=%u drv=%u\n", rc, sd.spf_blob.buf_size, sd.driver_prop.size);
	sd.spf_blob.buf = calloc(1, sd.spf_blob.buf_size + 4);
	sd.driver_prop.sub_graph_prop_data = calloc(1, sd.driver_prop.size + 4);
	rc = acdb_ioctl(ACDB_CMD_GET_SUBGRAPH_DATA, &sq, sizeof sq, &sd, sizeof sd);
	printf("SG_DATA rc=%d spf=%u drv=%u num_sgid=%u\n", rc, sd.spf_blob.buf_size, sd.driver_prop.size, sd.driver_prop.num_sgid);
	dump(argv[3], ".spf.bin", sd.spf_blob.buf, sd.spf_blob.buf_size);
	dump(argv[3], ".drv.bin", sd.driver_prop.sub_graph_prop_data, sd.driver_prop.size);

	AcdbSubGraphList sl = { gr.num_subgraphs, gr.subgraphs };
	AcdbBlob cb = { 0, NULL };
	rc = acdb_ioctl(ACDB_CMD_GET_SUBGRAPH_CONNECTIONS, &sl, sizeof sl, &cb, sizeof cb);
	printf("SG_CONN sizes rc=%d size=%u\n", rc, cb.buf_size);
	if (!rc && cb.buf_size) {
		cb.buf = calloc(1, cb.buf_size);
		rc = acdb_ioctl(ACDB_CMD_GET_SUBGRAPH_CONNECTIONS, &sl, sizeof sl, &cb, sizeof cb);
		printf("SG_CONN rc=%d size=%u\n", rc, cb.buf_size);
		dump(argv[3], ".conn.bin", cb.buf, cb.buf_size);
	}
	return 0;
}
