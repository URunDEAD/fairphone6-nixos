/* SPDX-License-Identifier: LGPL-2.1-or-later
 *
 * fp6-long-exposure: drive fp6libcamerasrc's manual controls from the
 * command line (live camera; the camera must not be in use).
 *
 *   fp6-long-exposure [-d primary|secondary] [-u] [-e EXPOSURE_US] [-E US] [-i ISO]
 *                     [-k KELVIN] [-f FOCUS] [-t TOTAL_US | -b BULB_S]
 *                     [-m sum|mean] [-r ROTATION] [-o DIR] [-w WARMUP_S]
 *
 * Without -t/-b it only streams for the warm-up and prints the readouts
 * (a check of the manual exposure path). With -t it takes a stacked long
 * exposure of TOTAL_US; -b stops a bulb exposure after BULB_S seconds.
 */
#include <gst/gst.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

static void
readout (GstElement * src, const gchar * tag)
{
  gint64 exp = 0, frame = 0, maxexp = 0;
  gint iso = 0, k = 0, af = 0, st = 0, n = 0, target = 0;
  gfloat focus = 0;
  gchar *lens = NULL;

  g_object_get (src, "current-exposure-us", &exp, "current-iso", &iso,
      "current-kelvin", &k, "current-focus", &focus, "frame-duration-us",
      &frame, "max-exposure-us", &maxexp, "af-state", &af, "active-lens",
      &lens, "stack-state", &st, "stack-frames", &n, "stack-target-frames",
      &target, NULL);
  printf ("%s: exposure %" G_GINT64_FORMAT " us, ISO %d, %d K, focus %.3f, "
      "frame %" G_GINT64_FORMAT " us, max exposure %" G_GINT64_FORMAT
      " us, af-state %d, lens %s, stack %d (%d/%d)\n", tag, exp, iso, k,
      focus, frame, maxexp, af, lens ? lens : "?", st, n, target);
  fflush (stdout);
  g_free (lens);
}

int
main (int argc, char **argv)
{
  const gchar *device = "primary", *dir = "/tmp", *mode = "sum";
  gint64 exposure = 0, total = -1, exposure2 = -1;
  gint iso = 0, kelvin = 0, rotation = 0, opt;
  gdouble focus = -1, bulb = 0, warmup = 3;
  gboolean ultrawide = FALSE;
  GstElement *pipe, *src;
  gint ret = 0;

  while ((opt = getopt (argc, argv, "d:ue:E:i:k:f:t:b:m:r:o:w:")) != -1) {
    switch (opt) {
      case 'd': device = optarg; break;
      case 'u': ultrawide = TRUE; break;
      case 'e': exposure = g_ascii_strtoll (optarg, NULL, 10); break;
      case 'E': exposure2 = g_ascii_strtoll (optarg, NULL, 10); break;
      case 'i': iso = atoi (optarg); break;
      case 'k': kelvin = atoi (optarg); break;
      case 'f': focus = g_ascii_strtod (optarg, NULL); break;
      case 't': total = g_ascii_strtoll (optarg, NULL, 10); break;
      case 'b': bulb = g_ascii_strtod (optarg, NULL); total = 0; break;
      case 'm': mode = optarg; break;
      case 'r': rotation = atoi (optarg); break;
      case 'o': dir = optarg; break;
      case 'w': warmup = g_ascii_strtod (optarg, NULL); break;
      default:
        fprintf (stderr, "see the source header for usage\n");
        return 2;
    }
  }
#ifdef FP6_TUNING_DIR
  /* the softISP tuning files of the ultrawide/front sensors */
  if (!g_getenv ("LIBCAMERA_IPA_CONFIG_PATH"))
    g_setenv ("LIBCAMERA_IPA_CONFIG_PATH", FP6_TUNING_DIR, FALSE);
#endif
  gst_init (&argc, &argv);
  pipe = gst_parse_launch ("fp6libcamerasrc name=src ! fakesink sync=false",
      NULL);
  if (!pipe) {
    fprintf (stderr, "cannot create fp6libcamerasrc\n");
    return 1;
  }
  src = gst_bin_get_by_name (GST_BIN (pipe), "src");
  g_object_set (src, "device", device, "ultrawide", ultrawide,
      "exposure-us", exposure, "iso", iso, "wb-kelvin", kelvin,
      "focus", (gfloat) focus, "analysis", 1u, NULL);
  if (gst_element_set_state (pipe, GST_STATE_PLAYING) ==
      GST_STATE_CHANGE_FAILURE) {
    fprintf (stderr, "cannot start the camera\n");
    return 1;
  }
  for (gint i = 0; i < (gint) (warmup * 2); i++) {
    g_usleep (500000);
    readout (src, "live");
  }
  if (exposure2 >= 0) {
    /* change the exposure while streaming (-E) */
    g_object_set (src, "exposure-us", exposure2, NULL);
    for (gint i = 0; i < (gint) (warmup * 2); i++) {
      g_usleep (500000);
      readout (src, "live2");
    }
  }

  if (total >= 0) {
    gchar *path = g_strdup_printf ("%s/fp6-long-exposure-%" G_GINT64_FORMAT
        ".jpg", dir, g_get_real_time () / 1000);
    gboolean ok = FALSE;
    gint st = 0;
    gint64 start = g_get_monotonic_time ();

    g_signal_emit_by_name (src, "stack-start", path, total,
        g_strcmp0 (mode, "mean") == 0 ? 1 : 0, FALSE, rotation, 95, &ok);
    if (!ok) {
      fprintf (stderr, "stack-start refused\n");
      ret = 1;
    }
    while (ok) {
      g_usleep (500000);
      readout (src, "stack");
      g_object_get (src, "stack-state", &st, NULL);
      if (bulb > 0 && st == 2
          && g_get_monotonic_time () - start > bulb * G_USEC_PER_SEC)
        g_signal_emit_by_name (src, "stack-stop");
      if (st == 6 || st == 7 || st == 0)
        break;
      if (g_get_monotonic_time () - start > (total / 1000000 + 120) *
          G_USEC_PER_SEC) {
        fprintf (stderr, "timeout\n");
        g_signal_emit_by_name (src, "stack-cancel");
        ret = 1;
        break;
      }
    }
    if (st == 6) {
      gchar *res = NULL;
      g_object_get (src, "stack-result", &res, NULL);
      printf ("saved %s\n", res);
      g_free (res);
    } else if (st == 7) {
      gchar *err = NULL;
      g_object_get (src, "stack-error", &err, NULL);
      printf ("failed: %s\n", err);
      g_free (err);
      ret = 1;
    }
    g_free (path);
  }
  gst_element_set_state (pipe, GST_STATE_NULL);
  gst_object_unref (src);
  gst_object_unref (pipe);
  return ret;
}
