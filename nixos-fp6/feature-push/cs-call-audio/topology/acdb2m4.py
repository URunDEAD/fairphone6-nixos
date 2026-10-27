#!/usr/bin/env python3
"""Turn the stock FP6 ACDB voice-call graph into milos-voice-graph.m4.

Input: the GRAPH_OPEN blobs that tools/acdbq dumps from the stock ACDB
(<prefix>.spf.bin = per-subgraph APM params, <prefix>.conn.bin = params
between subgraphs). The blobs stay out of git (proprietary); only the
numeric structure written here (module ids, instance ids, ports,
connections, control links, VCPM tag table) goes into our m4.

What is kept (see the cs-call-audio README, "Phase B"):
  RX: stream SG 0xb0000040 + stream-PP SG 0xb0000041 (merged into one
      hostless FE subgraph); the RX device-PP SG 0xb0000044 and the stock
      device SGs are replaced by our SEN_MI2S_RX device subgraph.
  TX: stream SG 0xb000003f; the TX device-PP SG 0xb000003e (Fluence
      SMECNS + EC reference) and the stock device SG are replaced by our
      TX_CODEC_DMA_TX_3 device subgraph.
Control links and the VCPM VOICE_CONFIG ride in the GRAPH_OPEN (kernel
candidate 0008); the VSID is a SET_CFG blob (0006).

usage: acdb2m4.py <rx-prefix> <tx-prefix> > milos-voice-graph.m4
"""
import struct
import sys

P_SG, P_CONT, P_ML, P_MP, P_CONN, P_CTRL = 0x08001001, 0x08001000, 0x08001002, 0x08001003, 0x08001004, 0x08001061
VCPM_IID, VCPM_VOICE_CONFIG = 0x4, 0x08001162
PROP_PORT_INFO = 0x08001015
SG_PERF, SG_DIR, SG_SCEN = 0x0800100E, 0x0800100F, 0x08001010
C_TYPE, C_POS, C_STACK, C_DOMAIN = 0x08001011, 0x08001012, 0x08001013, 0x08001014

# Our device endpoints (FP6 topology): the stock endpoint instance ids that
# control links refer to are mapped onto them.
OUR_I2S_RX_IID = 0x6062      # device147.i2s_rx1 (MOD_IID_START 0x6060 + 2)
OUR_CDC_DMA_TX_IID = 0x6130  # device120.codec_dma_tx1 (MOD_IID_START 0x6130)
STOCK_I2S_RX_IID = 0x4615    # stock MI2S-LPAIF_WSA RX endpoint (handset)
STOCK_CDC_DMA_TX_IID = 0x43AF  # stock codec DMA TX endpoint (handset mic)

VSID_VOICE1 = 0x11C05000
VCPM_PARAM_ID_VSID = 0x080011BC


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
    def __init__(self):
        self.sgs = {}       # sg -> props
        self.conts = {}     # cont -> props
        self.mods = {}      # iid -> (sg, cont, mid)
        self.order = []     # iids in ACDB order
        self.ports = {}     # iid -> (max_ip, max_op)
        self.conns = []     # (src, sport, dst, dport)
        self.ctrls = []     # (p1, port1, p2, port2, intent)
        self.vcpm = {}      # sg -> raw VOICE_CONFIG payload words

    def parse(self, b):
        o = 0
        while o + 16 <= len(b):
            iid, pid, psz, _err = words(b, o, 4)
            p = b[o + 16:o + 16 + psz]
            if iid == VCPM_IID and pid == VCPM_VOICE_CONFIG:
                w = words(p, 0, psz // 4)
                self.vcpm[w[1]] = w
            elif pid == P_SG:
                q = 4
                for _ in range(words(p, 0, 1)[0]):
                    sg, n = words(p, q, 2)
                    self.sgs[sg], q = props(p, q + 8, n)
            elif pid == P_CONT:
                q = 4
                for _ in range(words(p, 0, 1)[0]):
                    c, n = words(p, q, 2)
                    self.conts[c], q = props(p, q + 8, n)
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
                    intents = pr.get(0x08001062, [0])
                    for it in intents[1:1 + intents[0]]:
                        self.ctrls.append((a, ap, c, cp, it))
            o += 16 + ((psz + 7) & ~7)


def load(prefix):
    g = Graph()
    for suf in ('.spf.bin', '.conn.bin'):
        g.parse(open(prefix + suf, 'rb').read())
    return g


def topo_order(keep, conns):
    indeg = {i: 0 for i in keep}
    for s, _, d, _ in conns:
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
    assert len(out) == len(keep), 'cycle in voice subgraph'
    return out


def emit(name, g, sgs, drop, prefix, fe_stream, endpoint_map, allmods, out):
    keep = [i for i in g.order if g.mods[i][0] in sgs and i not in drop]
    keepset = set(keep)
    conns = [c for c in g.conns if c[0] in keepset and c[2] in keepset]
    order = topo_order(keep, conns)
    srcs = {i for i in keep if not any(c[2] == i for c in conns)}
    sinks = {i for i in keep if not any(c[0] == i for c in conns)}
    # The FE endpoint: RX = the source (modem mailbox), TX = the sink.
    fe = [i for i in (srcs if name == 'RX' else sinks) if g.ports.get(i, (1, 1))[0 if name == 'RX' else 1] == 0]
    assert len(fe) == 1, (name, fe)
    fe = fe[0]
    ext = (sinks if name == 'RX' else srcs) - {fe}
    assert len(ext) == 1, (name, ext)
    ext = ext.pop()

    sg_main = min(sgs) if name == 'TX' else 0xb0000040
    sgp = g.sgs[sg_main]
    conts = []
    for i in order:
        c = g.mods[i][1]
        if c not in conts:
            conts.append(c)
    cidx = {c: n + 1 for n, c in enumerate(conts)}

    w = out.write
    w('dnl ---- %s: stock SG(s) %s, %d modules ----\n' % (name, ' '.join('%#x' % s for s in sorted(sgs)), len(order)))
    w("define(`VOICE_%s_SG_IID', `%#010x')dnl\n" % (name, sg_main))
    w("define(`VOICE_%s_SG_PERF', `%#x')dnl\n" % (name, sgp[SG_PERF][0]))
    w("define(`VOICE_%s_CONTAINERS', `\n" % name)
    for c in conts:
        cp = g.conts[c]
        w('AR_CONTAINER(%d, %#010x, %#010x, %#x, %#x, %#x)\n' % (
            cidx[c], c, cp[C_TYPE][1], cp[C_STACK][0], cp[C_POS][0], cp[C_DOMAIN][0]))
    w("')dnl\n")

    # Control links touching this graph's modules, with stock device
    # endpoints mapped onto ours; links to modules we do not have are left
    # out. Links to the other direction's stream modules stay (the stock
    # graphs carry them in both opens).
    def mapiid(i):
        return endpoint_map.get(i, i)
    known = allmods | set(endpoint_map.values())
    ctrls = []
    for a, ap, c, cp, it in g.ctrls:
        a, c = mapiid(a), mapiid(c)
        if (a in keepset or c in keepset) and a in known and c in known:
            if (a, ap, c, cp, it) not in ctrls:
                ctrls.append((a, ap, c, cp, it))
    open_words = []
    for a, ap, c, cp, it in ctrls:
        open_words += ['AR_CTRL_LINK_WORDS(%#x, %#x, %#x, %#x, %#x)' % (a, ap, c, cp, it)]
    vc = g.vcpm.get(sg_main)
    assert vc, 'no VCPM VOICE_CONFIG for %#x' % sg_main
    assert all(t in keepset for t in vc[7::2]), 'VCPM tag on a dropped module'
    param_size = 4 * len(vc)
    pad = [0] if param_size % 8 else []
    open_words += ['%#x,%#x,%d,0,' % (VCPM_IID, VCPM_VOICE_CONFIG, param_size) +
                   ','.join('%#x' % x for x in vc + pad)]

    w("define(`VOICE_%s_MODULES', `\n" % name)
    for n, i in enumerate(order):
        sg, c, mid = g.mods[i]
        mi, mo = g.ports.get(i, (1, 1))
        outs = [x for x in conns if x[0] == i]
        assert len(outs) <= 2, ('more than 2 outputs', hex(i))
        l0 = outs[0] if outs else (i, 1, 0, 2)
        l1 = outs[1] if len(outs) > 1 else None
        wtype = 'buffer'
        stream = ''
        if i == fe:
            wtype = 'aif_in' if name == 'RX' else 'aif_out'
            stream = fe_stream
        cfg = '%s.voice_vsid' % prefix if n == 0 else ''
        opn = '%s.voice_open' % prefix if n == 0 else ''
        args = ["`m%x'" % i, '1', str(cidx[c]), '%#x' % i, '%#010x' % mid, str(mi), str(mo),
                str(l0[1]), str(l0[3]), '%#x' % (l0[2] if outs else 0),
                "`%s'" % wtype, "`%s'" % stream, "`%s'" % cfg, "`%s'" % opn]
        if l1:
            args += [str(l1[1]), str(l1[3]), '%#x' % l1[2]]
        w('AR_MODULE_GENERIC(%s)\n' % ', '.join(args))
    w("AR_MODULE_CFG_WORDS(`%s.voice_vsid', %#x, %#x, 4, 0, %#x, 0)\n" % (prefix, VCPM_IID, VCPM_PARAM_ID_VSID, VSID_VOICE1))
    w("AR_MODULE_OPEN_WORDS(`%s.voice_open',\n\t%s)\n" % (prefix, ',\n\t'.join(open_words)))
    w("')dnl\n")
    w("define(`VOICE_%s_LINES', `" % name)
    lines = []
    for s, _, d, _ in conns:
        line = '"%s.m%x, , %s.m%x"' % (prefix, d, prefix, s)
        if line not in lines:  # two port links between the same modules
            lines.append(line)
    if name == 'TX':
        lines.insert(0, '"%s.m%x, , VoiceMMode1 Capture Mixer"' % (prefix, ext))
    w('\n\t\t'.join(lines))
    w("')dnl\n")
    if name == 'RX':
        w("define(`VOICE_RX_ROUTE', ``VoiceMMode1, %s.m%x'')dnl\n" % (prefix, ext))
    return keepset


def main():
    rx, tx = load(sys.argv[1]), load(sys.argv[2])
    rx_sgs = {0xb0000040, 0xb0000041}
    tx_sgs = {0xb000003f}
    tx_drop = {0x458b}   # MFC fed only by sources outside the graph (sidetone/HPCM)
    rx_keep = {i for i in rx.order if rx.mods[i][0] in rx_sgs}
    tx_keep = {i for i in tx.order if tx.mods[i][0] in tx_sgs and i not in tx_drop}
    allmods = rx_keep | tx_keep
    out = sys.stdout
    out.write('dnl Fairphone (Gen. 6) voice-call graph, generated by acdb2m4.py from the\n'
              'dnl stock ACDB (acdbdata/volcano_mtp_fps, PAL handset voice key vectors:\n'
              'dnl RX A1000010/A2000004/AC000005/AF000001/B3000001,\n'
              'dnl TX B1000007/A3000004/AD000008/B3000001). Do not edit by hand.\n')
    out.write("define(`VOICE_RX_DAI_ID', `16')dnl\ndefine(`VOICE_TX_DAI_ID', `17')dnl\n")
    emit('RX', rx, rx_sgs, set(), 'stream16', 'VoiceMMode1 Playback',
         {STOCK_I2S_RX_IID: OUR_I2S_RX_IID}, allmods, out)
    emit('TX', tx, tx_sgs, tx_drop, 'stream17', 'VoiceMMode1 Capture',
         {STOCK_CDC_DMA_TX_IID: OUR_CDC_DMA_TX_IID}, allmods, out)


if __name__ == '__main__':
    main()
