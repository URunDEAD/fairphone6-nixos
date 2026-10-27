#!/bin/sh
# Build the ACDB query tools natively against AudioReach graphservices
# (https://github.com/AudioReach/audioreach-graphservices, BSD-3-Clause).
#   ./build.sh <graphservices-checkout> <outdir>
# Then, with the stock files copied (read-only) to
#   results/feature-push-20261024/cs-call-audio/stock/acdbdata/volcano_mtp_fps/:
#   A=.../MTP_acdb_cal.acdb W=.../MTP_workspaceFileXml.qwsp
#   gkvs $A $W B3000000                                   # all voice key vectors
#   acdbq $A $W out/rx A1000000=A1000010 A2000000=A2000004 AC000000=AC000005 AF000000=AF000001 B3000000=B3000001
#   acdbq $A $W out/tx A3000000=A3000004 AD000000=AD000008 B1000000=B1000007 B3000000=B3000001
#   python3 dump-graph.py out/rx.spf.bin out/rx.conn.bin      # human-readable
#   python3 ../acdb2m4.py out/rx out/tx > ../milos-voice-graph.m4
# Outputs stay out of git (derived from proprietary data); only the m4 is ours.
set -eu
G=$1; O=$2; mkdir -p "$O"
H=$(dirname "$0")
SRC="$G/acdb/src/*.c $G/ar_osal/src/linux/ar_osal_file_io.c $G/ar_osal/src/linux/ar_osal_heap.c \
 $G/ar_osal/src/linux/ar_osal_mem_op.c $G/ar_osal/src/linux/ar_osal_mutex.c $G/ar_osal/src/linux/ar_osal_string.c \
 $G/ar_osal/src/linux/ar_osal_sleep.c $G/ar_osal/src/linux/ar_osal_timer.c $G/ar_osal/src/linux/ar_osal_thread.c \
 $G/ar_osal/src/linux/ar_osal_signal.c $G/ar_osal/src/linux/ar_osal_stub_log_pkt_op.c"
FL="-O1 -w -Wno-error=incompatible-pointer-types -D__unused=__attribute__((unused)) -I$G/acdb/api -I$G/acdb/inc -I$G/ar_osal/api -lpthread"
for t in acdbq gkvs; do
  # shellcheck disable=SC2086
  ${CC:-cc} -o "$O/$t" "$H/$t.c" "$H/logstub.c" $SRC $FL
done
