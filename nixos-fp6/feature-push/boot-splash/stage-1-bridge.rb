# FP6 boot splash bridge frame (nixos-fp6/feature-push/boot-splash/boot-splash-fp6.nix).
#
# Paints the Plymouth theme's resting frame (the NixOS logo on #05070D, at
# Plymouth's position and size) into the msm fbdev emulation (/dev/fb0)
# as soon as the panel module has registered it in stage-1. fbcon is not mapped to VT1
# (fbcon=vc:2-6), so nothing else writes to fb0. The image stays on the panel
# through the rest of stage-1, switch_root, NixOS stage-2 activation and
# systemd start-up, until plymouthd takes the DRM master and draws the
# same frame. This replaces the dark gap that ran from ~1.2 s (DRM init) to
# Plymouth (~14-19 s).
#
# The Mobile NixOS lvgui splash is skipped on normal boots unless
# FP6_STAGE1_SPLASH is set. When it runs, it takes the DRM master
# and the bridge frame is painted again after it quits. The recovery menu
# (volume key held during stage-1) and the boot-error applet still run
# as before: they are separate applets that do not depend on the splash.
#
# All of this is best effort: any error is logged, and the boot continues.

module FP6BridgeFrame
  FRAME = "/etc/fp6-bridge-frame.bgrx"
  FB = "/dev/fb0"
  SYSFS = "/sys/class/graphics/fb0"
  WIDTH = @width@
  HEIGHT = @height@
  STRIDE = @stride@
  # Created by the Tasks::Modules wrapper below (in the initramfs root; /run
  # gets a tmpfs mounted over it later).
  MODULES_LOADED = "/.fp6-stage-1-modules-loaded"

  def self.read_sysfs(name)
    File.read("#{SYSFS}/#{name}").strip
  end

  def self.geometry_ok?()
    size = read_sysfs("virtual_size")
    stride = read_sysfs("stride").to_i
    bpp = read_sysfs("bits_per_pixel").to_i
    return true if size == "#{WIDTH},#{HEIGHT}" && stride == STRIDE && bpp == 32
    $logger.warn("FP6 boot splash: fb0 is #{size} stride #{stride} bpp #{bpp}, expected #{WIDTH},#{HEIGHT} stride #{STRIDE} bpp 32; not painting.")
    false
  rescue => e
    $logger.warn("FP6 boot splash: cannot read fb0 geometry (#{e.inspect}); not painting.")
    false
  end

  def self.paint(reason)
    return unless File.exist?(FRAME)
    unless File.exist?(FB)
      $logger.warn("FP6 boot splash: #{FB} is missing; bridge frame not painted (#{reason}).")
      return
    end
    return unless geometry_ok?
    File.open(FRAME, "r") do |src|
      File.open(FB, "r+") do |dst|
        while chunk = src.read(1048576)
          dst.write(chunk)
        end
      end
    end
    $logger.info("FP6 boot splash: bridge frame painted on #{FB} (#{reason}).")
  rescue => e
    $logger.warn("FP6 boot splash: could not paint the bridge frame (#{reason}): #{e.inspect}")
  end
end

FP6_STAGE1_SPLASH = @stage1Splash@

class Tasks::Modules
  alias_method :fp6_run_before_bridge_marker, :run

  def run()
    begin
      fp6_run_before_bridge_marker()
    ensure
      begin
        File.open(FP6BridgeFrame::MODULES_LOADED, "w") { |f| f.write("1") }
      rescue => e
        $logger.warn("FP6 boot splash: could not create #{FP6BridgeFrame::MODULES_LOADED}: #{e.inspect}")
      end
    end
  end
end

# Runs once the stage-1 modules (panel -> msm fbdev) are loaded and /sys is
# mounted. Both are always true early in stage-1, so this task cannot hang the
# boot.
class Tasks::FP6PaintBridgeFrame < SingletonTask
  def initialize()
    add_dependency(:Mount, "/dev")
    add_dependency(:Mount, "/sys")
    add_dependency(:Files, FP6BridgeFrame::MODULES_LOADED)
  end

  def run()
    # fb0 is registered while the panel module binds msm. Wait at most 1 s.
    20.times do
      break if File.exist?(FP6BridgeFrame::FB)
      sleep(0.05)
    end
    FP6BridgeFrame.paint("stage-1")
  end

  def ux_priority()
    -90
  end
end

class Tasks::Splash
  alias_method :fp6_run_lvgui_splash, :run
  alias_method :fp6_quit_lvgui_splash, :quit

  def run()
    if FP6_STAGE1_SPLASH
      fp6_run_lvgui_splash()
    else
      $logger.info("FP6 boot splash: lvgui stage-1 splash skipped (bridge frame shown instead).")
    end
  end

  def quit(reason, sticky: nil)
    had_splash = !@pid.nil?
    fp6_quit_lvgui_splash(reason, sticky: sticky)
    FP6BridgeFrame.paint("after the stage-1 splash") if had_splash
  end
end
