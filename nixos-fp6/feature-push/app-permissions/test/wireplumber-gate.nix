# Runs PipeWire + WirePlumber (with wireplumber/fp6-media-permissions.lua)
# in the build sandbox, with a fake microphone, and checks what the script
# enforces from the fp6-permissions metadata (what fp6-media-gate writes):
#   no gate key: capture links as usual; gate active: a capture stream
#   waits until its client's verdict is "<serial>:granted", the link goes
#   when it turns "denied", a verdict for another serial does not count,
#   without a live gate client nothing is enforced.
#   nix-build test/wireplumber-gate.nix
{ pkgs ? import (import ../../../upstream/mobile-nixos/npins).nixpkgs { } }:
let
  script = ../wireplumber/fp6-media-permissions.lua;
  fragment = pkgs.writeText "60-fp6-media-permissions.conf" ''
    wireplumber.components = [
      { name = fp6-media-permissions.lua, type = script/lua, provides = custom.fp6-media-permissions }
    ]
    wireplumber.profiles = { main = { custom.fp6-media-permissions = required } }
  '';
  pwFragment = pkgs.writeText "fake-mic.conf" ''
    context.objects = [
      { factory = adapter
        args = { factory.name = support.null-audio-sink node.name = "fake-mic"
                 media.class = "Audio/Source/Virtual" audio.position = [ MONO ]
                 object.linger = true node.driver = true } }
    ]
  '';
in pkgs.runCommand "fp6-wireplumber-gate-test" {
  nativeBuildInputs = [ pkgs.pipewire pkgs.wireplumber pkgs.dbus pkgs.python3 pkgs.coreutils ];
} ''
  set -u
  export HOME=$TMPDIR/home XDG_RUNTIME_DIR=$TMPDIR/run XDG_CONFIG_HOME=$TMPDIR/config XDG_DATA_HOME=$TMPDIR/data
  mkdir -p $HOME $XDG_RUNTIME_DIR $XDG_CONFIG_HOME/pipewire/pipewire.conf.d $XDG_CONFIG_HOME/wireplumber/wireplumber.conf.d $XDG_DATA_HOME/wireplumber/scripts
  chmod 700 $XDG_RUNTIME_DIR
  cp ${pwFragment} $XDG_CONFIG_HOME/pipewire/pipewire.conf.d/fake-mic.conf
  cp ${fragment} $XDG_CONFIG_HOME/wireplumber/wireplumber.conf.d/60-fp6-media-permissions.conf
  cp ${script} $XDG_DATA_HOME/wireplumber/scripts/fp6-media-permissions.lua
  cat > $TMPDIR/dbus.conf <<EOF
  <busconfig><type>session</type><listen>unix:path=$XDG_RUNTIME_DIR/bus</listen>
  <policy context="default"><allow send_destination="*" eavesdrop="true"/><allow eavesdrop="true"/><allow own="*"/></policy></busconfig>
  EOF
  dbus-daemon --config-file=$TMPDIR/dbus.conf --fork --nopidfile
  export DBUS_SESSION_BUS_ADDRESS=unix:path=$XDG_RUNTIME_DIR/bus
  PIPEWIRE_DEBUG=3 pipewire > $TMPDIR/pipewire.log 2>&1 &
  sleep 2
  WIREPLUMBER_DEBUG=3,s-client:5,s-fp6-permissions:5,wp-permission-manager:5 wireplumber > $TMPDIR/wp.log 2>&1 &
  wp=$!
  python3 -I ${./wireplumber-gate.py} || { echo "--- wireplumber log"; grep -i "fp6 permission\|Updating permissions\|Attached\|handling client" $TMPDIR/wp.log | tail -30 | grep -v impl_set_property | head -40; grep "^[EWC] " $TMPDIR/wp.log | grep -v "bluez\|dbus\|rtkit\|resource.c\|node.c\|logind" | head -30; pw-dump | grep -i "metadata.name"; exit 1; }
  kill $wp || true
  if grep -i "fp6-media-permissions.*error\|lua.*error" $TMPDIR/wp.log; then echo "script errors"; exit 1; fi
  grep "not linking" $TMPDIR/wp.log | head -5
  mkdir -p $out; cp $TMPDIR/wp.log $out/
''
