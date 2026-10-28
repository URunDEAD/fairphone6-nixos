-- SPDX-License-Identifier: GPL-3.0-or-later
-- FP6 app permissions (nixos-fp6/feature-push/app-permissions): camera and
-- microphone access of PipeWire clients, as decided by fp6-permd and
-- published per client by fp6-media-gate in the metadata object
-- "fp6-permissions" (created here):
--
--   subject 0,         "fp6.wireplumber" = "ready"   (set here)
--   subject <the gate's own client>, "fp6.gate" = "active:<its object.serial>"
--                      (gate running: a client on the manager socket; when
--                      the gate exits its client and the entry go away)
--   subject client id, "fp6.microphone" = "<object.serial>:granted|denied|ask"
--   subject client id, "fp6.camera"     = "<object.serial>:granted|denied|ask"
--
-- While the gate is active:
--  * no link from a microphone (Audio/Source*, Audio/Duplex) or a camera
--    (Video/Source*) to a client's stream unless that client's verdict is
--    "granted"; a link that loses its grant is removed. Streams wait
--    unlinked (no error, no destroy) and are linked as soon as the verdict
--    changes, e.g. when the user answers the prompt;
--  * clients whose verdict is "denied" do not see microphones / cameras at
--    all (permission manager), so they cannot link them themselves either;
--  * the entries are not protected against a client that writes them on
--    purpose (any unrestricted PipeWire client may set metadata); this gates
--    apps that use the microphone / camera normally, see the README.
-- Clients without a verdict yet wait (the gate answers within moments).
-- Without a live gate client nothing is enforced (a stopped or crashed gate
-- never blocks audio).

log = Log.open_topic ("s-fp6-permissions")
lutils = require ("linking-utils")
cutils = require ("common-utils")

local META = "fp6-permissions"

fp6_impl_metadata = ImplMetadata (META)
fp6_impl_metadata:activate (Features.ALL, function (m, e)
  if e then
    log:warning ("cannot create the " .. META .. " metadata: " .. tostring (e))
    return
  end
  -- an entry from the start: pw-dump (how fp6-media-gate finds this object)
  -- does not list empty metadata objects
  m:set (0, "fp6.wireplumber", "Spa:String", "ready")
end)

fp6_clients_om = ObjectManager { Interest { type = "client" } }
fp6_clients_om:activate ()

fp6_meta_om = ObjectManager {
  Interest { type = "metadata", Constraint { "metadata.name", "=", META } },
}

local function meta ()
  return fp6_meta_om:lookup ()
end

local function gate_active ()
  local m = meta ()
  if m == nil then
    return false
  end
  for c in fp6_clients_om:iterate () do
    if c:get_property ("pipewire.sec.socket") == "pipewire-0-manager" then
      local v = m:find (c ["bound-id"], "fp6.gate")
      if v ~= nil and v == "active:" .. tostring (c.properties ["object.serial"]) then
        return true
      end
    end
  end
  return false
end

-- "granted", "denied", "ask" or "pending" (no verdict for this connection yet)
local function verdict (client_id, perm)
  if not gate_active () then
    return "granted"
  end
  client_id = tonumber (client_id)
  if client_id == nil then
    -- in-process nodes of PipeWire / WirePlumber themselves
    return "granted"
  end
  local client = fp6_clients_om:lookup {
    Constraint { "bound-id", "=", client_id, type = "gobject" },
  }
  if client == nil then
    return "pending"
  end
  local v = meta ():find (client_id, "fp6." .. perm)
  if v == nil then
    return "pending"
  end
  local serial, what = string.match (v, "^(%d+):(%a+)$")
  if serial == nil or serial ~= tostring (client.properties ["object.serial"]) then
    return "pending"
  end
  return what
end

local function perm_of_class (class)
  class = class or ""
  if class == "Audio/Source" or class == "Audio/Source/Virtual" or class == "Audio/Duplex" then
    return "microphone"
  end
  if class == "Video/Source" or class == "Video/Source/Virtual" then
    return "camera"
  end
  return nil
end

local function perm_of_si (si)
  local node = si:get_associated_proxy ("node")
  local class = (node and node.properties ["media.class"]) or si.properties ["media.class"]
  return perm_of_class (class)
end

-- ---- permission manager: hide sources from denied clients ----------------
fp6_pm = PermissionManager ()
fp6_pm:set_default_permissions (Perm.ALL)

local function hide_from_denied (perm)
  return function (_, client, _)
    if verdict (client ["bound-id"], perm) == "denied" then
      return Perm.NONE
    end
    return Perm.ALL
  end
end

for _, class in ipairs ({ "Audio/Source", "Audio/Source/Virtual", "Audio/Duplex", "Video/Source", "Video/Source/Virtual" }) do
  fp6_pm:add_interest_match (hide_from_denied (perm_of_class (class)),
    Interest { type = "node", Constraint { "media.class", "=", class } })
end

SimpleEventHook {
  name = "client/find-fp6-access",
  after = { "client/find-config-access", "client/find-flatpak-access",
            "client/find-snap-access", "client/find-portal-access" },
  before = "client/find-default-access",
  interests = {
    EventInterest {
      Constraint { "event.type", "=", "select-access" },
    },
  },
  execute = function (event)
    -- sandboxed / configured clients keep the permission manager chosen
    -- for them; linking below still applies to them
    if event:get_data ("permission-manager") ~= nil or event:get_data ("default-permissions") ~= nil then
      return
    end
    local client = event:get_subject ()
    if cutils.get_client_access (client.properties) == "restricted" then
      return
    end
    log:debug (client, "fp6 permission manager")
    event:set_data ("permission-manager", fp6_pm)
  end
}:register ()

-- ---- linking: no capture from a source without a grant --------------------
SimpleEventHook {
  name = "linking/fp6-capture-permission",
  after = { "linking/find-defined-target", "linking/find-filter-target",
            "linking/find-audio-group-target", "linking/find-media-role-target",
            "linking/find-media-role-sink-target", "linking/find-default-target",
            "linking/find-best-target", "linking/get-filter-from-target" },
  before = "linking/prepare-link",
  interests = {
    EventInterest {
      Constraint { "event.type", "=", "select-target" },
    },
  },
  execute = function (event)
    local _, om, si, si_props, si_flags, target = lutils:unwrap_select_target_event (event)
    if si_props ["item.node.type"] ~= "stream" then
      return
    end

    -- the source it would read from: the chosen target, else the current peer
    local peer = nil
    if si_flags.peer_id then
      peer = om:lookup { type = "SiLinkable", Constraint { "id", "=", si_flags.peer_id, type = "gobject" } }
    end
    local perm = nil
    if target then
      perm = perm_of_si (target)
    elseif peer then
      perm = perm_of_si (peer)
    end
    if perm == nil then
      return
    end

    local node = si:get_associated_proxy ("node")
    local client_id = node and node.properties ["client.id"]
    local v = verdict (client_id, perm)
    if v == "granted" then
      return
    end

    log:info (si, string.format ("%s of '%s' (client %s): %s, not linking",
        perm, tostring (si_props ["node.name"]), tostring (client_id), v))
    if peer and perm_of_si (peer) then
      local link = lutils.lookupLink (si.id, si_flags.peer_id)
      if link ~= nil then
        link:remove ()
        log:info (si, "... link to the source removed")
      end
      si_flags.peer_id = nil
    end
    event:set_data ("target", nil)
    event:stop_processing ()
  end
}:register ()

-- ---- verdict changes: re-apply permissions, re-link ------------------------
local function on_change (_, subject, key, _, _)
  if key ~= nil and string.sub (key, 1, 4) ~= "fp6." then
    return
  end
  fp6_pm:update_permissions ()
  local source = Plugin.find ("standard-event-source")
  if source then
    source:call ("schedule-rescan", "linking")
  end
end

-- the gate's client going away ends enforcement
fp6_clients_om:connect ("object-removed", function (_, c)
  local m = meta ()
  if m ~= nil and m:find (c ["bound-id"], "fp6.gate") ~= nil then
    log:info ("fp6-media-gate gone: not enforcing")
    on_change (m, 0, nil, nil, nil)
  end
end)

fp6_meta_om:connect ("object-added", function (_, m)
  m:connect ("changed", on_change)
  on_change (m, 0, nil, nil, nil)
end)
fp6_meta_om:activate ()
