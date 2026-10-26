/* fp6camera self test.
 *
 *   fp6camera-selftest
 *       Offline (build time): videotestsrc instead of the camera, the "LED" is
 *       a temp file from FP6_SELFTEST_LED / FP6_CAMERA_CONFIG, image to
 *       FP6_SELFTEST_IMG.
 *   fp6camera-selftest --photo DEVICE OUT.jpg [--flash]
 *       Live: one still through camerabin + fp6camerabinsrc + fp6libcamerasrc,
 *       exactly the elements lomiri-camera-app uses.  --flash uses the
 *       plugin's bounded LED pulse (config [flash]).
 *   fp6camera-selftest --video DEVICE OUT.mp4 SECONDS [WxH]
 *       Live: camerabin MP4/H.264/AAC recording for SECONDS (1..20).
 *
 * DEVICE = primary | secondary | literal libcamera id.
 */
#define GST_USE_UNSTABLE_API 1
#include <gst/gst.h>
#include <gst/interfaces/photography.h>
#include <gst/pbutils/encoding-profile.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static const char *led;

static int
led_value (void)
{
  char buf[32] = { 0 };
  FILE *f;
  if (!led)
    return -1;
  f = fopen (led, "r");
  if (!f)
    return -1;
  if (!fgets (buf, sizeof buf, f))
    buf[0] = 0;
  fclose (f);
  return atoi (buf);
}

#define CHECK(c) do { if (!(c)) { g_printerr ("FAIL %s:%d %s\n", __FILE__, __LINE__, #c); exit (1); } } while (0)

static gboolean
wait_ready (GstElement * src, gint64 ms)
{
  gint64 end = g_get_monotonic_time () + ms * 1000;
  gboolean ready = FALSE;
  while (g_get_monotonic_time () < end) {
    g_object_get (src, "ready-for-capture", &ready, NULL);
    if (ready)
      return TRUE;
    g_usleep (10000);
  }
  return FALSE;
}

/* Pump the bus until an element message `name` (or error/timeout); tracks the
 * LED peak meanwhile. */
static gboolean
wait_message (GstElement * bin, const char *name, gint64 ms, int *ledpeak)
{
  GstBus *bus = gst_element_get_bus (bin);
  gint64 end = g_get_monotonic_time () + ms * 1000;
  gboolean done = FALSE;

  while (!done && g_get_monotonic_time () < end) {
    GstMessage *m = gst_bus_timed_pop (bus, 10 * GST_MSECOND);
    int v = led_value ();
    if (ledpeak && v > *ledpeak)
      *ledpeak = v;
    if (!m)
      continue;
    if (GST_MESSAGE_TYPE (m) == GST_MESSAGE_ERROR) {
      GError *e;
      gchar *dbg;
      gst_message_parse_error (m, &e, &dbg);
      g_printerr ("error from %s: %s\n%s\n", GST_OBJECT_NAME (m->src),
          e->message, dbg ? dbg : "");
      exit (1);
    }
    if (GST_MESSAGE_TYPE (m) == GST_MESSAGE_ELEMENT &&
        gst_structure_has_name (gst_message_get_structure (m), name))
      done = TRUE;
    gst_message_unref (m);
  }
  gst_object_unref (bus);
  return done;
}

static void
tune_encoder (GstBin * bin, GstBin * sub, GstElement * el, gpointer u)
{
  GstElementFactory *f = gst_element_get_factory (el);
  if (f && g_strcmp0 (GST_OBJECT_NAME (f), "x264enc") == 0) {
    /* same settings the patched Qt camerabin applies */
    g_object_set (el, "speed-preset", 1, NULL);
    gst_util_set_object_arg (G_OBJECT (el), "tune", "zerolatency");
    g_object_set (el, "bitrate", 8000, NULL);
  }
  if (f && (strstr (GST_OBJECT_NAME (f), "enc") || strstr (GST_OBJECT_NAME (f), "mux")))
    g_print ("element %s\n", GST_OBJECT_NAME (f));
}

static GstElement *
make_camerabin (GstElement * vs, GstElement ** src_out, int mode)
{
  GstElement *bin = gst_element_factory_make ("camerabin", NULL);
  GstElement *src = gst_element_factory_make ("fp6camerabinsrc", NULL);
  GstElement *sink = gst_element_factory_make ("fakesink", NULL);
  CHECK (bin);
  CHECK (src);
  CHECK (vs);
  CHECK (sink);
  g_object_set (sink, "sync", FALSE, NULL);
  g_object_set (src, "video-source", vs, NULL);
  g_object_set (bin, "camera-source", src, "viewfinder-sink", sink,
      "mode", mode, NULL);
  g_signal_connect (bin, "deep-element-added", G_CALLBACK (tune_encoder),
      NULL);
  *src_out = src;
  return bin;
}

static int
live_photo (const char *device, const char *out, gboolean flash)
{
  GstElement *vs = gst_element_factory_make ("fp6libcamerasrc", NULL), *src;
  GstElement *bin;
  int peak = 0;
  gint64 t0;

  CHECK (vs);
  g_object_set (vs, "device", device, NULL);
  bin = make_camerabin (vs, &src, 1);
  g_object_set (bin, "location", out, NULL);
  gst_photography_set_flash_mode (GST_PHOTOGRAPHY (src), flash ?
      GST_PHOTOGRAPHY_FLASH_MODE_ON : GST_PHOTOGRAPHY_FLASH_MODE_OFF);
  t0 = g_get_monotonic_time ();
  CHECK (gst_element_set_state (bin, GST_STATE_PLAYING) !=
      GST_STATE_CHANGE_FAILURE);
  CHECK (wait_ready (src, 15000));
  g_print ("ready after %.2f s\n", (g_get_monotonic_time () - t0) / 1e6);
  {
    /* AE/AWB convergence and the continuous AF sweep (~2.5-3.5 s) */
    const char *e = g_getenv ("FP6_SELFTEST_SETTLE_MS");
    g_usleep ((gulong) (e ? atoi (e) : 3500) * 1000);
  }
  g_signal_emit_by_name (bin, "start-capture", NULL);
  CHECK (wait_message (bin, "image-done", 10000, &peak));
  g_print ("image-done after %.2f s, led peak %d\n",
      (g_get_monotonic_time () - t0) / 1e6, peak);
  g_usleep (300000);
  gst_element_set_state (bin, GST_STATE_NULL);
  gst_object_unref (bin);
  if (flash)
    CHECK (led_value () == 0);
  CHECK (g_file_test (out, G_FILE_TEST_EXISTS));
  g_print ("PASS photo %s -> %s\n", device, out);
  return 0;
}

static int
live_video (const char *device, const char *out, int seconds, const char *size)
{
  GstElement *vs = gst_element_factory_make ("fp6libcamerasrc", NULL), *src;
  GstElement *bin;
  GstEncodingProfile *prof;
  GstCaps *c;
  int w = 1440, h = 1080;
  gint64 t0;

  CHECK (vs);
  if (size)
    CHECK (sscanf (size, "%dx%d", &w, &h) == 2);
  seconds = CLAMP (seconds, 1, 20);
  g_object_set (vs, "device", device, NULL);
  bin = make_camerabin (vs, &src, 2);
  prof = gst_encoding_profile_from_string
      ("video/quicktime,variant=iso:video/x-h264:audio/mpeg,mpegversion=4");
  CHECK (prof);
  c = gst_caps_new_simple ("video/x-raw", "width", G_TYPE_INT, w,
      "height", G_TYPE_INT, h, NULL);
  g_object_set (bin, "video-profile", prof, "location", out,
      "video-capture-caps", c, NULL);
  gst_caps_unref (c);
  gst_encoding_profile_unref (prof);
  t0 = g_get_monotonic_time ();
  CHECK (gst_element_set_state (bin, GST_STATE_PLAYING) !=
      GST_STATE_CHANGE_FAILURE);
  CHECK (wait_ready (src, 15000));
  g_print ("ready after %.2f s\n", (g_get_monotonic_time () - t0) / 1e6);
  g_signal_emit_by_name (bin, "start-capture", NULL);
  g_usleep ((gulong) seconds * G_USEC_PER_SEC);
  g_signal_emit_by_name (bin, "stop-capture", NULL);
  CHECK (wait_message (bin, "video-done", 15000, NULL));
  gst_element_set_state (bin, GST_STATE_NULL);
  gst_object_unref (bin);
  CHECK (g_file_test (out, G_FILE_TEST_EXISTS));
  g_print ("PASS video %s %dx%d %ds -> %s\n", device, w, h, seconds, out);
  return 0;
}

static int
offline (void)
{
  GstElement *probe, *bin, *src, *vs;
  gint angle = -1, maxled = 0;
  const char *img = g_getenv ("FP6_SELFTEST_IMG");

  CHECK (led && img);

  /* 1. Qt-style metadata probe: bare instance, READY, camera-device 1 */
  probe = gst_element_factory_make ("fp6camerabinsrc", NULL);
  CHECK (probe);
  CHECK (GST_IS_PHOTOGRAPHY (probe));
  CHECK (gst_element_set_state (probe, GST_STATE_READY) ==
      GST_STATE_CHANGE_SUCCESS);
  g_object_set (probe, "camera-device", 1, NULL);
  g_object_get (probe, "sensor-mount-angle", &angle, NULL);
  CHECK (angle == 270);
  g_object_set (probe, "camera-device", 0, NULL);
  g_object_get (probe, "sensor-mount-angle", &angle, NULL);
  CHECK (angle == 270);
  gst_element_set_state (probe, GST_STATE_NULL);
  gst_object_unref (probe);
  g_print ("probe ok\n");

  /* 2. fp6libcamerasrc exists with a device property */
  vs = gst_element_factory_make ("fp6libcamerasrc", NULL);
  CHECK (vs);
  g_object_set (vs, "device", "secondary", NULL);
  gst_object_unref (vs);

  /* 3. camerabin still capture with flash ON */
  vs = gst_parse_bin_from_description ("videotestsrc is-live=true ! "
      "video/x-raw,format=BGRx,width=640,height=480,framerate=30/1", TRUE,
      NULL);
  bin = make_camerabin (vs, &src, 1);
  g_object_set (bin, "audio-source",
      gst_element_factory_make ("audiotestsrc", NULL), "location", img, NULL);
  gst_photography_set_flash_mode (GST_PHOTOGRAPHY (src),
      GST_PHOTOGRAPHY_FLASH_MODE_ON);
  CHECK (gst_element_set_state (bin, GST_STATE_PLAYING) !=
      GST_STATE_CHANGE_FAILURE);
  CHECK (gst_element_get_state (bin, NULL, NULL, 10 * GST_SECOND) ==
      GST_STATE_CHANGE_SUCCESS);
  CHECK (wait_ready (src, 5000));
  CHECK (led_value () <= 0);
  g_signal_emit_by_name (bin, "start-capture", NULL);
  CHECK (wait_message (bin, "image-done", 10000, &maxled));
  CHECK (maxled == 160);
  g_usleep (300000);
  CHECK (led_value () == 0);
  CHECK (g_file_test (img, G_FILE_TEST_EXISTS));
  g_print ("flash capture ok (led peak %d)\n", maxled);

  /* 4. video light: FILL_IN in video mode -> on; OFF -> off */
  g_object_set (bin, "mode", 2, NULL);
  gst_photography_set_flash_mode (GST_PHOTOGRAPHY (src),
      GST_PHOTOGRAPHY_FLASH_MODE_FILL_IN);
  g_usleep (100000);
  CHECK (led_value () == 160);
  g_object_set (bin, "mode", 1, NULL);
  g_usleep (100000);
  CHECK (led_value () == 0);
  g_object_set (bin, "mode", 2, NULL);
  g_usleep (100000);
  CHECK (led_value () == 160);
  gst_photography_set_flash_mode (GST_PHOTOGRAPHY (src),
      GST_PHOTOGRAPHY_FLASH_MODE_OFF);
  g_usleep (100000);
  CHECK (led_value () == 0);
  /* stopping the pipeline must also switch it off */
  gst_photography_set_flash_mode (GST_PHOTOGRAPHY (src),
      GST_PHOTOGRAPHY_FLASH_MODE_FILL_IN);
  g_usleep (100000);
  CHECK (led_value () == 160);
  gst_element_set_state (bin, GST_STATE_NULL);
  CHECK (led_value () == 0);
  g_print ("video light ok\n");

  gst_object_unref (bin);
  g_print ("PASS\n");
  return 0;
}

int
main (int argc, char **argv)
{
  gst_init (&argc, &argv);
  led = g_getenv ("FP6_SELFTEST_LED");
  if (argc >= 4 && strcmp (argv[1], "--photo") == 0) {
    if (!led)
      led = "/sys/class/leds/white:flash/brightness";
    return live_photo (argv[2], argv[3], argc >= 5
        && strcmp (argv[4], "--flash") == 0);
  }
  if (argc >= 5 && strcmp (argv[1], "--video") == 0)
    return live_video (argv[2], argv[3], atoi (argv[4]),
        argc >= 6 ? argv[5] : NULL);
  if (argc == 1)
    return offline ();
  g_printerr ("usage: %s [--photo DEV OUT [--flash] | --video DEV OUT SECONDS [WxH]]\n",
      argv[0]);
  return 2;
}
