/* SPDX-License-Identifier: LGPL-2.1-or-later
 *
 * fp6camera: glue that lets Qt5 QtMultimedia's camerabin backend (used by
 * lomiri-camera-app) drive the FP6 libcamera cameras.
 *
 * Qt5 camerabin only knows two ways to find cameras: scanning /dev/video*
 * (useless here: CAMSS nodes are raw Bayer RDI queues) or a camera source
 * element with a "camera-device" property (the Ubuntu Touch droidcamsrc
 * model: "primary" = back, "secondary" = front, plus "sensor-mount-angle").
 *
 * Elements:
 *   fp6camerabinsrc  runtime subclass of camerabin's wrappercamerabinsrc that
 *                    adds camera-device / sensor-mount-angle, implements
 *                    GstPhotography flash (bounded LED pulse around a still
 *                    capture, continuous "video light" in video mode) and
 *                    reports autofocus completion.
 *   fp6libcamerasrc  bin: libcamerasrc (fixed per-camera stream size, so the
 *                    camera is never reconfigured while running) ->
 *                    videocrop (digital zoom) -> videoconvertscale ->
 *                    capsfilter(list of offered sizes) [-> videoflip when
 *                    the camera's "flip" key is set (default none; for a
 *                    sensor that reads out mirrored; applies to viewfinder,
 *                    stills and video alike)].
 *                    Property "device" = primary | secondary | libcamera id.
 *                    Property "zoom": wrappercamerabinsrc uses a source's own
 *                    "zoom" instead of its digitalzoom bin.  That bin sets
 *                    videocrop's crop from the application thread while the
 *                    streaming thread is inside transform_frame, so videocrop
 *                    recomputes the crop for a buffer that was allocated for
 *                    the old output size and memcpy runs off the end
 *                    (SIGSEGV in gst_video_crop_transform_frame on fast
 *                    pinch-zoom).  Here the crop is applied from a buffer
 *                    probe on videocrop's sink pad, i.e. in the streaming
 *                    thread between buffers, so renegotiation, allocation and
 *                    the copy always agree.
 *
 * Configuration: GKeyFile at $FP6_CAMERA_CONFIG or /etc/fp6-camera/camera.conf;
 * compiled-in defaults are used for anything missing.
 */
#define GST_USE_UNSTABLE_API 1
#include <gst/gst.h>
#include <gst/basecamerabinsrc/gstbasecamerasrc.h>
#include <gst/interfaces/photography.h>
#include <gst/video/video.h>

#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>

GST_DEBUG_CATEGORY_STATIC (fp6cam_debug);
#define GST_CAT_DEFAULT fp6cam_debug

#ifndef FP6CAM_DEFAULT_CONFIG
#define FP6CAM_DEFAULT_CONFIG "/etc/fp6-camera/camera.conf"
#endif

/* ------------------------------------------------------------------------ */
/* Configuration                                                            */
/* ------------------------------------------------------------------------ */

typedef struct
{
  gchar *camera_name;           /* libcamera id */
  gint width, height;           /* fixed libcamera stream size */
  gchar *format;                /* GStreamer raw format, e.g. BGRx */
  gchar *sizes;                 /* "WxH;WxH;..." offered to camerabin */
  gint mount_angle;             /* Android orientation = 360 - libcamera Rotation */
  gchar *af_mode;               /* optional libcamerasrc af-mode nick */
  gchar *flip;                  /* videoflip method nick, "" / none = off */
  gboolean swap_rb;             /* relabel the stream with R and B swapped */
  gint scale_threads;
} Fp6CamEntry;

typedef struct
{
  Fp6CamEntry cam[2];           /* 0 = primary/back, 1 = secondary/front */
  gchar *led_path;
  gint led_brightness;
  gint preflash_ms;
  gint capture_hold_max_ms;
  gint video_light_max_s;
  gint first_frame_timeout_ms;
  gint max_restarts;
  gboolean auto_is_on;
} Fp6CamConfig;

static Fp6CamConfig *global_config;

static gchar *
kf_str (GKeyFile * kf, const gchar * grp, const gchar * key, const gchar * def)
{
  gchar *v = kf ? g_key_file_get_string (kf, grp, key, NULL) : NULL;
  if (v)
    g_strstrip (v);
  if (v && *v)
    return v;
  g_free (v);
  return g_strdup (def);
}

static gint
kf_int (GKeyFile * kf, const gchar * grp, const gchar * key, gint def,
    gint min, gint max)
{
  GError *err = NULL;
  gint v;

  if (!kf)
    return def;
  v = g_key_file_get_integer (kf, grp, key, &err);
  if (err) {
    g_clear_error (&err);
    return def;
  }
  return CLAMP (v, min, max);
}

static void
load_entry (GKeyFile * kf, const gchar * grp, Fp6CamEntry * e,
    const gchar * name, gint w, gint h, const gchar * sizes, gint angle,
    const gchar * def_flip)
{
  e->camera_name = kf_str (kf, grp, "camera-name", name);
  e->width = kf_int (kf, grp, "width", w, 16, 16384);
  e->height = kf_int (kf, grp, "height", h, 16, 16384);
  e->format = kf_str (kf, grp, "format", "BGRx");
  e->sizes = kf_str (kf, grp, "sizes", sizes);
  e->mount_angle = kf_int (kf, grp, "mount-angle", angle, 0, 359);
  e->mount_angle = (e->mount_angle / 90) * 90;
  e->af_mode = kf_str (kf, grp, "af-mode", "");
  e->flip = kf_str (kf, grp, "flip", def_flip);
  e->swap_rb = kf ? g_key_file_get_boolean (kf, grp, "swap-rb", NULL) : FALSE;
  e->scale_threads = kf_int (kf, grp, "scale-threads", 4, 1, 8);
}

static const Fp6CamConfig *
fp6cam_config (void)
{
  static gsize once = 0;

  if (g_once_init_enter (&once)) {
    Fp6CamConfig *c = g_new0 (Fp6CamConfig, 1);
    const gchar *path = g_getenv ("FP6_CAMERA_CONFIG");
    GKeyFile *kf = g_key_file_new ();
    GError *err = NULL;

    if (!path || !*path)
      path = FP6CAM_DEFAULT_CONFIG;
    if (!g_key_file_load_from_file (kf, path, G_KEY_FILE_NONE, &err)) {
      GST_INFO ("no config at %s (%s), using built-in defaults", path,
          err ? err->message : "?");
      g_clear_error (&err);
      g_key_file_unref (kf);
      kf = NULL;
    }

    load_entry (kf, "primary", &c->cam[0],
        "/base/soc@0/cci@ac15000/i2c-bus@0/camera@1a", 2048, 1536,
        "2048x1536;1440x1080;960x720;640x480", 270, "none");
    load_entry (kf, "secondary", &c->cam[1],
        "/base/soc@0/cci@ac16000/i2c-bus@1/camera@3d", 1536, 1152,
        "1536x1152;1440x1080;960x720;640x480", 270, "none");

    c->led_path = kf_str (kf, "flash", "led-brightness-path",
        "/sys/class/leds/white:flash/brightness");
    c->led_brightness = kf_int (kf, "flash", "brightness", 160, 1, 255);
    c->preflash_ms = kf_int (kf, "flash", "preflash-ms", 400, 0, 1500);
    c->capture_hold_max_ms =
        kf_int (kf, "flash", "capture-hold-max-ms", 1100, 100, 3000);
    c->first_frame_timeout_ms =
        kf_int (kf, "stream", "first-frame-timeout-ms", 1200, 300, 20000);
    c->max_restarts = kf_int (kf, "stream", "max-restarts", 2, 0, 5);
    c->video_light_max_s =
        kf_int (kf, "flash", "video-light-max-s", 600, 1, 3600);
    c->auto_is_on = kf ? g_key_file_get_boolean (kf, "flash", "auto-is-on",
        NULL) : FALSE;

    if (kf)
      g_key_file_unref (kf);
    global_config = c;
    g_once_init_leave (&once, 1);
  }
  return global_config;
}

/* Map a "device" string to an entry index; -1 = literal libcamera id. */
static gint
device_index (const gchar * device)
{
  if (!device || !*device || g_strcmp0 (device, "primary") == 0
      || g_strcmp0 (device, "0") == 0 || g_strcmp0 (device, "back") == 0)
    return 0;
  if (g_strcmp0 (device, "secondary") == 0 || g_strcmp0 (device, "1") == 0
      || g_strcmp0 (device, "front") == 0)
    return 1;
  return -1;
}

/* ------------------------------------------------------------------------ */
/* LED control (bounded)                                                    */
/* ------------------------------------------------------------------------ */

static GMutex led_lock;
static GCond led_cond;
static guint64 led_generation;
static gboolean led_on;

static void
led_write (gint value)
{
  const Fp6CamConfig *c = fp6cam_config ();
  gchar buf[16];
  gint fd, n;

  fd = open (c->led_path, O_WRONLY | O_CLOEXEC);
  if (fd < 0) {
    GST_WARNING ("cannot open %s: %s", c->led_path, g_strerror (errno));
    return;
  }
  n = g_snprintf (buf, sizeof buf, "%d\n", value);
  if (write (fd, buf, n) != n)
    GST_WARNING ("cannot write %s: %s", c->led_path, g_strerror (errno));
  close (fd);
}

typedef struct
{
  guint64 generation;
  gint64 deadline;
} LedWatch;

static gpointer
led_watchdog (gpointer data)
{
  LedWatch *w = data;

  g_mutex_lock (&led_lock);
  while (led_generation == w->generation && led_on) {
    if (!g_cond_wait_until (&led_cond, &led_lock, w->deadline))
      break;                    /* timed out */
  }
  if (led_generation == w->generation && led_on) {
    GST_INFO ("flash LED bound reached, switching off");
    led_write (0);
    led_on = FALSE;
    led_generation++;
  }
  g_mutex_unlock (&led_lock);
  g_free (w);
  return NULL;
}

/* Switch the LED on for at most max_ms; returns the generation token. */
static guint64
led_on_bounded (gint64 max_ms)
{
  LedWatch *w = g_new0 (LedWatch, 1);
  GThread *t;
  guint64 gen;

  g_mutex_lock (&led_lock);
  led_generation++;
  gen = led_generation;
  led_write (fp6cam_config ()->led_brightness);
  led_on = TRUE;
  g_cond_broadcast (&led_cond);
  w->generation = gen;
  w->deadline = g_get_monotonic_time () + max_ms * G_TIME_SPAN_MILLISECOND;
  g_mutex_unlock (&led_lock);

  t = g_thread_new ("fp6-led-watch", led_watchdog, w);
  g_thread_unref (t);
  return gen;
}

static void
led_off (void)
{
  g_mutex_lock (&led_lock);
  if (led_on)
    led_write (0);
  led_on = FALSE;
  led_generation++;
  g_cond_broadcast (&led_cond);
  g_mutex_unlock (&led_lock);
}

/* ------------------------------------------------------------------------ */
/* fp6libcamerasrc                                                          */
/* ------------------------------------------------------------------------ */

#define FP6_TYPE_LIBCAMERA_SRC (fp6_libcamera_src_get_type ())
G_DECLARE_FINAL_TYPE (Fp6LibcameraSrc, fp6_libcamera_src, FP6, LIBCAMERA_SRC,
    GstBin);

struct _Fp6LibcameraSrc
{
  GstBin parent;
  GstPad *ghost;
  gchar *device;
  gboolean built;
  GstElement *cam;              /* libcamerasrc child (not owned) */
  GstElement *crop;             /* videocrop child (not owned) */
  gint stream_w, stream_h;      /* fixed stream size of the built chain */
  gfloat zoom;                  /* requested, under object lock */
  gfloat zoom_applied;          /* streaming thread only */
  /* first-frame watchdog */
  GMutex wd_lock;
  GCond wd_cond;
  guint64 wd_gen;               /* bumped on every (re)start / stop */
  gboolean got_frame;
  gint restarts;
  struct _Fp6Manual *manual;    /* MANUAL CONTROLS section (fp6camera-manual.c) */
  struct _Fp6Phase2 *p2;        /* PHASE 2 section (fp6camera-phase2.c) */
};

/* MANUAL CONTROLS section hooks (defined in fp6camera-manual.c) */
typedef struct _Fp6Manual Fp6Manual;
static Fp6Manual *fp6m_new (Fp6LibcameraSrc * self);
static void fp6m_free (Fp6LibcameraSrc * self);
static void fp6m_class_init (GObjectClass * oc);
static const Fp6CamEntry *fp6m_select_entry (Fp6LibcameraSrc * self, gint idx,
    const Fp6CamEntry * e);
static void fp6m_attach (Fp6LibcameraSrc * self, GstElement * cam,
    GstElement * crop, gint idx);
static void fp6m_detach (Fp6LibcameraSrc * self);
static gboolean fp6m_set_property (Fp6LibcameraSrc * self, guint id,
    const GValue * value);
static gboolean fp6m_get_property (Fp6LibcameraSrc * self, guint id,
    GValue * value);

/* PHASE 2 section hooks (defined in fp6camera-phase2.c) */
typedef struct _Fp6Phase2 Fp6Phase2;
static Fp6Phase2 *fp6p2_new (Fp6LibcameraSrc * self);
static void fp6p2_free (Fp6LibcameraSrc * self);
static void fp6p2_class_init (GObjectClass * oc);
static void fp6p2_build (Fp6LibcameraSrc * self, GstElement * cam,
    GstElement * crop, const gchar * camera_name, const Fp6CamEntry * e,
    gint idx);
static void fp6p2_clear (Fp6LibcameraSrc * self);
static gboolean fp6p2_set_property (Fp6LibcameraSrc * self, guint id,
    const GValue * value);
static gboolean fp6p2_get_property (Fp6LibcameraSrc * self, guint id,
    GValue * value);

/* FAST LAUNCH section hooks (defined in fp6camera-fastlaunch.c) */
static void fp6fl_open (Fp6LibcameraSrc * self);
static void fp6fl_close (Fp6LibcameraSrc * self);

enum
{
  LSRC_PROP_0,
  LSRC_PROP_DEVICE,
  LSRC_PROP_ZOOM,
};

#define FP6CAM_MAX_ZOOM 10.0f

G_DEFINE_TYPE (Fp6LibcameraSrc, fp6_libcamera_src, GST_TYPE_BIN);

static GstStaticPadTemplate lsrc_template = GST_STATIC_PAD_TEMPLATE ("src",
    GST_PAD_SRC, GST_PAD_ALWAYS, GST_STATIC_CAPS ("video/x-raw"));

static GstCaps *
sizes_to_caps (const gchar * format, const gchar * sizes, gint max_w,
    gint max_h)
{
  GstCaps *caps = gst_caps_new_empty ();
  gchar **parts = g_strsplit (sizes, ";", -1);

  for (gchar ** p = parts; *p; p++) {
    gint w = 0, h = 0;
    if (sscanf (g_strstrip (*p), "%dx%d", &w, &h) != 2 || w <= 0 || h <= 0)
      continue;
    if (w > max_w || h > max_h)
      continue;                 /* never upscale */
    gst_caps_append_structure (caps, gst_structure_new ("video/x-raw",
            "format", G_TYPE_STRING, format,
            "width", G_TYPE_INT, w, "height", G_TYPE_INT, h, NULL));
  }
  g_strfreev (parts);
  if (gst_caps_is_empty (caps))
    gst_caps_append_structure (caps, gst_structure_new ("video/x-raw",
            "format", G_TYPE_STRING, format,
            "width", G_TYPE_INT, max_w, "height", G_TYPE_INT, max_h, NULL));
  return caps;
}

static void
lsrc_clear (Fp6LibcameraSrc * self)
{
  GList *children = g_list_copy (GST_BIN_CHILDREN (self));

  fp6m_detach (self);           /* MANUAL CONTROLS */
  fp6p2_clear (self);           /* PHASE 2 */
  gst_ghost_pad_set_target (GST_GHOST_PAD (self->ghost), NULL);
  g_mutex_lock (&self->wd_lock);
  self->cam = NULL;
  self->crop = NULL;
  self->wd_gen++;
  g_cond_broadcast (&self->wd_cond);
  g_mutex_unlock (&self->wd_lock);
  for (GList * l = children; l; l = l->next)
    gst_bin_remove (GST_BIN (self), GST_ELEMENT (l->data));
  g_list_free (children);
  self->built = FALSE;
}

/* CAMSS on this kernel intermittently delivers no frame on the first stream
 * of a sensor after another CSIPHY used csid0 (camera-test.sh stress).  A
 * fresh libcamera start (new CameraManager: nodes reopened, CAMSS re-powered)
 * always recovered, so restart libcamerasrc through NULL when the first
 * frame does not arrive in time. */
static GstPadProbeReturn
lsrc_frame_probe (GstPad * pad, GstPadProbeInfo * info, gpointer user)
{
  Fp6LibcameraSrc *self = user;

  if (G_UNLIKELY (!self->got_frame)) {
    g_mutex_lock (&self->wd_lock);
    if (!self->got_frame)
      GST_INFO_OBJECT (self, "first frame (restarts %d)", self->restarts);
    self->got_frame = TRUE;
    g_cond_broadcast (&self->wd_cond);
    g_mutex_unlock (&self->wd_lock);
  }
  return GST_PAD_PROBE_OK;
}

static gpointer lsrc_watchdog (gpointer data);

/* Streaming thread, before videocrop sees the buffer: apply a changed zoom.
 * videocrop marks its src pad for reconfigure; basetransform renegotiates
 * and reallocates at the start of this very chain call. */
static GstPadProbeReturn
lsrc_zoom_probe (GstPad * pad, GstPadProbeInfo * info, gpointer user)
{
  Fp6LibcameraSrc *self = user;
  gfloat zoom;
  gint w = self->stream_w, h = self->stream_h, cw, ch, l, t;

  GST_OBJECT_LOCK (self);
  zoom = self->zoom;
  GST_OBJECT_UNLOCK (self);
  if (zoom == self->zoom_applied || !self->crop || w <= 0 || h <= 0)
    return GST_PAD_PROBE_OK;
  self->zoom_applied = zoom;

  zoom = CLAMP (zoom, 1.0f, FP6CAM_MAX_ZOOM);
  cw = ((gint) (w / zoom)) & ~1;
  ch = ((gint) ((gint64) cw * h / w)) & ~1;
  cw = CLAMP (cw, 16, w);
  ch = CLAMP (ch, 16, h);
  l = ((w - cw) / 2) & ~1;
  t = ((h - ch) / 2) & ~1;
  GST_DEBUG_OBJECT (self, "zoom %.2f -> crop %dx%d+%d+%d", zoom, cw, ch, l, t);
  g_object_set (self->crop, "left", l, "right", w - cw - l, "top", t,
      "bottom", h - ch - t, NULL);
  return GST_PAD_PROBE_OK;
}

static void
lsrc_arm_watchdog (Fp6LibcameraSrc * self)
{
  g_mutex_lock (&self->wd_lock);
  self->wd_gen++;
  self->got_frame = FALSE;
  g_mutex_unlock (&self->wd_lock);
  g_thread_unref (g_thread_new ("fp6-first-frame", lsrc_watchdog,
          gst_object_ref (self)));
}

static void
lsrc_disarm_watchdog (Fp6LibcameraSrc * self)
{
  g_mutex_lock (&self->wd_lock);
  self->wd_gen++;
  g_cond_broadcast (&self->wd_cond);
  g_mutex_unlock (&self->wd_lock);
}

static gpointer
lsrc_watchdog (gpointer data)
{
  Fp6LibcameraSrc *self = data;
  const Fp6CamConfig *c = fp6cam_config ();
  gint64 deadline = g_get_monotonic_time () +
      (gint64) c->first_frame_timeout_ms * G_TIME_SPAN_MILLISECOND;
  guint64 gen;
  gboolean restart = FALSE;
  GstElement *cam = NULL;

  g_mutex_lock (&self->wd_lock);
  gen = self->wd_gen;
  while (!self->got_frame && self->wd_gen == gen)
    if (!g_cond_wait_until (&self->wd_cond, &self->wd_lock, deadline))
      break;
  if (!self->got_frame && self->wd_gen == gen && self->cam
      && self->restarts < c->max_restarts) {
    self->restarts++;
    restart = TRUE;
    cam = gst_object_ref (self->cam);
  }
  g_mutex_unlock (&self->wd_lock);

  if (restart) {
    GST_WARNING_OBJECT (self, "no frame within %d ms: restarting libcamera "
        "(attempt %d/%d)", c->first_frame_timeout_ms, self->restarts,
        c->max_restarts);
    gst_element_set_state (cam, GST_STATE_NULL);
    gst_element_get_state (cam, NULL, NULL, 5 * GST_SECOND);
    g_mutex_lock (&self->wd_lock);
    restart = self->wd_gen == gen;      /* not stopped meanwhile */
    g_mutex_unlock (&self->wd_lock);
    if (restart) {
      if (!gst_element_sync_state_with_parent (cam))
        GST_ELEMENT_WARNING (self, RESOURCE, FAILED,
            ("camera restart failed"), (NULL));
      else
        lsrc_arm_watchdog (self);
    }
    gst_object_unref (cam);
  } else if (!self->got_frame && self->wd_gen == gen) {
    GST_ELEMENT_WARNING (self, RESOURCE, READ,
        ("no camera frames after %d restarts", self->restarts), (NULL));
  }
  gst_object_unref (self);
  return NULL;
}

/* The packed RGB format with red and blue exchanged, or NULL. */
static const gchar *
swapped_rb_format (const gchar * f)
{
  static const gchar *pairs[][2] = {
    {"BGRx", "RGBx"}, {"BGRA", "RGBA"}, {"xRGB", "xBGR"}, {"ARGB", "ABGR"},
    {"BGR", "RGB"},
  };
  for (guint i = 0; i < G_N_ELEMENTS (pairs); i++) {
    if (g_strcmp0 (f, pairs[i][0]) == 0)
      return pairs[i][1];
    if (g_strcmp0 (f, pairs[i][1]) == 0)
      return pairs[i][0];
  }
  return NULL;
}

/* After the capssetter relabel: the buffers still carry libcamerasrc's
 * GstVideoMeta with the old format, which gst_video_frame_map() rejects
 * ("invalid video buffer"). Rewrite the meta's format (metadata only). */
static GstPadProbeReturn
lsrc_swap_meta_probe (GstPad * pad, GstPadProbeInfo * info, gpointer user)
{
  GstVideoFormat to = (GstVideoFormat) GPOINTER_TO_INT (user);
  GstBuffer *buf = GST_PAD_PROBE_INFO_BUFFER (info);
  GstVideoMeta *m = gst_buffer_get_video_meta (buf);

  if (m && m->format != to) {
    buf = gst_buffer_make_writable (buf);
    GST_PAD_PROBE_INFO_DATA (info) = buf;
    m = gst_buffer_get_video_meta (buf);
    if (m)
      m->format = to;
  }
  return GST_PAD_PROBE_OK;
}

static gboolean
lsrc_build (Fp6LibcameraSrc * self)
{
  const Fp6CamConfig *c = fp6cam_config ();
  gint idx = device_index (self->device);
  const Fp6CamEntry *e = &c->cam[idx < 0 ? 0 : idx];
  const gchar *camera_name = idx < 0 ? self->device : e->camera_name;
  GstElement *cam, *infilter, *crop, *scale, *outfilter, *flip = NULL, *last;
  GstElement *relabel = NULL;
  GstCaps *caps;
  GstPad *pad;
  GParamSpec *ps;

  if (self->built)
    return TRUE;

  /* MANUAL CONTROLS: lens selection (primary -> ultrawide) */
  e = fp6m_select_entry (self, idx, e);
  if (idx >= 0)
    camera_name = e->camera_name;

  cam = gst_element_factory_make ("libcamerasrc", "fp6-libcamerasrc");
  infilter = gst_element_factory_make ("capsfilter", "fp6-stream-caps");
  crop = gst_element_factory_make ("videocrop", "fp6-zoom-crop");
  scale = gst_element_factory_make ("videoconvertscale", "fp6-scale");
  outfilter = gst_element_factory_make ("capsfilter", "fp6-offered-caps");
  if (!cam || !infilter || !crop || !scale || !outfilter) {
    GST_ELEMENT_ERROR (self, CORE, MISSING_PLUGIN,
        ("libcamerasrc/videocrop/videoconvertscale/capsfilter missing"),
        (NULL));
    g_clear_object (&cam);
    g_clear_object (&infilter);
    g_clear_object (&crop);
    g_clear_object (&scale);
    g_clear_object (&outfilter);
    return FALSE;
  }

  GST_INFO_OBJECT (self, "device '%s' -> camera '%s' stream %dx%d %s",
      GST_STR_NULL (self->device), camera_name, e->width, e->height,
      e->format);
  g_object_set (cam, "camera-name", camera_name, NULL);

  ps = g_object_class_find_property (G_OBJECT_GET_CLASS (cam), "af-mode");
  if (ps && e->af_mode && *e->af_mode && idx >= 0)
    gst_util_set_object_arg (G_OBJECT (cam), "af-mode", e->af_mode);

  caps = gst_caps_new_simple ("video/x-raw",
      "format", G_TYPE_STRING, e->format,
      "width", G_TYPE_INT, e->width, "height", G_TYPE_INT, e->height, NULL);
  g_object_set (infilter, "caps", caps, NULL);
  gst_caps_unref (caps);

  if (g_object_class_find_property (G_OBJECT_GET_CLASS (scale), "n-threads"))
    g_object_set (scale, "n-threads", (guint) e->scale_threads, NULL);
  if (g_object_class_find_property (G_OBJECT_GET_CLASS (scale), "add-borders"))
    g_object_set (scale, "add-borders", FALSE, NULL);

  caps = sizes_to_caps (e->format, e->sizes, e->width, e->height);
  g_object_set (outfilter, "caps", caps, NULL);
  gst_caps_unref (caps);

  /* swap-rb: the sensor's Bayer order as reported is off by one row/column
   * (front S5KKD1: skin came out blue), so libcamera's RGB has R and B
   * exchanged. Relabel the buffers (no copy) with the swapped format; the
   * existing videoconvertscale then does the actual swap together with its
   * scaling. */
  if (idx >= 0 && e->swap_rb) {
    const gchar *sw = swapped_rb_format (e->format);
    relabel = sw ? gst_element_factory_make ("capssetter", "fp6-swap-rb") : NULL;
    if (relabel) {
      caps = gst_caps_new_simple ("video/x-raw", "format", G_TYPE_STRING, sw,
          NULL);
      g_object_set (relabel, "caps", caps, "join", TRUE, "replace", FALSE,
          NULL);
      gst_caps_unref (caps);
      GST_INFO_OBJECT (self, "swap-rb: %s relabelled as %s", e->format, sw);
    } else {
      GST_WARNING_OBJECT (self, "swap-rb unavailable (format %s or capssetter "
          "missing)", e->format);
    }
  }

  if (idx >= 0 && e->flip && *e->flip && g_strcmp0 (e->flip, "none") != 0) {
    flip = gst_element_factory_make ("videoflip", "fp6-flip");
    if (flip) {
      gst_util_set_object_arg (G_OBJECT (flip), "method", e->flip);
      GST_INFO_OBJECT (self, "flip %s", e->flip);
    } else {
      GST_WARNING_OBJECT (self, "videoflip missing, image stays unflipped");
    }
  }

  gst_bin_add_many (GST_BIN (self), cam, infilter, crop, scale, outfilter,
      NULL);
  if (flip)
    gst_bin_add (GST_BIN (self), flip);
  if (relabel)
    gst_bin_add (GST_BIN (self), relabel);
  last = flip ? flip : outfilter;
  self->cam = cam;
  self->crop = crop;
  self->stream_w = e->width;
  self->stream_h = e->height;
  self->zoom_applied = 1.0f;    /* fresh videocrop: no crop */
  pad = gst_element_get_static_pad (cam, "src");
  gst_pad_add_probe (pad, GST_PAD_PROBE_TYPE_BUFFER, lsrc_frame_probe, self,
      NULL);
  gst_object_unref (pad);
  pad = gst_element_get_static_pad (crop, "sink");
  gst_pad_add_probe (pad, GST_PAD_PROBE_TYPE_BUFFER, lsrc_zoom_probe, self,
      NULL);
  gst_object_unref (pad);
  if (relabel) {
    pad = gst_element_get_static_pad (relabel, "src");
    gst_pad_add_probe (pad, GST_PAD_PROBE_TYPE_BUFFER, lsrc_swap_meta_probe,
        GINT_TO_POINTER (gst_video_format_from_string (swapped_rb_format
                (e->format))), NULL);
    gst_object_unref (pad);
  }
  if (!(relabel ? gst_element_link_many (cam, infilter, relabel, crop, NULL)
          : gst_element_link_many (cam, infilter, crop, NULL))
      || !gst_element_link_many (crop, scale, outfilter, NULL)
      || (flip && !gst_element_link (outfilter, flip))) {
    GST_ELEMENT_ERROR (self, CORE, NEGOTIATION, ("cannot link camera chain"),
        (NULL));
    lsrc_clear (self);
    return FALSE;
  }
  pad = gst_element_get_static_pad (last, "src");
  gst_ghost_pad_set_target (GST_GHOST_PAD (self->ghost), pad);
  gst_object_unref (pad);
  fp6m_attach (self, cam, crop, idx);   /* MANUAL CONTROLS */
  fp6p2_build (self, cam, crop, camera_name, e, idx);   /* PHASE 2 */
  self->built = TRUE;
  return TRUE;
}

static GstStateChangeReturn
lsrc_change_state (GstElement * element, GstStateChange transition)
{
  Fp6LibcameraSrc *self = FP6_LIBCAMERA_SRC (element);
  GstStateChangeReturn ret;

  if (transition == GST_STATE_CHANGE_NULL_TO_READY && !lsrc_build (self))
    return GST_STATE_CHANGE_FAILURE;
  if (transition == GST_STATE_CHANGE_NULL_TO_READY)
    fp6fl_open (self);          /* FAST LAUNCH */
  if (transition == GST_STATE_CHANGE_PLAYING_TO_PAUSED)
    lsrc_disarm_watchdog (self);

  ret = GST_ELEMENT_CLASS (fp6_libcamera_src_parent_class)->change_state
      (element, transition);

  /* Drop the libcamera element on NULL so the camera is released and a new
   * device selection takes effect on the next start. */
  if (transition == GST_STATE_CHANGE_PAUSED_TO_PLAYING
      && ret != GST_STATE_CHANGE_FAILURE) {
    self->restarts = 0;
    lsrc_arm_watchdog (self);
  }
  if (transition == GST_STATE_CHANGE_PAUSED_TO_READY)
    fp6fl_close (self);         /* FAST LAUNCH */
  if (transition == GST_STATE_CHANGE_READY_TO_NULL)
    lsrc_clear (self);
  return ret;
}

static void
lsrc_set_property (GObject * object, guint id, const GValue * value,
    GParamSpec * pspec)
{
  Fp6LibcameraSrc *self = FP6_LIBCAMERA_SRC (object);

  if (fp6m_set_property (self, id, value))      /* MANUAL CONTROLS */
    return;
  if (fp6p2_set_property (self, id, value))     /* PHASE 2 */
    return;
  switch (id) {
    case LSRC_PROP_DEVICE:
      if (GST_STATE (self) != GST_STATE_NULL) {
        GST_WARNING_OBJECT (self, "device can only change in NULL state");
        break;
      }
      g_free (self->device);
      self->device = g_value_dup_string (value);
      break;
    case LSRC_PROP_ZOOM:
      GST_OBJECT_LOCK (self);
      self->zoom = CLAMP (g_value_get_float (value), 1.0f, FP6CAM_MAX_ZOOM);
      GST_OBJECT_UNLOCK (self);
      break;
    default:
      G_OBJECT_WARN_INVALID_PROPERTY_ID (object, id, pspec);
  }
}

static void
lsrc_get_property (GObject * object, guint id, GValue * value,
    GParamSpec * pspec)
{
  Fp6LibcameraSrc *self = FP6_LIBCAMERA_SRC (object);

  if (fp6m_get_property (self, id, value))      /* MANUAL CONTROLS */
    return;
  if (fp6p2_get_property (self, id, value))     /* PHASE 2 */
    return;
  switch (id) {
    case LSRC_PROP_DEVICE:
      g_value_set_string (value, self->device ? self->device : "primary");
      break;
    case LSRC_PROP_ZOOM:
      GST_OBJECT_LOCK (self);
      g_value_set_float (value, self->zoom);
      GST_OBJECT_UNLOCK (self);
      break;
    default:
      G_OBJECT_WARN_INVALID_PROPERTY_ID (object, id, pspec);
  }
}

static void
lsrc_finalize (GObject * object)
{
  Fp6LibcameraSrc *self = FP6_LIBCAMERA_SRC (object);
  fp6p2_free (self);            /* PHASE 2 */
  fp6m_free (self);             /* MANUAL CONTROLS */
  g_free (self->device);
  g_mutex_clear (&self->wd_lock);
  g_cond_clear (&self->wd_cond);
  G_OBJECT_CLASS (fp6_libcamera_src_parent_class)->finalize (object);
}

static void
fp6_libcamera_src_class_init (Fp6LibcameraSrcClass * klass)
{
  GObjectClass *oc = G_OBJECT_CLASS (klass);
  GstElementClass *ec = GST_ELEMENT_CLASS (klass);

  oc->set_property = lsrc_set_property;
  oc->get_property = lsrc_get_property;
  oc->finalize = lsrc_finalize;
  ec->change_state = lsrc_change_state;

  g_object_class_install_property (oc, LSRC_PROP_DEVICE,
      g_param_spec_string ("device", "Device",
          "primary | secondary | literal libcamera camera id", "primary",
          G_PARAM_READWRITE | G_PARAM_STATIC_STRINGS));
  g_object_class_install_property (oc, LSRC_PROP_ZOOM,
      g_param_spec_float ("zoom", "Zoom",
          "Digital zoom (centre crop, applied between buffers)", 1.0f,
          G_MAXFLOAT, 1.0f, G_PARAM_READWRITE | G_PARAM_STATIC_STRINGS));
  fp6m_class_init (oc);         /* MANUAL CONTROLS */
  fp6p2_class_init (oc);        /* PHASE 2 */

  gst_element_class_add_static_pad_template (ec, &lsrc_template);
  gst_element_class_set_static_metadata (ec, "FP6 libcamera source",
      "Source/Video", "libcamerasrc with a fixed stream size and scaling for "
      "camerabin", "fp6 port");
}

static void
fp6_libcamera_src_init (Fp6LibcameraSrc * self)
{
  self->ghost = gst_ghost_pad_new_no_target_from_template ("src",
      gst_static_pad_template_get (&lsrc_template));
  gst_element_add_pad (GST_ELEMENT (self), self->ghost);
  self->device = g_strdup ("primary");
  self->zoom = self->zoom_applied = 1.0f;
  g_mutex_init (&self->wd_lock);
  g_cond_init (&self->wd_cond);
  self->manual = fp6m_new (self);       /* MANUAL CONTROLS */
  self->p2 = fp6p2_new (self);  /* PHASE 2 */
}

/* ------------------------------------------------------------------------ */
/* MANUAL CONTROLS (FP6 Phase 1 pro mode): separate file, same unit         */
/* ------------------------------------------------------------------------ */
#include "fp6camera-manual.c"

/* ------------------------------------------------------------------------ */
/* PHASE 2 (computational / standard features): separate file, same unit   */
/* ------------------------------------------------------------------------ */
#include "fp6camera-phase2.c"

/* ------------------------------------------------------------------------ */
/* FAST LAUNCH (stored AE state): separate file, same unit                  */
/* ------------------------------------------------------------------------ */
#include "fp6camera-fastlaunch.c"

/* ------------------------------------------------------------------------ */
/* fp6camerabinsrc: runtime subclass of wrappercamerabinsrc                 */
/* ------------------------------------------------------------------------ */

typedef struct
{
  gint camera_device;
  GstPhotographyFlashMode flash_mode;
  GstPhotographyFocusMode focus_mode;
  gboolean video_light;
  gboolean active;              /* >= PAUSED */
  GMutex lock;
} CbsState;

static GQuark cbs_quark;
static gpointer cbs_parent_class;
static GstBaseCameraSrcClass cbs_parent_vfuncs;
static GstStateChangeReturn (*cbs_parent_change_state) (GstElement *,
    GstStateChange);

enum
{
  CBS_PROP_0,
  CBS_PROP_CAMERA_DEVICE,
  CBS_PROP_SENSOR_MOUNT_ANGLE,
  /* GstPhotography interface properties (except "zoom", already provided
   * by GstBaseCameraSrc) */
  CBS_PROP_WB_MODE,
  CBS_PROP_COLOR_TONE,
  CBS_PROP_SCENE_MODE,
  CBS_PROP_FLASH_MODE,
  CBS_PROP_NOISE_REDUCTION,
  CBS_PROP_CAPABILITIES,
  CBS_PROP_EV_COMP,
  CBS_PROP_ISO_SPEED,
  CBS_PROP_APERTURE,
  CBS_PROP_EXPOSURE_TIME,
  CBS_PROP_IMAGE_CAPTURE_SUPPORTED_CAPS,
  CBS_PROP_IMAGE_PREVIEW_SUPPORTED_CAPS,
  CBS_PROP_FLICKER_MODE,
  CBS_PROP_FOCUS_MODE,
  CBS_PROP_COLOR_TEMPERATURE,
  CBS_PROP_WHITE_POINT,
  CBS_PROP_ANALOG_GAIN,
  CBS_PROP_EXPOSURE_MODE,
  CBS_PROP_LENS_FOCUS,
  CBS_PROP_MIN_EXPOSURE_TIME,
  CBS_PROP_MAX_EXPOSURE_TIME,
};

static CbsState *
cbs_state (gpointer obj)
{
  return g_object_get_qdata (G_OBJECT (obj), cbs_quark);
}

static void
cbs_state_free (gpointer p)
{
  CbsState *s = p;
  g_mutex_clear (&s->lock);
  g_free (s);
}

/* Re-evaluate continuous video light: FILL_IN flash mode, video mode, and
 * the source running. */
static void
cbs_update_video_light (GstBaseCameraSrc * src)
{
  CbsState *s = cbs_state (src);
  gboolean want;

  g_mutex_lock (&s->lock);
  want = s->active && src->mode == MODE_VIDEO
      && s->flash_mode == GST_PHOTOGRAPHY_FLASH_MODE_FILL_IN;
  if (want != s->video_light) {
    s->video_light = want;
    g_mutex_unlock (&s->lock);
    if (want) {
      GST_INFO_OBJECT (src, "video light on");
      led_on_bounded ((gint64) fp6cam_config ()->video_light_max_s * 1000);
    } else {
      GST_INFO_OBJECT (src, "video light off");
      led_off ();
    }
    return;
  }
  g_mutex_unlock (&s->lock);
}

static void
cbs_mode_notify (GObject * obj, GParamSpec * pspec, gpointer user)
{
  cbs_update_video_light (GST_BASE_CAMERA_SRC (obj));
}

typedef struct
{
  GstBaseCameraSrc *src;
} FlashJob;

static gpointer
cbs_flash_capture_thread (gpointer data)
{
  FlashJob *job = data;
  GstBaseCameraSrc *src = job->src;
  const Fp6CamConfig *c = fp6cam_config ();
  gboolean ok;
  gint64 end;

  /* let the software AE settle on the lit scene */
  g_usleep ((gulong) c->preflash_ms * 1000);

  g_mutex_lock (&src->capturing_mutex);
  ok = cbs_parent_vfuncs.start_capture (src);
  if (!ok) {
    src->capturing = FALSE;
    g_mutex_unlock (&src->capturing_mutex);
    g_object_notify (G_OBJECT (src), "ready-for-capture");
    GST_WARNING_OBJECT (src, "deferred flash capture failed to start");
  } else {
    g_mutex_unlock (&src->capturing_mutex);
  }

  /* keep the light until the frame was grabbed, bounded */
  end = g_get_monotonic_time () +
      (gint64) c->capture_hold_max_ms * G_TIME_SPAN_MILLISECOND;
  while (ok && g_get_monotonic_time () < end) {
    gboolean busy;
    g_mutex_lock (&src->capturing_mutex);
    busy = src->capturing;
    g_mutex_unlock (&src->capturing_mutex);
    if (!busy)
      break;
    g_usleep (15000);
  }
  led_off ();
  cbs_update_video_light (src);
  gst_object_unref (src);
  g_free (job);
  return NULL;
}

/* Called by GstBaseCameraSrc with capturing_mutex held. */
static gboolean
cbs_start_capture (GstBaseCameraSrc * src)
{
  CbsState *s = cbs_state (src);
  const Fp6CamConfig *c = fp6cam_config ();
  GstPhotographyFlashMode fm;
  gboolean fire;

  g_mutex_lock (&s->lock);
  fm = s->flash_mode;
  g_mutex_unlock (&s->lock);

  fire = src->mode == MODE_IMAGE && (fm == GST_PHOTOGRAPHY_FLASH_MODE_ON
      || fm == GST_PHOTOGRAPHY_FLASH_MODE_RED_EYE
      || fm == GST_PHOTOGRAPHY_FLASH_MODE_FILL_IN
      || (fm == GST_PHOTOGRAPHY_FLASH_MODE_AUTO && c->auto_is_on));

  if (fire) {
    FlashJob *job = g_new0 (FlashJob, 1);
    GThread *t;

    GST_INFO_OBJECT (src, "flash capture: preflash %d ms, hold <= %d ms",
        c->preflash_ms, c->capture_hold_max_ms);
    led_on_bounded ((gint64) c->preflash_ms + c->capture_hold_max_ms + 200);
    job->src = gst_object_ref (src);
    t = g_thread_new ("fp6-flash-capture", cbs_flash_capture_thread, job);
    g_thread_unref (t);
    return TRUE;
  }
  return cbs_parent_vfuncs.start_capture (src);
}

static gboolean
cbs_construct_pipeline (GstBaseCameraSrc * src)
{
  GstElement *vs = NULL;

  /* Qt probes the factory (sensor-mount-angle) with a bare instance that
   * has no video source; do not instantiate any default (v4l2src) source
   * for that probe. */
  g_object_get (src, "video-source", &vs, NULL);
  if (!vs) {
    GST_DEBUG_OBJECT (src, "no video-source: metadata probe instance");
    return TRUE;
  }
  gst_object_unref (vs);
  return cbs_parent_vfuncs.construct_pipeline (src);
}

static gboolean
cbs_setup_pipeline (GstBaseCameraSrc * src)
{
  GstElement *vs = NULL;

  g_object_get (src, "video-source", &vs, NULL);
  if (!vs) {
    GST_ELEMENT_ERROR (src, CORE, STATE_CHANGE,
        ("fp6camerabinsrc needs a video-source (fp6libcamerasrc)"), (NULL));
    return FALSE;
  }
  gst_object_unref (vs);
  return cbs_parent_vfuncs.setup_pipeline ?
      cbs_parent_vfuncs.setup_pipeline (src) : TRUE;
}

static GstStateChangeReturn
cbs_change_state (GstElement * element, GstStateChange transition)
{
  CbsState *s = cbs_state (element);
  GstStateChangeReturn ret;

  if (transition == GST_STATE_CHANGE_PAUSED_TO_READY) {
    g_mutex_lock (&s->lock);
    s->active = FALSE;
    g_mutex_unlock (&s->lock);
    cbs_update_video_light (GST_BASE_CAMERA_SRC (element));
    led_off ();
  }

  ret = cbs_parent_change_state (element, transition);

  if (transition == GST_STATE_CHANGE_READY_TO_PAUSED
      && ret != GST_STATE_CHANGE_FAILURE) {
    g_mutex_lock (&s->lock);
    s->active = TRUE;
    g_mutex_unlock (&s->lock);
    cbs_update_video_light (GST_BASE_CAMERA_SRC (element));
  }
  if (transition == GST_STATE_CHANGE_READY_TO_NULL)
    led_off ();
  return ret;
}

/* --- GstPhotography --- */

static gboolean
cbs_get_flash_mode (GstPhotography * photo, GstPhotographyFlashMode * mode)
{
  CbsState *s = cbs_state (photo);
  g_mutex_lock (&s->lock);
  *mode = s->flash_mode;
  g_mutex_unlock (&s->lock);
  return TRUE;
}

static gboolean
cbs_set_flash_mode (GstPhotography * photo, GstPhotographyFlashMode mode)
{
  CbsState *s = cbs_state (photo);
  g_mutex_lock (&s->lock);
  s->flash_mode = mode;
  g_mutex_unlock (&s->lock);
  GST_INFO_OBJECT (photo, "flash mode %d", mode);
  cbs_update_video_light (GST_BASE_CAMERA_SRC (photo));
  return TRUE;
}

static gboolean
cbs_get_focus_mode (GstPhotography * photo, GstPhotographyFocusMode * mode)
{
  CbsState *s = cbs_state (photo);
  *mode = s->focus_mode;
  return TRUE;
}

static gboolean
cbs_set_focus_mode (GstPhotography * photo, GstPhotographyFocusMode mode)
{
  cbs_state (photo)->focus_mode = mode;
  return TRUE;
}

static GstPhotographyCaps
cbs_get_capabilities (GstPhotography * photo)
{
  return GST_PHOTOGRAPHY_CAPS_FLASH | GST_PHOTOGRAPHY_CAPS_FOCUS;
}

static void
cbs_set_autofocus (GstPhotography * photo, gboolean on)
{
  /* libcamera's software AF runs continuously (af-mode in camera.conf);
   * report completion so QCamera searchAndLock() resolves. */
  if (on) {
    GstStructure *st = gst_structure_new (GST_PHOTOGRAPHY_AUTOFOCUS_DONE,
        "status", G_TYPE_INT, GST_PHOTOGRAPHY_FOCUS_STATUS_SUCCESS, NULL);
    gst_element_post_message (GST_ELEMENT (photo),
        gst_message_new_element (GST_OBJECT (photo), st));
  }
}

static void
cbs_photography_init (GstPhotographyInterface * iface)
{
  iface->get_flash_mode = cbs_get_flash_mode;
  iface->set_flash_mode = cbs_set_flash_mode;
  iface->get_focus_mode = cbs_get_focus_mode;
  iface->set_focus_mode = cbs_set_focus_mode;
  iface->get_capabilities = cbs_get_capabilities;
  iface->set_autofocus = cbs_set_autofocus;
}

/* --- properties --- */

static void
cbs_set_property (GObject * object, guint id, const GValue * value,
    GParamSpec * pspec)
{
  CbsState *s = cbs_state (object);

  switch (id) {
    case CBS_PROP_CAMERA_DEVICE:
      s->camera_device = CLAMP (g_value_get_int (value), 0, 1);
      break;
    case CBS_PROP_FLASH_MODE:
      cbs_set_flash_mode (GST_PHOTOGRAPHY (object), g_value_get_enum (value));
      break;
    case CBS_PROP_FOCUS_MODE:
      s->focus_mode = g_value_get_enum (value);
      break;
    default:
      /* other photography settings are accepted and ignored */
      break;
  }
}

static void
cbs_get_property (GObject * object, guint id, GValue * value,
    GParamSpec * pspec)
{
  CbsState *s = cbs_state (object);
  GParamSpec *real = pspec;

  switch (id) {
    case CBS_PROP_CAMERA_DEVICE:
      g_value_set_int (value, s->camera_device);
      return;
    case CBS_PROP_SENSOR_MOUNT_ANGLE:
      g_value_set_int (value,
          fp6cam_config ()->cam[s->camera_device ? 1 : 0].mount_angle);
      return;
    case CBS_PROP_FLASH_MODE:
      g_value_set_enum (value, s->flash_mode);
      return;
    case CBS_PROP_FOCUS_MODE:
      g_value_set_enum (value, s->focus_mode);
      return;
    case CBS_PROP_CAPABILITIES:
      g_value_set_ulong (value, GST_PHOTOGRAPHY_CAPS_FLASH |
          GST_PHOTOGRAPHY_CAPS_FOCUS);
      return;
    default:
      break;
  }
  /* defaults of the interface pspec for the remaining read-only knobs */
  if (G_IS_PARAM_SPEC_OVERRIDE (pspec))
    real = g_param_spec_get_redirect_target (pspec);
  if (G_VALUE_TYPE (value) == G_PARAM_SPEC_VALUE_TYPE (real))
    g_param_value_set_default (real, value);
}

static void
cbs_class_init (gpointer g_class, gpointer class_data)
{
  GObjectClass *oc = G_OBJECT_CLASS (g_class);
  GstElementClass *ec = GST_ELEMENT_CLASS (g_class);
  GstBaseCameraSrcClass *bc = GST_BASE_CAMERA_SRC_CLASS (g_class);

  cbs_parent_class = g_type_class_peek_parent (g_class);
  cbs_parent_vfuncs = *GST_BASE_CAMERA_SRC_CLASS (cbs_parent_class);
  cbs_parent_change_state = GST_ELEMENT_CLASS (cbs_parent_class)->change_state;

  /* GObject dispatches each property to its owner class, so these handlers
   * only see the properties installed/overridden here. */
  oc->set_property = cbs_set_property;
  oc->get_property = cbs_get_property;
  ec->change_state = cbs_change_state;
  bc->start_capture = cbs_start_capture;
  bc->construct_pipeline = cbs_construct_pipeline;
  bc->setup_pipeline = cbs_setup_pipeline;

  g_object_class_install_property (oc, CBS_PROP_CAMERA_DEVICE,
      g_param_spec_int ("camera-device", "Camera device",
          "0 = primary (back), 1 = secondary (front)", 0, 1, 0,
          G_PARAM_READWRITE | G_PARAM_STATIC_STRINGS));
  g_object_class_install_property (oc, CBS_PROP_SENSOR_MOUNT_ANGLE,
      g_param_spec_int ("sensor-mount-angle", "Sensor mount angle",
          "Clockwise rotation needed to show the selected camera upright",
          0, 270, 0, G_PARAM_READABLE | G_PARAM_STATIC_STRINGS));

#define OVR(id, name) g_object_class_override_property (oc, id, name)
  OVR (CBS_PROP_WB_MODE, GST_PHOTOGRAPHY_PROP_WB_MODE);
  OVR (CBS_PROP_COLOR_TONE, GST_PHOTOGRAPHY_PROP_COLOR_TONE);
  OVR (CBS_PROP_SCENE_MODE, GST_PHOTOGRAPHY_PROP_SCENE_MODE);
  OVR (CBS_PROP_FLASH_MODE, GST_PHOTOGRAPHY_PROP_FLASH_MODE);
  OVR (CBS_PROP_NOISE_REDUCTION, GST_PHOTOGRAPHY_PROP_NOISE_REDUCTION);
  OVR (CBS_PROP_CAPABILITIES, GST_PHOTOGRAPHY_PROP_CAPABILITIES);
  OVR (CBS_PROP_EV_COMP, GST_PHOTOGRAPHY_PROP_EV_COMP);
  OVR (CBS_PROP_ISO_SPEED, GST_PHOTOGRAPHY_PROP_ISO_SPEED);
  OVR (CBS_PROP_APERTURE, GST_PHOTOGRAPHY_PROP_APERTURE);
  OVR (CBS_PROP_EXPOSURE_TIME, GST_PHOTOGRAPHY_PROP_EXPOSURE_TIME);
  OVR (CBS_PROP_IMAGE_CAPTURE_SUPPORTED_CAPS,
      GST_PHOTOGRAPHY_PROP_IMAGE_CAPTURE_SUPPORTED_CAPS);
  OVR (CBS_PROP_IMAGE_PREVIEW_SUPPORTED_CAPS,
      GST_PHOTOGRAPHY_PROP_IMAGE_PREVIEW_SUPPORTED_CAPS);
  OVR (CBS_PROP_FLICKER_MODE, GST_PHOTOGRAPHY_PROP_FLICKER_MODE);
  OVR (CBS_PROP_FOCUS_MODE, GST_PHOTOGRAPHY_PROP_FOCUS_MODE);
  OVR (CBS_PROP_COLOR_TEMPERATURE, GST_PHOTOGRAPHY_PROP_COLOR_TEMPERATURE);
  OVR (CBS_PROP_WHITE_POINT, GST_PHOTOGRAPHY_PROP_WHITE_POINT);
  OVR (CBS_PROP_ANALOG_GAIN, GST_PHOTOGRAPHY_PROP_ANALOG_GAIN);
  OVR (CBS_PROP_EXPOSURE_MODE, GST_PHOTOGRAPHY_PROP_EXPOSURE_MODE);
  OVR (CBS_PROP_LENS_FOCUS, GST_PHOTOGRAPHY_PROP_LENS_FOCUS);
  OVR (CBS_PROP_MIN_EXPOSURE_TIME, GST_PHOTOGRAPHY_PROP_MIN_EXPOSURE_TIME);
  OVR (CBS_PROP_MAX_EXPOSURE_TIME, GST_PHOTOGRAPHY_PROP_MAX_EXPOSURE_TIME);
#undef OVR

  gst_element_class_set_static_metadata (ec, "FP6 camerabin source",
      "Source/Video", "wrappercamerabinsrc with FP6 camera selection, "
      "orientation and flash", "fp6 port");
}

static void
cbs_instance_init (GTypeInstance * instance, gpointer g_class)
{
  CbsState *s = g_new0 (CbsState, 1);

  g_mutex_init (&s->lock);
  s->flash_mode = GST_PHOTOGRAPHY_FLASH_MODE_OFF;
  s->focus_mode = GST_PHOTOGRAPHY_FOCUS_MODE_CONTINUOUS_NORMAL;
  g_object_set_qdata_full (G_OBJECT (instance), cbs_quark, s, cbs_state_free);
  g_signal_connect (instance, "notify::mode", G_CALLBACK (cbs_mode_notify),
      NULL);
}

static GType
fp6_camerabin_src_register (void)
{
  GstElementFactory *f;
  GstPluginFeature *loaded;
  GType parent, type;
  GTypeQuery q;
  GTypeInfo info = { 0, };
  const GInterfaceInfo photo_info = {
    (GInterfaceInitFunc) cbs_photography_init, NULL, NULL
  };

  type = g_type_from_name ("GstFp6CameraBinSrc");
  if (type)
    return type;

  /* During a registry rescan this plugin may be initialised before the
   * camerabin plugin is known, so load camerabin explicitly first. */
  parent = g_type_from_name ("GstWrapperCameraBinSrc");
  if (!parent) {
    GstPlugin *p = gst_plugin_load_by_name ("camerabin");
#ifdef FP6CAM_CAMERABIN_PLUGIN
    if (!p)
      p = gst_plugin_load_file (FP6CAM_CAMERABIN_PLUGIN, NULL);
#endif
    if (p)
      gst_object_unref (p);
    parent = g_type_from_name ("GstWrapperCameraBinSrc");
  }
  if (!parent) {
    f = gst_element_factory_find ("wrappercamerabinsrc");
    if (f) {
      loaded = gst_plugin_feature_load (GST_PLUGIN_FEATURE (f));
      gst_object_unref (f);
      if (loaded) {
        parent = gst_element_factory_get_element_type
            (GST_ELEMENT_FACTORY (loaded));
        gst_object_unref (loaded);
      }
    }
  }
  if (!parent) {
    GST_WARNING ("wrappercamerabinsrc not available");
    return G_TYPE_INVALID;
  }
  if (!parent || !g_type_is_a (parent, GST_TYPE_BASE_CAMERA_SRC))
    return G_TYPE_INVALID;

  g_type_query (parent, &q);
  info.class_size = q.class_size;
  info.class_init = cbs_class_init;
  info.instance_size = q.instance_size;
  info.instance_init = cbs_instance_init;

  cbs_quark = g_quark_from_static_string ("fp6-camerabinsrc-state");
  type = g_type_register_static (parent, "GstFp6CameraBinSrc", &info, 0);
  g_type_add_interface_static (type, GST_TYPE_PHOTOGRAPHY, &photo_info);
  return type;
}

/* ------------------------------------------------------------------------ */

static gboolean
plugin_init (GstPlugin * plugin)
{
  GType cbs;
  gboolean ok;

  GST_DEBUG_CATEGORY_INIT (fp6cam_debug, "fp6camera", 0, "FP6 camera glue");

  ok = gst_element_register (plugin, "fp6libcamerasrc", GST_RANK_NONE,
      FP6_TYPE_LIBCAMERA_SRC);
  cbs = fp6_camerabin_src_register ();
  if (cbs != G_TYPE_INVALID)
    ok &= gst_element_register (plugin, "fp6camerabinsrc", GST_RANK_NONE, cbs);
  else
    GST_WARNING ("fp6camerabinsrc not registered (no camerabin plugin)");
  /* never return FALSE: a failed init blacklists the plugin in the registry */
  (void) ok;
  return TRUE;
}

#ifndef PACKAGE
#define PACKAGE "fp6camera"
#endif

GST_PLUGIN_DEFINE (GST_VERSION_MAJOR, GST_VERSION_MINOR, fp6camera,
    "FP6 libcamera glue for Qt camerabin", plugin_init, "0.1", "LGPL",
    "fp6camera", "local")
