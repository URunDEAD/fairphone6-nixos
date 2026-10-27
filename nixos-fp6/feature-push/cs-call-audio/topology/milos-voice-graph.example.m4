dnl FORMAT EXAMPLE ONLY - NOT A WORKING VOICE GRAPH.
dnl
dnl The real file is generated from the stock FP6 ACDB voice-call graph
dnl (key vector StreamRX=VOICE_CALL_RX / StreamTX=VOICE_CALL_TX,
dnl VSID=VSID_VOICE1, handset device) - see the cs-call-audio README,
dnl "Phase B". The module ids below are 0 (invalid): a topology built from
dnl this file loads, exposes the VoiceMMode1 PCMs and mixer controls, and
dnl the DSP rejects GRAPH_OPEN when the PCM is opened. Building it needs
dnl -DFP6_VOICE_ALLOW_EXAMPLE.
ifdef(`FP6_VOICE_ALLOW_EXAMPLE', `', `errprint(`milos-voice-graph.example.m4 is a format example; pass a generated graph
')m4exit(`1')')dnl
dnl
dnl Ids that are ours (topology namespace, not from the ACDB)
define(`VOICE_RX_DAI_ID', `16')dnl
define(`VOICE_TX_DAI_ID', `17')dnl
define(`VOICE_RX_SG_IID', `0x00004010')dnl
define(`VOICE_TX_SG_IID', `0x00004011')dnl
define(`VOICE_RX_CONT_IID', `0x00004010')dnl
define(`VOICE_TX_CONT_IID', `0x00004011')dnl
define(`VOICE_RX_CONT_CAP', `APM_CONTAINER_TYPE_ID_GC')dnl
define(`VOICE_TX_CONT_CAP', `APM_CONTAINER_TYPE_ID_GC')dnl
dnl
dnl Ids that MUST come from the ACDB graph (module ids of the voice stream
dnl subgraph modules; their order and ports as in the stock GRAPH_OPEN)
define(`VOICE_RX_MAILBOX_MID', `0x0')dnl
define(`VOICE_DECODER_MID', `0x0')dnl
define(`VOICE_RX_SOFTVOL_MID', `0x0')dnl
define(`VOICE_TX_SOFTVOL_MID', `0x0')dnl
define(`VOICE_ENCODER_MID', `0x0')dnl
define(`VOICE_TX_MAILBOX_MID', `0x0')dnl
dnl
dnl RX: modem mailbox (FE endpoint) -> decoder -> soft volume -> device
define(`VOICE_RX_MODULES', `
AR_MODULE_GENERIC(`voice_rx_mbox', 1, 1, 0x6200, VOICE_RX_MAILBOX_MID, 0, 1, 1, 2, 0x6201, `aif_in', `VoiceMMode1 Playback', `')
AR_MODULE_GENERIC(`voice_dec', 1, 1, 0x6201, VOICE_DECODER_MID, 1, 1, 1, 2, 0x6202, `buffer', `', `')
AR_MODULE_GENERIC(`voice_rx_vol', 1, 1, 0x6202, VOICE_RX_SOFTVOL_MID, 1, 1, 1, 2, NONE_IID, `buffer', `', `stream16.voice_rx_vcpm')
AR_MODULE_CFG_WORDS(`stream16.voice_rx_vcpm',
	VCPM_VOICE_CONFIG_WORDS(0x00004010,
		VOICE_MOD_TAG_ID_RX_MAILBOX, 0x6200,
		VOICE_MOD_TAG_ID_DECODER, 0x6201,
		VOICE_MOD_TAG_ID_STREAM_RX_SOFT_VOL, 0x6202),
	VCPM_VSID_WORDS(VSID_VOICE1))
')dnl
define(`VOICE_RX_LINES', `"stream16.voice_dec, , stream16.voice_rx_mbox"
		"stream16.voice_rx_vol, , stream16.voice_dec"')dnl
dnl Route into the SEN_MI2S_RX Audio Mixer: "<control>, <last RX module>"
define(`VOICE_RX_ROUTE', ``VoiceMMode1, stream16.voice_rx_vol'')dnl
dnl
dnl TX: device -> soft volume -> encoder -> modem mailbox (FE endpoint)
define(`VOICE_TX_MODULES', `
AR_MODULE_GENERIC(`voice_tx_vol', 1, 1, 0x6210, VOICE_TX_SOFTVOL_MID, 1, 1, 1, 2, 0x6211, `buffer', `', `stream17.voice_tx_vcpm')
AR_MODULE_GENERIC(`voice_enc', 1, 1, 0x6211, VOICE_ENCODER_MID, 1, 1, 1, 2, 0x6212, `buffer', `', `')
AR_MODULE_GENERIC(`voice_tx_mbox', 1, 1, 0x6212, VOICE_TX_MAILBOX_MID, 1, 0, 1, 0, NONE_IID, `aif_out', `VoiceMMode1 Capture', `')
AR_MODULE_CFG_WORDS(`stream17.voice_tx_vcpm',
	VCPM_VOICE_CONFIG_WORDS(0x00004011,
		VOICE_MOD_TAG_ID_STREAM_TX_SOFT_VOL, 0x6210,
		VOICE_MOD_TAG_ID_ENCODER, 0x6211,
		VOICE_MOD_TAG_ID_TX_MAILBOX, 0x6212),
	VCPM_VSID_WORDS(VSID_VOICE1))
')dnl
define(`VOICE_TX_LINES', `"stream17.voice_tx_vol, , VoiceMMode1 Capture Mixer"
		"stream17.voice_enc, , stream17.voice_tx_vol"
		"stream17.voice_tx_mbox, , stream17.voice_enc"')dnl
