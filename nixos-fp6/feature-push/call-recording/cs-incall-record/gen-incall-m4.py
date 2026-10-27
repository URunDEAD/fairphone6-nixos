#!/usr/bin/env python3
"""Turn the stock FP6 ACDB in-call record (and in-call music) graphs into
cs-incall-record.m4, an add-on for cs-call-audio's voice topology.

Off by default: nothing uses the fragment until it is appended to
cs-call-audio's FP6_VOICE_GRAPH description (see voice-graph-with-incall.nix
and NOTES.txt).

Inputs (all derived from the stock ACDB; keep them out of git):
  <rec>.spf.bin      acdbq dump, StreamTX INCALL_RECORD   B1000000=B100000C
  <music>.spf.bin    acdbq dump, StreamRX INCALL_MUSIC    A1000000=A1000013
  <tagdir>/rec.mux   acdbtag tagdata b00000d0 C0000027 BD000000=1..4, in one
                     file, each block headed by "== mux <n>"
  <tagdir>/rec.cal   acdbtag cal b00000d0
  <tagdir>/music.cal acdbtag cal b00000d6
Commands (stock files in results/feature-push-20261024/cs-call-audio/stock/):
  sh ../../cs-call-audio/topology/acdb-tools/build.sh <graphservices> <bin>
  cc -o <bin>/acdbtag acdb-tools/acdbtag.c ../../cs-call-audio/topology/acdb-tools/logstub.c \
     <same sources and flags as build.sh>
  acdbq   $A $W out/rec   B1000000=B100000C
  acdbq   $A $W out/music A1000000=A1000013
  for v in 1 2 3 4; do echo "== mux $v"; acdbtag $A $W tagdata b00000d0 C0000027 BD000000=$v; done > out/rec.mux
  acdbtag $A $W cal b00000d0 > out/rec.cal
  acdbtag $A $W cal b00000d6 > out/music.cal
  python3 gen-incall-m4.py out/rec out/music out > cs-incall-record.m4

Only numeric structure goes into the m4 (module ids, instance ids, ports,
connections, the control link, the mux/demux routing table, media formats
and log codes), the same policy as cs-call-audio's acdb2m4.py.

What the stock graph is (ACDB, SG 0xb00000d0, one subgraph, 11 modules):
  TX voice SG 0xb000003f  splitter 0x4560 out 3 -> RT_PROXY_TX 0x429a -> MFC 0x4081 -> MUX_DEMUX 0x4557 in 2
  RX voice SG 0xb0000041  splitter 0x4561 out 3 ->                        MFC 0x4085 -> MUX_DEMUX 0x4557 in 4
  MUX_DEMUX -> RAT 0x4654 -> MFC 0x4086 -> LOG -> PCM_CNV -> PCM_ENC -> LOG -> RD_SHARED_MEM_EP (host)
  control link RAT 0x4654 <-> TX mailbox 0x41b0 (INTENT_ID_TIMER_DRIFT_INFO)
The two splitter -> record connections and the control link belong to the
record graph's GRAPH_OPEN (that is how the stock graph manager opens it
while a call runs); here they ride in the GRAPH_OPEN extra params (kernel
candidate 0008). The record direction is the MUX_DEMUX config that PAL
sends as tag data (TAG_STREAM_MUX_DEMUX 0xC0000027, key 0xBD000000):
here it is a SET_CFG blob (kernel candidate 0006), chosen at build time
with FP6_INCALL_RECORD_DIR (1 UL, 2 DL, 3 UL+DL mono, 4 UL+DL stereo).

In-call music (ACDB SG 0xb00000d6, 10 modules) ends at the TX voice SG's
MFC 0x458b -> EC_SYNC 0x458c in 4 (mixed into the uplink by SAL 0x458d).
cs-call-audio dropped 0x458b from their TX graph; here it is the last
module of the music subgraph and the connection to 0x458c:4 rides in the
music GRAPH_OPEN.

usage: gen-incall-m4.py <rec-prefix> <music-prefix> <tagdir> > cs-incall-record.m4
       gen-incall-m4.py --check <milos-voice-graph.m4> [<0001 topology patch>]
         (checks that cs-call-audio's voice graph still has what this
         fragment attaches to; run after they regenerate)
"""
import re
import struct
import sys

P_SG, P_CONT, P_ML, P_MP, P_CONN, P_CTRL = 0x08001001, 0x08001000, 0x08001002, 0x08001003, 0x08001004, 0x08001061
PROP_PORT_INFO = 0x08001015
SG_PERF, SG_DIR, SG_SCEN = 0x0800100E, 0x0800100F, 0x08001010
C_TYPE, C_POS, C_STACK, C_DOMAIN = 0x08001011, 0x08001012, 0x08001013, 0x08001014
APM_MODULE_IID = 0x1

MID_WR_SHMEM, MID_RD_SHMEM = 0x07001000, 0x07001001
MID_PCM_CNV, MID_PCM_ENC, MID_PCM_DEC = 0x07001003, 0x07001004, 0x07001005
MID_LOG, MID_MFC = 0x0700101A, 0x07001015
MID_MUX_DEMUX, MID_RAT = 0x07001098, 0x07001041

PID_LOG_CFG = 0x08001031           # logging: log code, tap point, mode
PID_MFC_OUT_MF = 0x08001024        # PARAM_ID_MFC_OUTPUT_MEDIA_FORMAT
PID_RT_PROXY_CONFIG = 0x08001157
PID_MUX_DEMUX_CONFIG = 0x080011BD
PID_MUX_DEMUX_OUT_FORMAT = 0x080011BE
PID_RAT_MEDIA_FORMAT = 0x080010D0
PCM_INTERLEAVED = 1

# The voice-graph modules the in-call graphs attach to (instance ids are the
# same in cs-call-audio's milos-voice-graph.m4, checked by --check).
VOICE_TX_SPLITTER = 0x4560   # SG 0xb000003f
VOICE_RX_SPLITTER = 0x4561   # SG 0xb0000041 (merged into 0xb0000040 there)
VOICE_TX_MAILBOX = 0x41b0
VOICE_TX_EC_SYNC = 0x458c
MUSIC_LAST_MFC = 0x458b      # stock: in the TX voice SG; here: in the music SG

# cs-call-audio's TX voice capture mixer widget (fed by TX_CODEC_DMA_TX_3);
# the record FE hangs off it in DAPM only (renamed there on 2026-10-27).
VOICE_TX_MIXER_WIDGET = 'VoiceMMode1 Mixer'

RECORD_DAI_ID = 2            # FRONTEND_DAI_MULTIMEDIA3 -> "MultiMedia3 Capture"
MUSIC_DAI_ID = 3             # FRONTEND_DAI_MULTIMEDIA4 -> "MultiMedia4 Playback"


def words(b, o, n):
    return list(struct.unpack_from('<%dI' % n, b, o))


def props(b, o, n):
    out = {}
    for _ in range(n):
        pid, psz = words(b, o, 2)
        out[pid] = words(b, o + 8, psz // 4)
        o += 8 + psz
    return out, o


class Graph:
    def __init__(self, path):
        self.sgs, self.conts, self.mods, self.order = {}, {}, {}, []
        self.ports, self.conns, self.ctrls = {}, [], []
        b = open(path, 'rb').read()
        o = 0
        while o + 16 <= len(b):
            _iid, pid, psz, _err = words(b, o, 4)
            p = b[o + 16:o + 16 + psz]
            if pid in (P_SG, P_CONT):
                q, tbl = 4, self.sgs if pid == P_SG else self.conts
                for _ in range(words(p, 0, 1)[0]):
                    k, n = words(p, q, 2)
                    tbl[k], q = props(p, q + 8, n)
            elif pid == P_ML:
                q = 4
                for _ in range(words(p, 0, 1)[0]):
                    sg, c, n = words(p, q, 3)
                    q += 12
                    for _ in range(n):
                        mid, i = words(p, q, 2)
                        q += 8
                        self.mods[i] = (sg, c, mid)
                        self.order.append(i)
            elif pid == P_MP:
                q = 4
                for _ in range(words(p, 0, 1)[0]):
                    i, n = words(p, q, 2)
                    pr, q = props(p, q + 8, n)
                    if PROP_PORT_INFO in pr:
                        self.ports[i] = tuple(pr[PROP_PORT_INFO][:2])
            elif pid == P_CONN:
                q = 4
                for _ in range(words(p, 0, 1)[0]):
                    self.conns.append(tuple(words(p, q, 4)))
                    q += 16
            elif pid == P_CTRL:
                q = 4
                for _ in range(words(p, 0, 1)[0]):
                    a, ap, c, cp, n = words(p, q, 5)
                    pr, q = props(p, q + 20, n)
                    it = pr.get(0x08001062, [0])
                    for x in it[1:1 + it[0]]:
                        self.ctrls.append((a, ap, c, cp, x))
            o += 16 + ((psz + 7) & ~7)


PARAM_RE = re.compile(r'iid=0x([0-9a-f]+) pid=0x([0-9a-f]+) size=(\d+):(.*)')


def read_params(path):
    """acdbtag text -> {block: [(iid, pid, size, [words])]}; block = the
    number after '== mux' or 0."""
    out, cur = {}, 0
    for line in open(path):
        if line.startswith('== mux'):
            cur = int(line.split()[2])
            continue
        m = PARAM_RE.search(line)
        if m:
            w = [int(x, 16) for x in m.group(4).split()]
            out.setdefault(cur, []).append((int(m.group(1), 16), int(m.group(2), 16), int(m.group(3)), w))
    return out


def param_words(iid, pid, size, w):
    """One APM param entry as m4 words (iid, pid, size, 0, payload, pad to 8)."""
    n = (size + 3) // 4
    w = (w + [0] * n)[:n]
    if size % 4:  # keep only the valid low bytes of the last word
        w[-1] &= (1 << (8 * (size % 4))) - 1
    pad = [0] if n % 2 else []
    return ['%#x' % iid, '%#010x' % pid, str(size), '0'] + ['%#x' % x for x in w + pad]


def conn_words(conns):
    payload = [len(conns)] + [x for c in conns for x in c]
    return param_words(APM_MODULE_IID, P_CONN, 4 * len(payload), payload)


def ctrl_words(a, ap, c, cp, it):
    return ['AR_CTRL_LINK_WORDS(%#x, %#x, %#x, %#x, %#x)' % (a, ap, c, cp, it)]


def topo_order(keep, conns):
    indeg = {i: 0 for i in keep}
    for _s, _, d, _ in conns:
        indeg[d] += 1
    out, ready = [], [i for i in keep if indeg[i] == 0]
    while ready:
        i = ready.pop(0)
        out.append(i)
        for s, _, d, _ in conns:
            if s == i:
                indeg[d] -= 1
                if indeg[d] == 0:
                    ready.append(d)
    assert len(out) == len(keep), 'cycle'
    return out


def rat_mf(channels):
    # param_id_rat_mf_t: rate, bits 16 | q15 << 16, FIXED_POINT, channels,
    # u16 channel map (L, R / C), as PAL's payloadRATConfig sends it.
    cmap = [0x00020001] if channels == 2 else [0x3]
    return param_words(0, PID_RAT_MEDIA_FORMAT, 16 + 2 * channels, [48000, 0x000f0010, 1, channels] + cmap)


def emit_sg(out, g, sgid, prefix, dai, fe_stream, fe_type, extra_mods, cross, cfg, opn, tuples):
    """One FE subgraph: SectionPCM via AR_PCM_*, sub graph, containers,
    modules (ICR_MODULE), DAPM lines inside the subgraph."""
    keep = [i for i in g.order if g.mods[i][0] == sgid] + extra_mods
    keepset = set(keep)
    conns = [c for c in g.conns if c[0] in keepset and c[2] in keepset]
    order = topo_order(keep, conns)
    conts = []
    for i in order:
        if g.mods[i][1] not in conts:
            conts.append(g.mods[i][1])
    cidx = {c: n + 1 for n, c in enumerate(conts)}
    sp = g.sgs[sgid]
    w = out.write
    w("define(`NAME_PREFIX', `%s')dnl\n" % prefix)
    w('AR_SUBGRAPH(1, %d, %#010x, %#x, %#x, %#x)\n' % (dai, sgid, sp[SG_PERF][0], sp[SG_DIR][0], sp[SG_SCEN][0]))
    for c in conts:
        cp = g.conts[c]
        w('AR_CONTAINER(%d, %#010x, %#010x, %#x, %#x, %#x)\n' % (
            cidx[c], c, cp[C_TYPE][1], cp[C_STACK][0], cp[C_POS][0], cp[C_DOMAIN][0]))
    for i in order:
        _sg, c, mid = g.mods[i]
        mi, mo = g.ports.get(i, (1, 1))
        outs = [x for x in conns if x[0] == i]
        assert len(outs) <= 1, ('more than one output in an in-call graph', hex(i))
        sport, dport, dst = (outs[0][1], outs[0][3], outs[0][2]) if outs else (1, 2, 0)
        wtype, stream = ('buffer', '')
        if mid in (MID_RD_SHMEM, MID_WR_SHMEM):
            wtype, stream = fe_type, fe_stream
        w("ICR_MODULE(`m%x', 1, %d, %#x, %#010x, %d, %d, %d, %d, %#x, `%s', `%s', `%s', `%s', `%s')\n" % (
            i, cidx[c], i, mid, mi, mo, sport, dport, dst, wtype, stream,
            cfg.get(i, ''), opn.get(i, ''), tuples.get(i, '')))
    lines = []
    for s, _, d, _ in conns:
        lines.append('"%s.m%x, , %s.m%x"' % (prefix, d, prefix, s))
    return order, lines, conns


def main():
    rec = Graph(sys.argv[1] + '.spf.bin')
    music = Graph(sys.argv[2] + '.spf.bin')
    tagdir = sys.argv[3]
    mux = read_params(tagdir + '/rec.mux')
    rcal = read_params(tagdir + '/rec.cal').get(0, [])
    mcal = read_params(tagdir + '/music.cal').get(0, [])

    def cal_of(cal, iid, pid):
        for i, p, s, w in cal:
            if i == iid and p == pid:
                return s, w
        return None

    def log_tuples(cal, i, mid):
        t = []
        if mid == MID_LOG:
            lc = cal_of(cal, i, PID_LOG_CFG)
            code, tap, mode = (lc[1] + [0, 0, 0])[:3] if lc else (0x19af, 1, 0)
            t += ['AR_TKN_U32_MODULE_LOG_CODE "%#x"' % code,
                  'AR_TKN_U32_MODULE_LOG_TAP_POINT_ID "%d"' % (tap or 1),
                  'AR_TKN_U32_MODULE_LOG_MODE "%d"' % mode]
        if mid in (MID_PCM_CNV, MID_PCM_ENC, MID_PCM_DEC):
            t += ['AR_TKN_U32_MODULE_FMT_INTERLEAVE "%d"' % PCM_INTERLEAVED]
        return ' '.join(t)

    out = sys.stdout
    w = out.write
    w('dnl In-call record (and optional in-call music) front ends for the FP6\n'
      'dnl voice graph. Generated by gen-incall-m4.py from the stock ACDB\n'
      'dnl (acdbdata/volcano_mtp_fps: StreamTX INCALL_RECORD B1000000=B100000C,\n'
      'dnl StreamRX INCALL_MUSIC A1000000=A1000013). Do not edit by hand.\n'
      'dnl Append to cs-call-audio milos-voice-graph.m4 (FP6_VOICE_GRAPH); needs\n'
      'dnl kernel candidates 0006 + 0008. Options (define before this file):\n'
      'dnl   FP6_INCALL_RECORD_DIR  1 UL, 2 DL, 3 UL+DL mono, 4 UL+DL stereo (default 4)\n'
      'dnl   FP6_INCALL_MUSIC       also add the in-call music (uplink inject) FE\n')
    w("include(`audioreach/audioreach.m4')dnl\n"
      "include(`audioreach/tokens.m4')dnl\n"
      "include(`util/pcm.m4')dnl\n"
      "include(`audioreach/subgraph.m4')dnl\n"
      "include(`audioreach/container.m4')dnl\n"
      "include(`audioreach/module_generic.m4')dnl\n"
      "ifdef(`FP6_INCALL_RECORD_DIR', `', `define(`FP6_INCALL_RECORD_DIR', `4')')dnl\n")
    # Own module macro: AR_MODULE_GENERIC plus extra tuples (log code,
    # interleave), so known modules get the tokens the kernel expects.
    w("dnl ICR_MODULE(name, sgidx, container-idx, iid, module-id, max-ip, max-op,\n"
      "dnl            src-port, dst-port, dst-iid, widget-type, stream-name,\n"
      "dnl            cfg-data-name, open-data-name, extra tuples)\n"
      "define(`ICR_MODULE',\n"
      "`'\n"
      "`SectionVendorTuples.\"NAME_PREFIX.$1_tuples\" {'\n"
      "`        tokens \"audioreach_tokens\"'\n"
      "`        tuples.\"word.u32_data\" {'\n"
      "`                AR_TKN_U32_MODULE_INSTANCE_ID STR($4)'\n"
      "`                AR_TKN_U32_MODULE_ID STR($5)'\n"
      "`                AR_TKN_U32_MODULE_MAX_IP_PORTS STR($6)'\n"
      "`                AR_TKN_U32_MODULE_MAX_OP_PORTS STR($7)'\n"
      "`                AR_TKN_U32_MODULE_SRC_OP_PORT_ID STR($8)'\n"
      "`                AR_TKN_U32_MODULE_DST_IN_PORT_ID STR($9)'\n"
      "`                AR_TKN_U32_MODULE_SRC_INSTANCE_ID STR($4)'\n"
      "`                AR_TKN_U32_MODULE_DST_INSTANCE_ID STR($10)'\n"
      "`                $15'\n"
      "`        }'\n"
      "`}'\n"
      "`SectionData.\"NAME_PREFIX.$1_data\" {'\n"
      "`        tuples \"NAME_PREFIX.$1_tuples\"'\n"
      "`}'\n"
      "`SectionWidget.\"NAME_PREFIX.$1\" {'\n"
      "`        index STR($2)'\n"
      "`        type STR($11)'\n"
      "`ifelse(`$12', `', `', `        stream_name \"$12\"')'\n"
      "`        no_pm \"true\"'\n"
      "`        subseq \"10\"'\n"
      "`        data ['\n"
      "`                \"NAME_PREFIX.sub_graph$2_data\"'\n"
      "`                \"NAME_PREFIX.container$3_data\"'\n"
      "`                \"NAME_PREFIX.$1_data\"'\n"
      "`ifelse(`$13', `', `', `                \"$13\"')'\n"
      "`ifelse(`$14', `', `', `                \"$14\"')'\n"
      "`        ]'\n"
      "`}')dnl\n")

    # ------------------------------------------------------------ record
    sgid = 0xb00000d0
    ext_in = [c for c in rec.conns if c[2] in rec.mods and c[0] not in rec.mods]
    assert sorted(c[0] for c in ext_in) == [VOICE_TX_SPLITTER, VOICE_RX_SPLITTER], ext_in
    ctrls = [c for c in rec.ctrls if c[2] == VOICE_TX_MAILBOX or c[0] == VOICE_TX_MAILBOX]
    assert len(ctrls) == 1, ctrls
    by_mid = {}
    for i in rec.order:
        by_mid.setdefault(rec.mods[i][2], []).append(i)
    muxm, = by_mid[MID_MUX_DEMUX]
    ratm, = by_mid[MID_RAT]
    rec_prefix = 'stream%d' % RECORD_DAI_ID
    w('dnl ---- in-call record: stock SG %#x, %d modules -> "MultiMedia%d Capture" ----\n' % (
        sgid, len(rec.order), RECORD_DAI_ID + 1))
    w("define(`NAME_PREFIX', `%s')dnl\n" % rec_prefix)
    w("AR_PCM_CAPTURE(%d, `S16_LE', 48000, 48000, 1, 2)\n" % RECORD_DAI_ID)
    # Direction: the mux/demux table + matching RAT media format.
    for d in (1, 2, 3, 4):
        blk = mux[d]
        ws = []
        for iid, pid, size, pw in blk:
            assert iid == muxm
            ws += param_words(iid, pid, size, pw)
        rat = [x if n else '%#x' % ratm for n, x in enumerate(rat_mf(2 if d >= 3 else 1))]
        w("ifelse(FP6_INCALL_RECORD_DIR, `%d', `AR_MODULE_CFG_WORDS(`%s.mux_cfg',\n\t%s,\n\t%s)')dnl\n" % (
            d, rec_prefix, ', '.join(ws), ', '.join(rat)))
    # The two cross-graph connections and the RAT <-> TX mailbox drift link.
    a, ap, c, cp, it = ctrls[0]
    w("AR_MODULE_OPEN_WORDS(`%s.voice_taps',\n\t%s,\n\t%s)\n" % (
        rec_prefix, ', '.join(conn_words(sorted(ext_in))), ', '.join(ctrl_words(a, ap, c, cp, it))))
    rcfg = {muxm: '%s.mux_cfg' % rec_prefix}
    ropen = {muxm: '%s.voice_taps' % rec_prefix}
    for i in rec.order:
        mid = rec.mods[i][2]
        blobs = []
        if mid == MID_MFC:
            mf = cal_of(rcal, i, PID_MFC_OUT_MF)
            if mf and mf[1][0] != 0xffffffff:
                blobs += param_words(i, PID_MFC_OUT_MF, mf[0], mf[1])
        rp = cal_of(rcal, i, PID_RT_PROXY_CONFIG)
        if rp:
            blobs += param_words(i, PID_RT_PROXY_CONFIG, rp[0], rp[1])
        if blobs:
            name = '%s.m%x_cfg' % (rec_prefix, i)
            w("AR_MODULE_CFG_WORDS(`%s', %s)\n" % (name, ', '.join(blobs)))
            rcfg[i] = name
    rtup = {i: log_tuples(rcal, i, rec.mods[i][2]) for i in rec.order}
    order, lines, _ = emit_sg(out, rec, sgid, rec_prefix, RECORD_DAI_ID, 'MultiMedia%d Capture' % (RECORD_DAI_ID + 1),
                              'aif_out', [], ext_in, rcfg, ropen, rtup)
    first = [c[2] for c in ext_in if c[0] == VOICE_TX_SPLITTER][0]
    mixer = 'MultiMedia%d Mixer' % (RECORD_DAI_ID + 1)
    ctl = 'VoiceMMode1 Record'
    w('''
SectionControlMixer."%(ctl)s" {
	reg "-1"
	index "%(dai)d"
	max "1"
	invert "false"
	channel."fl" {
		reg "-1"
		max "1"
	}
	ops."ctl" {
		get "SND_SOC_AR_TPLG_FE_BE_GRAPH_CTL_MIX"
		put "SND_SOC_AR_TPLG_FE_BE_GRAPH_CTL_MIX"
		info "1"
	}
	data [
		"%(p)s.sub_graph1_data"
	]
}
SectionWidget."%(mixer)s" {
	reg "-1"
	index "%(dai)d"
	type "mixer"
	mixer [
		"%(ctl)s"
	]
	data [
		"%(p)s.sub_graph1_data"
	]
}
dnl DAPM only (gives DPCM the TX_CODEC_DMA_TX_3 back end through the voice
dnl capture mixer %(vmix)s). The source is not a module, so the mixer adds no APM
dnl connection: the real ones are in %(p)s.voice_taps.
SectionGraph."%(p)s.incall record Graph" {
	index "%(dai)d"
	lines [
		"%(p)s.m%(first)x, , %(mixer)s"
		"%(mixer)s, %(ctl)s, %(vmix)s"
		%(lines)s
	]
}
''' % dict(ctl=ctl, dai=RECORD_DAI_ID, p=rec_prefix, mixer=mixer, first=first, vmix=VOICE_TX_MIXER_WIDGET, lines='\n\t\t'.join(lines)))

    # ------------------------------------------------------------- music
    sgid = 0xb00000d6
    mprefix = 'stream%d' % MUSIC_DAI_ID
    last = [i for i in music.order if not any(c[0] == i for c in music.conns if c[2] in music.mods)]
    ext_out = [c for c in music.conns if c[0] in music.mods and c[2] not in music.mods]
    assert ext_out == [(last[0], 1, MUSIC_LAST_MFC, 2)], ext_out
    # 0x458b joins the music SG (stock: TX voice SG container 0xe000011d),
    # in the music SG's second container, MFC with one in, one out.
    music.mods[MUSIC_LAST_MFC] = (sgid, music.mods[last[0]][1], MID_MFC)
    music.ports[MUSIC_LAST_MFC] = (1, 1)
    w('dnl ---- in-call music: stock SG %#x, %d modules + MFC %#x -> "MultiMedia%d Playback" ----\n' % (
        sgid, len(music.order), MUSIC_LAST_MFC, MUSIC_DAI_ID + 1))
    w("ifdef(`FP6_INCALL_MUSIC', `\n")
    w("define(`NAME_PREFIX', `%s')dnl\n" % mprefix)
    w("AR_PCM_PLAYBACK(%d, `S16_LE', 48000, 48000, 1, 2)\n" % MUSIC_DAI_ID)
    w("AR_MODULE_OPEN_WORDS(`%s.uplink_inject',\n\t%s)\n" % (
        mprefix, ', '.join(conn_words([(MUSIC_LAST_MFC, 1, VOICE_TX_EC_SYNC, 4)]))))
    mtup = {i: log_tuples(mcal, i, music.mods[i][2]) for i in music.order}
    order, lines, _ = emit_sg(out, music, sgid, mprefix, MUSIC_DAI_ID, 'MultiMedia%d Playback' % (MUSIC_DAI_ID + 1),
                              'aif_in', [MUSIC_LAST_MFC], [], {}, {MUSIC_LAST_MFC: '%s.uplink_inject' % mprefix}, mtup)
    mixer = 'MultiMedia%d Mixer' % (MUSIC_DAI_ID + 1)
    ctl = 'VoiceMMode1 Uplink'
    w('''
SectionControlMixer."%(ctl)s" {
	reg "-1"
	index "%(dai)d"
	max "1"
	invert "false"
	channel."fl" {
		reg "-1"
		max "1"
	}
	ops."ctl" {
		get "SND_SOC_AR_TPLG_FE_BE_GRAPH_CTL_MIX"
		put "SND_SOC_AR_TPLG_FE_BE_GRAPH_CTL_MIX"
		info "1"
	}
	data [
		"%(p)s.sub_graph1_data"
	]
}
SectionWidget."%(mixer)s" {
	reg "-1"
	index "%(dai)d"
	type "mixer"
	mixer [
		"%(ctl)s"
	]
	data [
		"%(p)s.sub_graph1_data"
	]
}
dnl DAPM only (gives DPCM the SEN_MI2S_RX back end). The mixer widget is
dnl never a route source towards a module, so it adds no APM connection:
dnl the real one (%(last)#x:1 -> EC_SYNC %(ec)#x:4) is in %(p)s.uplink_inject.
SectionGraph."%(p)s.incall music Graph" {
	index "%(dai)d"
	lines [
		"%(mixer)s, %(ctl)s, %(p)s.m%(lastx)x"
		"SEN_MI2S_RX Audio Mixer, , %(mixer)s"
		%(lines)s
	]
}
')dnl
''' % dict(ctl=ctl, dai=MUSIC_DAI_ID, p=mprefix, mixer=mixer, last=MUSIC_LAST_MFC, lastx=MUSIC_LAST_MFC,
           ec=VOICE_TX_EC_SYNC, lines='\n\t\t'.join(lines)))


def check(voice_m4, patch=None):
    """The attach points this fragment relies on, in cs-call-audio's files."""
    t = open(voice_m4).read()
    errs = []
    mods = {}
    for m in re.finditer(r"AR_MODULE_GENERIC\(`m([0-9a-f]+)', ([^)]*)\)", t):
        a = [x.strip() for x in m.group(2).split(',')]
        mods[int(m.group(1), 16)] = a
    for iid in (VOICE_TX_SPLITTER, VOICE_RX_SPLITTER):
        a = mods.get(iid)
        if not a or int(a[3], 16) != 0x07001011 or a[5] != '2':
            errs.append('splitter %#x with 2 output ports missing' % iid)
        elif len(a) > 13 and a[13] == '3':
            errs.append('splitter %#x output port 3 is used by the voice graph' % iid)
    if VOICE_TX_MAILBOX not in mods:
        errs.append('TX mailbox %#x missing' % VOICE_TX_MAILBOX)
    ec = mods.get(VOICE_TX_EC_SYNC)
    if not ec or ec[4] != '2':
        errs.append('EC_SYNC %#x with 2 inputs missing' % VOICE_TX_EC_SYNC)
    if MUSIC_LAST_MFC in mods:
        errs.append('%#x is back in the TX voice graph: drop it from the music SG' % MUSIC_LAST_MFC)
    if VOICE_TX_MIXER_WIDGET not in t:
        errs.append('TX capture mixer widget %r not referenced' % VOICE_TX_MIXER_WIDGET)
    if patch:
        pt = open(patch).read()
        if 'SEN_MI2S_RX Audio Mixer' not in pt:
            errs.append('SEN_MI2S_RX Audio Mixer not in the patch')
        if 'AR_CTRL_LINK_WORDS' not in pt or 'AR_MODULE_OPEN_WORDS' not in pt:
            errs.append('AR_MODULE_OPEN_WORDS / AR_CTRL_LINK_WORDS helpers missing in the patch')
    for e in errs:
        print('check:', e)
    print('check: %s' % ('FAILED' if errs else 'ok'))
    return 1 if errs else 0


if __name__ == '__main__':
    if len(sys.argv) > 1 and sys.argv[1] == '--check':
        sys.exit(check(*sys.argv[2:4]))
    main()
