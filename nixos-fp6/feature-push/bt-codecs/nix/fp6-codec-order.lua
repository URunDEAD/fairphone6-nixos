-- SPDX-License-Identifier: MIT
-- Runs only in WirePlumber's normal profile selection; status reads never invoke it.
local cutils = require("common-utils")
local order = Json.Raw([==[@ORDER@]==]):parse()
local descriptions = {
  sbc="SBC", sbc_xq="SBC-XQ", aac="AAC", aac_eld="AAC-ELD",
  aptx="aptX", aptx_hd="aptX HD", aptx_ll="aptX-LL", ldac="LDAC", lc3="LC3"
}
local function matches(profile, codec)
  if profile.available == "no" then return false end
  local name = profile.name or ""
  if codec == "lc3" then return name == "bap-sink" or name == "bap-duplex" or name == "bap-sink-lc3" end
  if name == "a2dp-sink-" .. codec then return true end
  -- The highest-priority codec has the unsuffixed base name in PipeWire 1.6.7.
  local suffix = descriptions[codec] and (" " .. descriptions[codec] .. ")")
  local desc = profile.description or ""
  return name == "a2dp-sink" and suffix and desc:sub(-#suffix) == suffix
end
SimpleEventHook {
  name = "device/fp6-codec-order",
  after = { "device/find-stored-profile", "device/find-calling-profile" },
  before = "device/find-preferred-profile",
  interests = { EventInterest { Constraint { "event.type", "=", "select-profile" } } },
  execute = function(event)
    if event:get_data("selected-profile") then return end
    local device = event:get_subject()
    if device.properties["device.api"] ~= "bluez5" then return end
    local profiles = {}
    for pod in device:iterate_params("EnumProfile") do
      local p = cutils.parseParam(pod, "EnumProfile")
      if p then table.insert(profiles, p) end
    end
    for _, codec in ipairs(order) do
      for _, p in ipairs(profiles) do
        if matches(p, codec) then event:set_data("selected-profile", p); return end
      end
    end
    -- Unmatched devices keep the upstream policy, including voice and LE Audio.
  end
}:register()
