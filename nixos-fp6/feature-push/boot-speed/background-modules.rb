# FP6 boot-speed (nixos-fp6/feature-push/boot-speed/boot-speed-fp6.nix).
#
# Loads some stage-1 kernel modules in the background instead of in the
# blocking Tasks::Modules loop.
#
# spi_geni_qcom: loading it registers the GENI SPI controllers, and the
# controller probe binds the built-in eswin_eph861x touchscreen on spi0.0.
# That probe sleeps msleep(5000) ("wait ic boot done") after powering the
# touch IC, so `modprobe spi_geni_qcom` blocked stage-1 for ~5.4 s on kernel6
# (~9 s on kernel5). In the background the probe still finishes during stage-1
# (~6.7 s after power-on), before stage-2 starts. Lomiri, fp6-lomiri-powerd
# (lp_mode, double-tap) and the IMU on spi1.0 therefore see the same devices
# as before.
#
# Stage-1 udev must not load the same module by its modalias while the
# background modprobe is still in the kernel: the second init_module waits
# until the first one returns, and `udevadm settle` (Tasks::UDev) would block
# on it. boot-speed-fp6.nix therefore adds a stage-1 only
# /etc/modprobe.d blacklist entry. A blacklist applies to alias loads only.
# The explicit `modprobe <name>` here still loads the module.
#
# This file wraps Tasks::Modules#run with alias_method (and does not replace
# it), so it composes with the boot-splash wrapper in any load order.
class Tasks::Modules
  FP6_BACKGROUND_MODULES = @backgroundModules@

  alias_method :fp6_run_without_background_modules, :run

  def run()
    background = @modules.select { |mod| FP6_BACKGROUND_MODULES.include?(mod) }
    @modules = @modules.reject { |mod| FP6_BACKGROUND_MODULES.include?(mod) }

    fp6_run_without_background_modules()

    background.each do |mod|
      begin
        pid = System.spawn("modprobe", mod)
        $logger.info("FP6 boot-speed: loading #{mod} in the background (pid #{pid}).")
      rescue => e
        $logger.warn("FP6 boot-speed: could not spawn modprobe #{mod} (#{e.inspect}); loading it in the foreground.")
        begin
          System.run("modprobe", mod)
        rescue System::CommandError
          $logger.warn("Kernel module #{mod} failed to load.")
        end
      end
    end
  end
end
