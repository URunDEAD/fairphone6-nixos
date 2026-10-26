# Keep the existing system available while testing NixOS from an image file.
# The battery driver returns EAGAIN for synthetic events until its firmware
# is ready. Power supplies are not needed in stage 1; stage 2 coldplugs them.
class Tasks::UDev
  def udevadm(*args)
    args << "--subsystem-nomatch=power_supply" if args.first == "trigger"
    System.run("udevadm", *args)
  end
end

class Tasks::FP6RootImage < SingletonTask
  def initialize()
    add_dependency(:Target, :Environment)
    add_dependency(:Devices, "/dev/disk/by-partlabel/userdata")
    add_dependency(:Mount, "/run")
    add_dependency(:Task, Tasks::UDev.instance)
    Targets[:SwitchRoot].add_dependency(:Task, self)
  end

  def run()
    System.run("losetup", "--find", "--partscan", "--sector-size", "4096", "/dev/disk/by-partlabel/userdata")
    System.run("udevadm", "trigger", "--action=add", "--subsystem-match=block")
    System.run("udevadm", "settle")
    backing = "/dev/disk/by-uuid/1df42e25-95a9-4ef4-bcb9-76903ab1261a"
    100.times do
      break if File.exist?(backing)
      sleep(0.1)
    end
    FileUtils.mkdir_p("/run/nura")
    System.run("mount", "-t", "ext4", "-o", "rw", backing, "/run/nura")
    System.run("losetup", "-f", "/run/nura/nixos-fp6.img")
    System.run("udevadm", "trigger", "--action=add", "--subsystem-match=block")
    System.run("udevadm", "settle")
  end
end
