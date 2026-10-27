-- FP6 accessibility audio: mono downmix and left/right balance on every
-- stereo output (internal speaker, wired/USB, Bluetooth), live.
--
-- Mechanism: PipeWire's audioconvert filter graph (PipeWire >= 1.4). Each
-- output node's own audioconvert gets the Props param
-- "audioconvert.filter-graph.<ORDER>" = a 2-in/2-out mixer graph
--   L' = gL * (mono ? (L + R) / 2 : L),  R' = gR * (mono ? (L + R) / 2 : R)
--   gL = min(1, 1 - balance), gR = min(1, 1 + balance)
-- so no node is added, removed or relinked: the default device, the
-- audio-routing module's device choice, per-app targets and volumes stay
-- exactly as they are. With mono off and balance 0 the graph is removed
-- (empty string), so there is nothing in the audio path.
--
-- Settings: WirePlumber settings fp6.a11y.mono-audio (bool) and
-- fp6.a11y.audio-balance (float), written (with --save) by
-- fp6-accessibility-audio from GSettings com.fairphone6.accessibility.
-- SPDX-License-Identifier: MIT

log = Log.open_topic ("s-fp6-a11y")

-- A high order: after any other filter graph (EQ etc.) on the node.
local GRAPH_KEY = "audioconvert.filter-graph.90"
local applied = {}   -- node id -> graph string last set

local function fmt (x)
  return string.format ("%.4f", x)
end

local function graph_for (mono, balance)
  if not mono and math.abs (balance) < 0.005 then
    return ""
  end
  local gl = math.min (1.0, 1.0 - balance)
  local gr = math.min (1.0, 1.0 + balance)
  local ll, lr, rl, rr
  if mono then
    ll, lr, rl, rr = 0.5 * gl, 0.5 * gl, 0.5 * gr, 0.5 * gr
  else
    ll, lr, rl, rr = gl, 0.0, 0.0, gr
  end
  return "{ nodes = [ "
    .. "{ type = builtin label = copy name = inl } "
    .. "{ type = builtin label = copy name = inr } "
    .. "{ type = builtin label = mixer name = outl control = { \"Gain 1\" = " .. fmt (ll) .. " \"Gain 2\" = " .. fmt (lr) .. " } } "
    .. "{ type = builtin label = mixer name = outr control = { \"Gain 1\" = " .. fmt (rl) .. " \"Gain 2\" = " .. fmt (rr) .. " } } "
    .. "] links = [ "
    .. "{ output = \"inl:Out\" input = \"outl:In 1\" } "
    .. "{ output = \"inr:Out\" input = \"outl:In 2\" } "
    .. "{ output = \"inl:Out\" input = \"outr:In 1\" } "
    .. "{ output = \"inr:Out\" input = \"outr:In 2\" } "
    .. "] inputs = [ \"inl:In\" \"inr:In\" ] outputs = [ \"outl:Out\" \"outr:Out\" ] }"
end

-- Only plain stereo outputs: the graph has exactly two inputs (mono outputs
-- such as the earpiece are left alone).
local function is_stereo (props)
  local ch = tonumber (props ["audio.channels"] or "")
  if ch then
    return ch == 2
  end
  local pos = props ["audio.position"]
  if pos then
    local n = 0
    for _ in string.gmatch (pos, "[%w_]+") do n = n + 1 end
    return n == 2
  end
  -- Bluetooth A2DP sinks may not carry channel properties; A2DP is stereo.
  local profile = props ["api.bluez5.profile"] or ""
  if string.find (profile, "a2dp", 1, true) then
    return true
  end
  return false
end

local function current_graph ()
  local mono = Settings.get_boolean ("fp6.a11y.mono-audio")
  local balance = Settings.get_float ("fp6.a11y.audio-balance") or 0.0
  if balance > 1 then balance = 1 elseif balance < -1 then balance = -1 end
  return graph_for (mono, balance)
end

local function apply (node, graph)
  local props = node.properties
  if not is_stereo (props) then
    return
  end
  local id = node ["bound-id"]
  if (applied [id] or "") == graph then
    return
  end
  log:info (node, "fp6 accessibility audio on " .. tostring (props ["node.name"]) .. ": "
            .. (graph == "" and "off" or graph))
  node:set_params ("Props", Pod.Object {
    "Spa:Pod:Object:Param:Props", "Props",
    params = Pod.Struct { GRAPH_KEY, graph }
  })
  applied [id] = graph
end

sinks_om = ObjectManager {
  Interest {
    type = "node",
    Constraint { "media.class", "=", "Audio/Sink", type = "pw-global" },
  }
}

local function apply_all ()
  local graph = current_graph ()
  for node in sinks_om:iterate () do
    apply (node, graph)
  end
end

sinks_om:connect ("object-added", function (_, node)
  apply (node, current_graph ())
end)
sinks_om:connect ("object-removed", function (_, node)
  applied [node ["bound-id"]] = nil
end)

Settings.subscribe ("fp6.a11y.mono-audio", apply_all)
Settings.subscribe ("fp6.a11y.audio-balance", apply_all)

sinks_om:activate ()
