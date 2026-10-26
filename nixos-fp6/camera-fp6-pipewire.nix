# Minimal upstream client-side fix for Snapshot/GStreamer's camera provider.
# PipeWire emits repeated metadata globals during permission changes; rebinding
# one spa_hook corrupts the proxy listener list and dispatches invalid callbacks.
# Upstream: 1e817dff65c7a39384af75aa50e9ae4a95b201a9 (2026-07-09).
{ pipewire, libcamera }:
(pipewire.override { inherit libcamera; }).overrideAttrs (old: {
  patches = (old.patches or []) ++ [ ./camera-fp6-pipewire-metadata-listener.patch ];
})
