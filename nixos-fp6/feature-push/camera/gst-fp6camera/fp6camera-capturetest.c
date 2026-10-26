/* SPDX-License-Identifier: LGPL-2.1-or-later
 *
 * fp6-capture: drive fp6libcamerasrc's PHASE 2 features from the command
 * line (live camera; the camera must not be in use).
 *
 *   fp6-capture [-d primary|secondary] [-u] [-w WARMUP_S] [-r ROTATION]
 *               [-R DNG_PATH]                    RAW: one DNG of the next frame
 *               [-j KIND -o PATH [-p PARAMS] [-s STOP_AFTER_S]]   a capture job
 *
 * KIND/PARAMS are those of the element's "job-start" signal (e.g. hdr,
 * night, bracket, timelapse, panorama, document; PARAMS a GstStructure
 * string such as "p, ev=2.0, frames=3"). -s sends "job-stop" after the
 * given time (timelapse/panorama). Prints the progress and the result.
 */
#include <gst/gst.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

static gboolean
has_signal (GstElement * e, const gchar * name)
{
  return g_signal_lookup (name, G_OBJECT_TYPE (e)) != 0;
}

int
main (int argc, char **argv)
{
  const gchar *device = "primary", *raw = NULL, *kind = NULL, *out = NULL;
  const gchar *params = "";
  gdouble warmup = 3, stop_after = 0;
  gint rotation = 0, opt, ret = 0;
  gboolean ultrawide = FALSE;
  GstElement *pipe, *src;

  while ((opt = getopt (argc, argv, "d:uw:r:R:j:o:p:s:")) != -1) {
    switch (opt) {
      case 'd': device = optarg; break;
      case 'u': ultrawide = TRUE; break;
      case 'w': warmup = g_ascii_strtod (optarg, NULL); break;
      case 'r': rotation = atoi (optarg); break;
      case 'R': raw = optarg; break;
      case 'j': kind = optarg; break;
      case 'o': out = optarg; break;
      case 'p': params = optarg; break;
      case 's': stop_after = g_ascii_strtod (optarg, NULL); break;
      default:
        fprintf (stderr, "see the source header for usage\n");
        return 2;
    }
  }
#ifdef FP6_TUNING_DIR
  if (!g_getenv ("LIBCAMERA_IPA_CONFIG_PATH"))
    g_setenv ("LIBCAMERA_IPA_CONFIG_PATH", FP6_TUNING_DIR, FALSE);
#endif
  gst_init (&argc, &argv);
  pipe = gst_parse_launch ("fp6libcamerasrc name=src ! fakesink sync=false",
      NULL);
  if (!pipe) {
    fprintf (stderr, "cannot create the pipeline (plugin path?)\n");
    return 1;
  }
  src = gst_bin_get_by_name (GST_BIN (pipe), "src");
  g_object_set (src, "device", device, "ultrawide", ultrawide, NULL);
  if (raw)
    g_object_set (src, "raw", TRUE, NULL);
  if (gst_element_set_state (pipe, GST_STATE_PLAYING) ==
      GST_STATE_CHANGE_FAILURE) {
    fprintf (stderr, "cannot start the camera\n");
    return 1;
  }
  g_usleep ((gulong) (warmup * G_USEC_PER_SEC));

  if (raw) {
    gboolean ok = FALSE, active = FALSE;
    gchar *fmt = NULL, *sensor = NULL;
    gint st = 0;
    g_object_get (src, "raw-active", &active, "raw-format", &fmt, "sensor",
        &sensor, NULL);
    printf ("raw: active %d, format %s, sensor %s\n", active,
        fmt ? fmt : "-", sensor ? sensor : "-");
    g_free (fmt);
    g_free (sensor);
    g_signal_emit_by_name (src, "raw-capture", raw, rotation, &ok);
    if (!ok) {
      fprintf (stderr, "raw-capture refused\n");
      ret = 1;
    }
    for (gint i = 0; ok && i < 300; i++) {
      g_object_get (src, "raw-state", &st, NULL);
      if (st == 3 || st == 4)
        break;
      g_usleep (50000);
    }
    if (ok) {
      gchar *res = NULL, *err = NULL;
      g_object_get (src, "raw-result", &res, "raw-error", &err, NULL);
      printf ("raw: state %d result %s error %s\n", st, res ? res : "-",
          err ? err : "-");
      ret = st == 3 ? 0 : 1;
      g_free (res);
      g_free (err);
    }
  }

  if (kind && out) {
    gboolean ok = FALSE;
    gint st = 0, last = -1;
    gdouble prog = 0, last_prog = -1;
    gint64 t0 = g_get_monotonic_time ();
    gboolean stopped = FALSE;

    if (!has_signal (src, "job-start")) {
      fprintf (stderr, "element has no job-start\n");
      return 1;
    }
    g_signal_emit_by_name (src, "job-start", kind, out, rotation, 92,
        params, &ok);
    if (!ok) {
      fprintf (stderr, "job-start refused\n");
      ret = 1;
    }
    while (ok) {
      gint frames = 0;
      g_object_get (src, "job-state", &st, "job-progress", &prog,
          "job-frames", &frames, NULL);
      if (st != last || prog - last_prog >= 0.1) {
        printf ("job: state %d progress %.2f frames %d (%.1f s)\n", st, prog,
            frames, (g_get_monotonic_time () - t0) / 1e6);
        fflush (stdout);
        last = st;
        last_prog = prog;
      }
      if (st == 3 || st == 4)
        break;
      if (!stopped && stop_after > 0
          && g_get_monotonic_time () - t0 > stop_after * G_USEC_PER_SEC) {
        g_signal_emit_by_name (src, "job-stop");
        stopped = TRUE;
      }
      if (g_get_monotonic_time () - t0 > 600 * G_USEC_PER_SEC) {
        fprintf (stderr, "job timed out\n");
        break;
      }
      g_usleep (100000);
    }
    if (ok) {
      gchar *res = NULL, *err = NULL;
      g_object_get (src, "job-result", &res, "job-error", &err, NULL);
      printf ("job: state %d result %s error %s (%.1f s)\n", st,
          res ? res : "-", err ? err : "-",
          (g_get_monotonic_time () - t0) / 1e6);
      ret = st == 3 ? 0 : 1;
      g_free (res);
      g_free (err);
    }
  }

  gst_element_set_state (pipe, GST_STATE_NULL);
  gst_object_unref (src);
  gst_object_unref (pipe);
  return ret;
}
