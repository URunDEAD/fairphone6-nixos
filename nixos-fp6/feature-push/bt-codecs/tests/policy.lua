-- Mock WirePlumber's event/device API; run with Lua 5.4.
local hook
package.preload["common-utils"] = function() return { parseParam=function(p) return p end } end
Json={Raw=function(_) return {parse=function() return {"ldac","aptx_hd","sbc"} end} end}
Constraint=function(x) return x end
EventInterest=function(x) return x end
SimpleEventHook=function(x) x.register=function(self) hook=self end; return x end
local file=assert(io.open("nix/fp6-codec-order.lua"));local code=file:read("*a");file:close();assert(load(code))()
local function event(profiles,selected,api)
 local e={selected=selected}
 local d={properties={["device.api"]=api or "bluez5"}}
 function d:iterate_params(_) local i=0;return function() i=i+1;return profiles[i] end end
 function e:get_data(_) return self.selected end
 function e:set_data(_,p) self.selected=p end
 function e:get_subject() return d end
 return e
end
local sbc={name="a2dp-sink-sbc",description="SBC",available="yes"}
local ldac={name="a2dp-sink",description="Translated (codec LDAC)",available="yes"}
local hfp={name="headset-head-unit-msbc"}
local e=event({sbc,ldac});hook.execute(e);assert(e.selected==ldac)
e=event({sbc,ldac},hfp);hook.execute(e);assert(e.selected==hfp)
e=event({sbc,ldac},sbc);hook.execute(e);assert(e.selected==sbc)
e=event({sbc,ldac},nil,"alsa");hook.execute(e);assert(e.selected==nil)
ldac.available="no";e=event({sbc,ldac});hook.execute(e);assert(e.selected==sbc)
e=event({{name="a2dp-sink",description="Unknown new codec"}});hook.execute(e);assert(e.selected==nil)
print("6 WirePlumber policy cases passed")
