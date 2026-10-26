/* SPDX-License-Identifier: LGPL-2.1-or-later
 *
 * FAST LAUNCH section of fp6libcamerasrc (included by fp6camera.c, like the
 * MANUAL CONTROLS and PHASE 2 sections). Camera fast launch,
 * results/feature-push-20261024/camera/fast-launch/README.md.
 *
 * Stored auto-exposure state: when a stream stops, the sensor's raw
 * EXPOSURE / ANALOGUE_GAIN codes (what the soft IPA's AGC last wrote) are
 * saved per camera in $XDG_CACHE_HOME/fp6-camera/; before the next start
 * they are written back to the sensor subdevice if it still holds its driver
 * defaults, i.e. on the first use after a boot. The AGC starts from the
 * sensor's current values, so the first frames are already exposed for the
 * last scene instead of the driver default. Within one boot the kernel
 * keeps the values by itself. The work is in Rust (fastlaunch/lib.rs).
 *
 * FP6_CAMERA_AE_SEED=0 disables it (measurements).
 */

int fp6fl_seed (const char *libcamera_id, const char *state_dir);
int fp6fl_store (const char *libcamera_id, const char *state_dir);

static gboolean
fp6fl_enabled (void)
{
  const gchar *v = g_getenv ("FP6_CAMERA_AE_SEED");
  return !(v && g_strcmp0 (v, "0") == 0);
}

static gchar *
fp6fl_state_dir (void)
{
  return g_build_filename (g_get_user_cache_dir (), "fp6-camera", NULL);
}

static gchar *
fp6fl_camera_id (Fp6LibcameraSrc * self)
{
  gchar *id = NULL;

  if (self->cam)
    g_object_get (self->cam, "camera-name", &id, NULL);
  return id;
}

/* NULL -> READY, after lsrc_build, before libcamera opens the camera */
static void
fp6fl_open (Fp6LibcameraSrc * self)
{
  gchar *id, *dir;
  gint r;

  if (!fp6fl_enabled () || !(id = fp6fl_camera_id (self)))
    return;
  dir = fp6fl_state_dir ();
  r = fp6fl_seed (id, dir);
  if (r > 0)
    GST_INFO_OBJECT (self, "fast launch: sensor exposure/gain seeded from the "
        "last session (%s)", id);
  else if (r < 0)
    GST_DEBUG_OBJECT (self, "fast launch: no AE seed for %s", id);
  g_free (dir);
  g_free (id);
}

/* PAUSED -> READY, after libcamera stopped streaming */
static void
fp6fl_close (Fp6LibcameraSrc * self)
{
  gchar *id, *dir;
  gint64 exposure_us = 0;
  gint iso = 0;

  if (!fp6fl_enabled () || !(id = fp6fl_camera_id (self)))
    return;
  /* MANUAL CONTROLS: a manual shutter/ISO is not an AE state to start from */
  g_object_get (self, "exposure-us", &exposure_us, "iso", &iso, NULL);
  if (exposure_us == 0 && iso == 0) {
    dir = fp6fl_state_dir ();
    if (fp6fl_store (id, dir) > 0)
      GST_DEBUG_OBJECT (self, "fast launch: AE state of %s stored", id);
    g_free (dir);
  }
  g_free (id);
}
