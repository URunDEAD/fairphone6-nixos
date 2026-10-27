#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "acdb.h"
int main(int argc, char **argv)
{
	AcdbDataFiles files = { 0 }; int i, rc;
	files.num_files = 2;
	for (i = 0; i < 2; i++) { strncpy(files.acdbFiles[i].fileName, argv[1 + i], 255); files.acdbFiles[i].fileNameLen = strlen(argv[1 + i]); }
	if ((rc = acdb_init(&files, NULL))) return 1;
	uint32_t keys[64]; int n = 0;
	for (i = 3; i < argc; i++) keys[n++] = strtoul(argv[i], 0, 16);
	AcdbUintList kl = { n, keys };
	AcdbKeyVectorList r; memset(&r, 0, sizeof r);
	rc = acdb_ioctl(ACDB_CMD_GET_SUPPORTED_GKVS, &kl, sizeof kl, &r, sizeof r);
	fprintf(stderr, "rc=%d n=%u size=%u\n", rc, r.num_key_vectors, r.list_size);
	r.key_vector_list = calloc(1, r.list_size);
	rc = acdb_ioctl(ACDB_CMD_GET_SUPPORTED_GKVS, &kl, sizeof kl, &r, sizeof r);
	uint32_t *p = (uint32_t *)r.key_vector_list;
	for (uint32_t k = 0; k < r.num_key_vectors; k++) {
		uint32_t nk = *p++;
		for (uint32_t j = 0; j < nk; j++, p += 2) printf("%08X=%08X ", p[0], p[1]);
		printf("\n");
	}
	return rc;
}
