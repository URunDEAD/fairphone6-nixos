# The release counter baked into the system (/etc/fp6-release, About).
# Bump `release` for every published update: fp6-release refuses to publish
# a release that is not newer than the channel's current one, and phones
# only offer a manifest whose release is higher than the running one.
{
  release = 2;
  channel = "kernel6";
}
