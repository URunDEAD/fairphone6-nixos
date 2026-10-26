/* SPDX-License-Identifier: LGPL-2.1-or-later
 *
 * fp6camera: MANUAL CONTROLS section (FP6 Phase 1, "Pro" mode of
 * lomiri-camera-app). #included by fp6camera.c after the fp6libcamerasrc
 * element; it only adds properties/signals/probes to fp6libcamerasrc and
 * never changes the element's automatic behaviour: with nothing manual
 * requested, nothing is sent to libcamerasrc.
 *
 * Control path (see results/feature-push-20261024/camera/phase1/README.md):
 *   QML (CameraApp.Fp6ProControls) --GObject--> fp6libcamerasrc properties
 *   --> libcamerasrc control properties --> libcamera soft IPA
 *   (camera-fp6-pmos/0090-fp6-manual-controls.patch).
 * The QML side finds the element through type qdata "fp6-manual-active"
 * on the Fp6LibcameraSrc GType (a GWeakRef to the most recently built
 * instance), so no symbols need to be exported from this plugin.
 *
 * Features:
 *  - exposure-us / iso / ev / wb-kelvin / focus / ae-lock / awb-lock /
 *    af-lock / trigger-autofocus, mapped to libcamera ExposureTime(Mode),
 *    AnalogueGain(Mode) (ISO 100 = gain 1.0), ExposureValue, AwbEnable +
 *    ColourTemperature / ColourGains, AfMode + LensPosition / AfTrigger.
 *    Exposure above the sensor's single-frame maximum previews at that
 *    maximum; the capture is then a stack (below).
 *  - Readouts of the values the camera really used (libcamera metadata).
 *  - Analysis at <= 5 Hz: luminance + RGB histogram, and a coarse
 *    zebra/focus-peaking mask (stream orientation) for an overlay.
 *  - Long exposure by stacking: "stack-start" accumulates consecutive
 *    frames in linear light (sum = one long exposure of the total time,
 *    or mean = noise reduction), optional dark-frame subtraction (the user
 *    covers the lens), bulb mode (total 0 = until "stack-stop"); the
 *    viewfinder shows the running result; saved as JPEG with EXIF
 *    (orientation, exposure time, ISO).
 *  - Lens selection: "ultrawide" swaps the primary (back) camera for the
 *    [ultrawide] camera.conf entry at the next NULL->READY.
 */

#include <gst/tag/tag.h>
#include <glib/gstdio.h>
#include <math.h>
#include <stdlib.h>

#define FP6M_PROP_BASE 100

enum
{
  FP6M_PROP_EXPOSURE_US = FP6M_PROP_BASE,
  FP6M_PROP_ISO,
  FP6M_PROP_EV,
  FP6M_PROP_WB_KELVIN,
  FP6M_PROP_AE_LOCK,
  FP6M_PROP_AWB_LOCK,
  FP6M_PROP_AF_LOCK,
  FP6M_PROP_FOCUS,
  FP6M_PROP_ULTRAWIDE,
  FP6M_PROP_ANALYSIS,
  FP6M_PROP_ZEBRA_LEVEL,
  /* read-only */
  FP6M_PROP_CUR_EXPOSURE_US,
  FP6M_PROP_CUR_ISO,
  FP6M_PROP_CUR_KELVIN,
  FP6M_PROP_CUR_FOCUS,
  FP6M_PROP_FRAME_DURATION_US,
  FP6M_PROP_MAX_EXPOSURE_US,
  FP6M_PROP_AF_STATE,
  FP6M_PROP_HAS_FOCUS,
  FP6M_PROP_HISTOGRAM,
  FP6M_PROP_OVERLAY,
  FP6M_PROP_OVERLAY_WIDTH,
  FP6M_PROP_OVERLAY_HEIGHT,
  FP6M_PROP_ANALYSIS_SERIAL,
  FP6M_PROP_STACK_STATE,
  FP6M_PROP_STACK_FRAMES,
  FP6M_PROP_STACK_TARGET_FRAMES,
  FP6M_PROP_STACK_FRAME_US,
  FP6M_PROP_STACK_DARK_FRAMES,
  FP6M_PROP_STACK_RESULT,
  FP6M_PROP_STACK_ERROR,
  FP6M_PROP_ACTIVE_LENS,
  FP6M_PROP_LAST
};

#define FP6M_ANALYSIS_HISTOGRAM 1
#define FP6M_ANALYSIS_ZEBRA     2
#define FP6M_ANALYSIS_PEAKING   4

#define FP6M_HIST_BINS 64
#define FP6M_MASK_W 192

/* stack-state values (also used by the QML side) */
typedef enum
{
  STK_IDLE = 0,
  STK_SETTLE = 1,               /* waiting for the stack exposure to apply */
  STK_LIGHT = 2,                /* accumulating */
  STK_WAIT_DARK = 3,            /* user must cover the lens and continue */
  STK_DARK = 4,                 /* accumulating dark frames */
  STK_SAVING = 5,
  STK_DONE = 6,
  STK_ERROR = 7,
} Fp6StackState;

#define FP6M_STACK_MAX_US (4 * 3600 * G_GINT64_CONSTANT (1000000))
#define FP6M_STACK_MAX_FRAMES 30000
#define FP6M_DARK_MAX_FRAMES 8

struct _Fp6Manual
{
  Fp6LibcameraSrc *src;         /* owner, not referenced */
  GMutex lock;
  GstElement *cam;              /* libcamerasrc, referenced while built */

  /* requested by the application */
  gint64 exposure_us;           /* 0 = auto */
  gint iso;                     /* 0 = auto */
  gfloat ev;
  gint wb_kelvin;               /* 0 = auto */
  gboolean ae_lock, awb_lock, af_lock;
  gfloat focus;                 /* < 0 = autofocus, else 0..1 */
  gboolean ultrawide, ultrawide_built;

  /* what was last sent, to avoid restarting AF needlessly */
  gint sent_af_mode;            /* -1 nothing, else AfMode value */
  gboolean sent_anything;

  /* analysis */
  guint analysis;
  gint zebra_level;
  gint64 analysis_last;
  GBytes *hist;
  GBytes *mask;
  gint mask_w, mask_h;
  guint serial;

  /* video info of the probed pad */
  GstCaps *caps;
  GstVideoInfo info;
  gboolean info_ok;

  /* stacking */
  Fp6StackState st;
  gchar *st_path;
  gint64 st_total_us;
  gint st_mode;                 /* 0 sum, 1 mean */
  gboolean st_want_dark;
  gint st_orientation;
  gint st_quality;
  gint64 st_frame_us;
  gfloat st_gain;
  gint st_target, st_frames, st_dark_target, st_dark_frames;
  gint st_settle_ok;
  gint64 st_settle_deadline;
  gboolean st_stop, st_continue, st_cancel;
  gfloat *acc, *dark;
  gint acc_w, acc_h;
  guint8 *preview;              /* last preview, info.size bytes */
  gint64 preview_last;
  gint preview_frames;
  gchar *st_result, *st_error;
  gint st_skip;
  gboolean fdl_known;           /* FrameDurationLimits seen in metadata */
  gboolean has_focus;           /* built camera has a focus lens */
  gchar *flip;                  /* videoflip method after the probe point */

  /* PHASE 2 capture jobs (fp6camera-jobs.c): fixed exposure/WB/focus for
   * a bracket or burst, like a stack, while ovr is set */
  gboolean ovr;
  gint64 ovr_exp_us;
  gdouble ovr_gain;
  gdouble ovr_gr, ovr_gb;       /* <= 0: keep the AWB running */
  gfloat ovr_lens;              /* < 0: keep autofocus */
};

static GWeakRef fp6m_active_ref;
static GQuark fp6m_active_quark;
static gfloat fp6m_lin[256];
static guint8 fp6m_gamma[4096];

/* ------------------------------------------------------------------------ */
/* configuration: [ultrawide]                                               */

static const Fp6CamEntry *
fp6m_ultrawide_entry (void)
{
  static gsize once = 0;
  static Fp6CamEntry e;

  if (g_once_init_enter (&once)) {
    const gchar *path = g_getenv ("FP6_CAMERA_CONFIG");
    GKeyFile *kf = g_key_file_new ();

    if (!path || !*path)
      path = FP6CAM_DEFAULT_CONFIG;
    if (!g_key_file_load_from_file (kf, path, G_KEY_FILE_NONE, NULL)) {
      g_key_file_unref (kf);
      kf = NULL;
    }
    /* OV13B10 13 MP ultrawide (back), 2048x1536 at 30 fps. It is mounted
     * 180 degrees from the main sensor; Qt keeps the primary camera's
     * orientation, so the stream is turned to match it. */
    load_entry (kf, "ultrawide", &e,
        "/base/soc@0/cci@ac15000/i2c-bus@1/camera@36", 2048, 1536,
        "2048x1536;1440x1080;960x720;640x480", 270, "rotate-180");
    if (kf)
      g_key_file_unref (kf);
    g_once_init_leave (&once, 1);
  }
  return &e;
}

/* lsrc_build hook: the entry to build for this device index */
static const Fp6CamEntry *
fp6m_select_entry (Fp6LibcameraSrc * self, gint idx, const Fp6CamEntry * e)
{
  Fp6Manual *m = self->manual;
  gboolean uw;

  g_mutex_lock (&m->lock);
  uw = idx == 0 && m->ultrawide;
  m->ultrawide_built = uw;
  if (uw)
    e = fp6m_ultrawide_entry ();
  /* the chain flips after our probe: masks and saved stacks must follow */
  g_free (m->flip);
  m->flip = g_strdup (idx >= 0 && e->flip ? e->flip : "none");
  g_mutex_unlock (&m->lock);
  if (uw)
    GST_INFO_OBJECT (self, "ultrawide lens selected");
  return e;
}

/* ------------------------------------------------------------------------ */
/* libcamerasrc control helpers (m->lock held)                              */

static gboolean
fp6m_has_prop (GstElement * cam, const gchar * name)
{
  return g_object_class_find_property (G_OBJECT_GET_CLASS (cam), name) != NULL;
}

static gint
fp6m_get_int (GstElement * cam, const gchar * name)
{
  gint v = 0;
  if (cam && fp6m_has_prop (cam, name))
    g_object_get (cam, name, &v, NULL);
  return v;
}

static gfloat
fp6m_get_float (GstElement * cam, const gchar * name)
{
  gfloat v = 0;
  if (cam && fp6m_has_prop (cam, name))
    g_object_get (cam, name, &v, NULL);
  return v;
}

static gint64
fp6m_get_int64 (GstElement * cam, const gchar * name)
{
  gint64 v = 0;
  if (cam && fp6m_has_prop (cam, name))
    g_object_get (cam, name, &v, NULL);
  return v;
}

/* array property element i as double, or def */
static gdouble
fp6m_get_array (GstElement * cam, const gchar * name, guint i, gdouble def)
{
  GValue v = G_VALUE_INIT;
  gdouble r = def;

  if (!cam || !fp6m_has_prop (cam, name))
    return def;
  g_value_init (&v, GST_TYPE_ARRAY);
  g_object_get_property (G_OBJECT (cam), name, &v);
  if (gst_value_array_get_size (&v) > i) {
    const GValue *e = gst_value_array_get_value (&v, i);
    if (G_VALUE_HOLDS_FLOAT (e))
      r = g_value_get_float (e);
    else if (G_VALUE_HOLDS_INT64 (e))
      r = (gdouble) g_value_get_int64 (e);
    else if (G_VALUE_HOLDS_INT (e))
      r = g_value_get_int (e);
  }
  g_value_unset (&v);
  return r;
}

static void
fp6m_set_gains (GstElement * cam, gfloat r, gfloat b)
{
  GValue arr = G_VALUE_INIT, e = G_VALUE_INIT;

  g_value_init (&arr, GST_TYPE_ARRAY);
  g_value_init (&e, G_TYPE_FLOAT);
  g_value_set_float (&e, r);
  gst_value_array_append_value (&arr, &e);
  g_value_set_float (&e, b);
  gst_value_array_append_value (&arr, &e);
  g_object_set_property (G_OBJECT (cam), "colour-gains", &arr);
  g_value_unset (&e);
  g_value_unset (&arr);
}

/* Largest single-frame exposure, from FrameDurationLimits (patch 0090). */
static gint64
fp6m_max_exposure_locked (Fp6Manual * m)
{
  gdouble fdl = fp6m_get_array (m->cam, "frame-duration-limits", 1, 0);
  if (fdl <= 0)
    return 33333;               /* unknown: no long frames */
  /* the exposure is a few lines shorter than the frame */
  return (gint64) (fdl * 0.99);
}

static gboolean
fp6m_stacking (Fp6Manual * m)
{
  return m->st >= STK_SETTLE && m->st <= STK_DARK;
}

/* a stack or a phase 2 capture job holds the camera settings */
static gboolean
fp6m_forced (Fp6Manual * m)
{
  return fp6m_stacking (m) || m->ovr;
}

static gboolean
fp6m_anything_manual (Fp6Manual * m)
{
  return m->exposure_us > 0 || m->iso > 0 || fabsf (m->ev) > 0.01f
      || m->wb_kelvin > 0 || m->ae_lock || m->awb_lock || m->af_lock
      || m->focus >= 0 || fp6m_forced (m);
}

/* AfMode values (libcamera control_ids) */
#define FP6M_AF_MANUAL 0
#define FP6M_AF_AUTO 1
#define FP6M_AF_CONTINUOUS 2

/* Push the requested state to libcamerasrc. m->lock held. */
static void
fp6m_apply_locked (Fp6Manual * m)
{
  GstElement *cam = m->cam;
  gboolean stacking = fp6m_stacking (m);
  gboolean ovr = m->ovr && !stacking;
  gint64 exp;
  gdouble gain;

  if (!cam)
    return;
  /* Nothing manual and nothing ever sent: leave libcamera fully alone. */
  if (!fp6m_anything_manual (m) && !m->sent_anything)
    return;
  m->sent_anything = TRUE;

  /* exposure time: value first, then the mode (see 0090: the IPA keeps the
   * last ExposureTime it received, in any mode) */
  exp = stacking ? m->st_frame_us : ovr ? m->ovr_exp_us : m->exposure_us;
  if (!stacking && exp > 0)
    exp = MIN (exp, fp6m_max_exposure_locked (m));
  if (exp <= 0 && (m->ae_lock || stacking))
    exp = fp6m_get_int (cam, "exposure-time");
  if (exp > 0) {
    g_object_set (cam, "exposure-time", (gint) MIN (exp, G_MAXINT), NULL);
    gst_util_set_object_arg (G_OBJECT (cam), "exposure-time-mode", "manual");
  } else {
    gst_util_set_object_arg (G_OBJECT (cam), "exposure-time-mode", "auto");
  }

  gain = stacking ? m->st_gain : ovr ? m->ovr_gain
      : (m->iso > 0 ? m->iso / 100.0 : 0);
  if (gain <= 0 && (m->ae_lock || stacking))
    gain = fp6m_get_float (cam, "analogue-gain");
  if (gain > 0) {
    g_object_set (cam, "analogue-gain", (gfloat) gain, NULL);
    gst_util_set_object_arg (G_OBJECT (cam), "analogue-gain-mode", "manual");
  } else {
    gst_util_set_object_arg (G_OBJECT (cam), "analogue-gain-mode", "auto");
  }

  g_object_set (cam, "exposure-value", m->ev, NULL);

  /* white balance */
  if (ovr && m->ovr_gr > 0 && m->ovr_gb > 0) {
    fp6m_set_gains (cam, m->ovr_gr, m->ovr_gb);
    g_object_set (cam, "awb-enable", FALSE, NULL);
  } else if (m->wb_kelvin > 0 && fp6m_has_prop (cam, "colour-temperature")) {
    g_object_set (cam, "colour-temperature", m->wb_kelvin, NULL);
    g_object_set (cam, "awb-enable", FALSE, NULL);
  } else if (m->awb_lock || stacking) {
    gdouble r = fp6m_get_array (cam, "colour-gains", 0, 0);
    gdouble b = fp6m_get_array (cam, "colour-gains", 1, 0);
    if (r > 0 && b > 0)
      fp6m_set_gains (cam, r, b);
    g_object_set (cam, "awb-enable", FALSE, NULL);
  } else {
    g_object_set (cam, "awb-enable", TRUE, NULL);
  }

  /* focus */
  if (ovr && m->ovr_lens >= 0 && m->has_focus) {
    gst_util_set_object_arg (G_OBJECT (cam), "af-mode", "manual");
    g_object_set (cam, "lens-position", CLAMP (m->ovr_lens, 0.0f, 100.0f),
        NULL);
    m->sent_af_mode = FP6M_AF_MANUAL;
  } else if (m->focus >= 0 || m->af_lock) {
    gfloat pos = m->focus >= 0 ? m->focus * 100.0f
        : fp6m_get_float (cam, "lens-position");
    gst_util_set_object_arg (G_OBJECT (cam), "af-mode", "manual");
    g_object_set (cam, "lens-position", CLAMP (pos, 0.0f, 100.0f), NULL);
    m->sent_af_mode = FP6M_AF_MANUAL;
  } else if (m->sent_af_mode == FP6M_AF_MANUAL) {
    /* back to the camera's normal (continuous) autofocus */
    gst_util_set_object_arg (G_OBJECT (cam), "af-mode", "continuous");
    m->sent_af_mode = FP6M_AF_CONTINUOUS;
  }

  GST_INFO_OBJECT (m->src, "manual: exposure %" G_GINT64_FORMAT " us, gain "
      "%.2f, ev %.1f, wb %d K%s, focus %.2f%s%s%s", exp, gain, m->ev,
      m->wb_kelvin, m->awb_lock ? " (locked)" : "", m->focus,
      m->ae_lock ? " ae-lock" : "", m->af_lock ? " af-lock" : "",
      stacking ? " [stack]" : ovr ? " [job]" : "");
}

/* Sensor output size the AF windows refer to (0020: AfWindows are in
 * sensor-output pixels). */
static void
fp6m_sensor_size (Fp6Manual * m, gint * w, gint * h)
{
  if (m->ultrawide_built) {
    *w = 2104;                  /* OV13B10 2x2 binned */
    *h = 1560;
  } else {
    *w = 4096;                  /* IMX896 2x2 binned */
    *h = 3072;
  }
}

/* Tap-to-focus: one autofocus scan (0020: AfTrigger works in continuous
 * mode too). x/y: tap position normalised to the displayed (zoomed)
 * stream, or < 0 for the whole frame. */
static void
fp6m_trigger_autofocus_at (Fp6LibcameraSrc * self, gdouble x, gdouble y)
{
  Fp6Manual *m = self->manual;
  GstElement *cam;
  gfloat zoom;

  GST_OBJECT_LOCK (self);
  zoom = MAX (self->zoom, 1.0f);
  GST_OBJECT_UNLOCK (self);

  g_mutex_lock (&m->lock);
  cam = m->cam;
  if (!cam || !m->has_focus || m->focus >= 0 || m->af_lock
      || !fp6m_has_prop (cam, "af-trigger")) {
    g_mutex_unlock (&m->lock);
    return;
  }
  if (x >= 0 && y >= 0 && fp6m_has_prop (cam, "af-windows")) {
    GValue list = G_VALUE_INIT, rect = G_VALUE_INIT, v = G_VALUE_INIT;
    gint sw, sh, ww, wh, rx, ry;
    gint r[4];

    fp6m_sensor_size (m, &sw, &sh);
    /* undo the digital zoom (centre crop) */
    x = 0.5 + (CLAMP (x, 0.0, 1.0) - 0.5) / zoom;
    y = 0.5 + (CLAMP (y, 0.0, 1.0) - 0.5) / zoom;
    ww = sw / 6;
    wh = sh / 6;
    rx = CLAMP ((gint) (x * sw) - ww / 2, 0, sw - ww);
    ry = CLAMP ((gint) (y * sh) - wh / 2, 0, sh - wh);
    r[0] = rx;
    r[1] = ry;
    r[2] = ww;
    r[3] = wh;
    g_value_init (&list, GST_TYPE_ARRAY);
    g_value_init (&rect, GST_TYPE_ARRAY);
    g_value_init (&v, G_TYPE_INT);
    for (gint i = 0; i < 4; i++) {
      g_value_set_int (&v, r[i]);
      gst_value_array_append_value (&rect, &v);
    }
    gst_value_array_append_value (&list, &rect);
    g_object_set_property (G_OBJECT (cam), "af-windows", &list);
    g_value_unset (&v);
    g_value_unset (&rect);
    g_value_unset (&list);
    gst_util_set_object_arg (G_OBJECT (cam), "af-metering", "windows");
    GST_INFO_OBJECT (self, "autofocus at %.2f,%.2f -> window %d,%d %dx%d", x,
        y, rx, ry, ww, wh);
  } else if (fp6m_has_prop (cam, "af-metering")) {
    gst_util_set_object_arg (G_OBJECT (cam), "af-metering", "auto");
  }
  gst_util_set_object_arg (G_OBJECT (cam), "af-trigger", "start");
  m->sent_anything = TRUE;
  g_mutex_unlock (&m->lock);
}

/* ------------------------------------------------------------------------ */
/* analysis (streaming thread, m->lock held)                                */

static inline gint
fp6m_luma (const guint8 * p, gint ro, gint go, gint bo)
{
  return (p[ro] * 54 + p[go] * 183 + p[bo] * 19) >> 8;
}

static void
fp6m_analyse (Fp6Manual * m, GstVideoFrame * f)
{
  const GstVideoFormatInfo *fi = f->info.finfo;
  gint w = GST_VIDEO_FRAME_WIDTH (f), h = GST_VIDEO_FRAME_HEIGHT (f);
  gint stride = GST_VIDEO_FRAME_PLANE_STRIDE (f, 0);
  gint ps = GST_VIDEO_FRAME_COMP_PSTRIDE (f, 0);
  const guint8 *d = GST_VIDEO_FRAME_PLANE_DATA (f, 0);
  gint ro = GST_VIDEO_FORMAT_INFO_POFFSET (fi, GST_VIDEO_COMP_R);
  gint go = GST_VIDEO_FORMAT_INFO_POFFSET (fi, GST_VIDEO_COMP_G);
  gint bo = GST_VIDEO_FORMAT_INFO_POFFSET (fi, GST_VIDEO_COMP_B);
  guint32 hist[4 * FP6M_HIST_BINS] = { 0 };
  gint mw = FP6M_MASK_W, mh = MAX (1, (gint) ((gint64) FP6M_MASK_W * h / w));
  guint8 *mask = NULL;

  if (ps < 3 || w < 64 || h < 64)
    return;

  if (m->analysis & FP6M_ANALYSIS_HISTOGRAM) {
    for (gint y = 0; y < h; y += 4) {
      const guint8 *row = d + (gsize) y * stride;
      for (gint x = 0; x < w; x += 4) {
        const guint8 *p = row + x * ps;
        hist[fp6m_luma (p, ro, go, bo) >> 2]++;
        hist[FP6M_HIST_BINS + (p[ro] >> 2)]++;
        hist[2 * FP6M_HIST_BINS + (p[go] >> 2)]++;
        hist[3 * FP6M_HIST_BINS + (p[bo] >> 2)]++;
      }
    }
    g_clear_pointer (&m->hist, g_bytes_unref);
    m->hist = g_bytes_new (hist, sizeof hist);
  }

  if (m->analysis & (FP6M_ANALYSIS_ZEBRA | FP6M_ANALYSIS_PEAKING)) {
    guint8 *grad = g_new0 (guint8, mw * mh);
    gint gmax = 0;

    mask = g_new0 (guint8, mw * mh);
    for (gint cy = 0; cy < mh; cy++) {
      gint y0 = (gint) ((gint64) cy * h / mh), y1 =
          (gint) ((gint64) (cy + 1) * h / mh);
      for (gint cx = 0; cx < mw; cx++) {
        gint x0 = (gint) ((gint64) cx * w / mw), x1 =
            (gint) ((gint64) (cx + 1) * w / mw);
        gint sy = MAX (1, (y1 - y0) / 4), sx = MAX (1, (x1 - x0) / 4);
        gint best = 0;
        gboolean zebra = FALSE;
        for (gint y = y0; y < y1 - 1; y += sy) {
          const guint8 *row = d + (gsize) y * stride;
          for (gint x = x0; x < x1 - 1; x += sx) {
            const guint8 *p = row + x * ps;
            gint l = fp6m_luma (p, ro, go, bo);
            if (l >= m->zebra_level)
              zebra = TRUE;
            if (m->analysis & FP6M_ANALYSIS_PEAKING) {
              /* full-resolution gradient: fine detail drives peaking */
              gint gx = abs (fp6m_luma (p + ps, ro, go, bo) - l);
              gint gy = abs (fp6m_luma (p + stride, ro, go, bo) - l);
              best = MAX (best, gx + gy);
            }
          }
        }
        if (zebra && (m->analysis & FP6M_ANALYSIS_ZEBRA))
          mask[cy * mw + cx] |= 1;
        grad[cy * mw + cx] = MIN (best, 255);
        gmax = MAX (gmax, best);
      }
    }
    if (m->analysis & FP6M_ANALYSIS_PEAKING) {
      /* relative threshold, but never on a flat or noisy-only image */
      gint thr = MAX (28, gmax * 55 / 100);
      for (gint i = 0; i < mw * mh; i++)
        if (grad[i] >= thr)
          mask[i] |= 2;
    }
    /* same orientation as the element's output (videoflip after us) */
    if (m->flip && g_strcmp0 (m->flip, "none") != 0) {
      gboolean fx = !g_strcmp0 (m->flip, "rotate-180")
          || !g_strcmp0 (m->flip, "horizontal-flip");
      gboolean fy = !g_strcmp0 (m->flip, "rotate-180")
          || !g_strcmp0 (m->flip, "vertical-flip");
      guint8 *t = g_new (guint8, mw * mh);
      for (gint y = 0; y < mh; y++)
        for (gint x = 0; x < mw; x++)
          t[(fy ? mh - 1 - y : y) * mw + (fx ? mw - 1 - x : x)] =
              mask[y * mw + x];
      g_free (mask);
      mask = t;
    }
    g_free (grad);
    g_clear_pointer (&m->mask, g_bytes_unref);
    m->mask = g_bytes_new_take (mask, mw * mh);
    m->mask_w = mw;
    m->mask_h = mh;
  }
  m->serial++;
}

/* ------------------------------------------------------------------------ */
/* stacking                                                                 */

static void
fp6m_tables_init (void)
{
  static gsize once = 0;
  if (g_once_init_enter (&once)) {
    /* the softISP output is gamma encoded (~1/2.2); stack in linear light */
    for (gint i = 0; i < 256; i++)
      fp6m_lin[i] = powf (i / 255.0f, 2.2f);
    for (gint i = 0; i < 4096; i++)
      fp6m_gamma[i] = (guint8) lroundf (255.0f * powf (i / 4095.0f,
              1.0f / 2.2f));
    g_once_init_leave (&once, 1);
  }
}

static void
fp6m_stack_free_locked (Fp6Manual * m)
{
  g_clear_pointer (&m->acc, g_free);
  g_clear_pointer (&m->dark, g_free);
  g_clear_pointer (&m->preview, g_free);
}

static void
fp6m_accumulate (gfloat * acc, GstVideoFrame * f)
{
  const GstVideoFormatInfo *fi = f->info.finfo;
  gint w = GST_VIDEO_FRAME_WIDTH (f), h = GST_VIDEO_FRAME_HEIGHT (f);
  gint stride = GST_VIDEO_FRAME_PLANE_STRIDE (f, 0);
  gint ps = GST_VIDEO_FRAME_COMP_PSTRIDE (f, 0);
  const guint8 *d = GST_VIDEO_FRAME_PLANE_DATA (f, 0);
  gint ro = GST_VIDEO_FORMAT_INFO_POFFSET (fi, GST_VIDEO_COMP_R);
  gint go = GST_VIDEO_FORMAT_INFO_POFFSET (fi, GST_VIDEO_COMP_G);
  gint bo = GST_VIDEO_FORMAT_INFO_POFFSET (fi, GST_VIDEO_COMP_B);

  for (gint y = 0; y < h; y++) {
    const guint8 *p = d + (gsize) y * stride;
    gfloat *a = acc + (gsize) y * w * 3;
    for (gint x = 0; x < w; x++, p += ps, a += 3) {
      a[0] += fp6m_lin[p[ro]];
      a[1] += fp6m_lin[p[go]];
      a[2] += fp6m_lin[p[bo]];
    }
  }
}

/* Render the (dark-subtracted) result into a packed image of info's
 * layout. scale: 1 for sum, 1/frames for mean. */
static void
fp6m_render (const gfloat * acc, const gfloat * dark, gfloat dark_scale,
    gfloat scale, const GstVideoInfo * info, guint8 * out)
{
  const GstVideoFormatInfo *fi = info->finfo;
  gint w = GST_VIDEO_INFO_WIDTH (info), h = GST_VIDEO_INFO_HEIGHT (info);
  gint stride = GST_VIDEO_INFO_PLANE_STRIDE (info, 0);
  gint ps = GST_VIDEO_INFO_COMP_PSTRIDE (info, 0);
  gint off[3] = { GST_VIDEO_FORMAT_INFO_POFFSET (fi, GST_VIDEO_COMP_R),
    GST_VIDEO_FORMAT_INFO_POFFSET (fi, GST_VIDEO_COMP_G),
    GST_VIDEO_FORMAT_INFO_POFFSET (fi, GST_VIDEO_COMP_B)
  };

  for (gint y = 0; y < h; y++) {
    guint8 *p = out + (gsize) y * stride;
    const gfloat *a = acc + (gsize) y * w * 3;
    const gfloat *dk = dark ? dark + (gsize) y * w * 3 : NULL;
    for (gint x = 0; x < w; x++, p += ps, a += 3) {
      if (ps == 4)
        p[0] = p[1] = p[2] = p[3] = 0xff;       /* padding/alpha byte */
      for (gint c = 0; c < 3; c++) {
        gfloat v = a[c] - (dk ? dk[c] * dark_scale : 0.0f);
        gint i = (gint) (v * scale * 4095.0f);
        p[off[c]] = fp6m_gamma[CLAMP (i, 0, 4095)];
      }
      if (dk)
        dk += 3;
    }
  }
}


typedef struct
{
  Fp6LibcameraSrc *src;         /* ref */
  GstVideoInfo info;
  guint8 *image;                /* info.size bytes */
  gchar *path;
  gint orientation, quality, iso;
  gdouble exposure_s;
  gchar *flip;                  /* videoflip method or NULL */
  const gchar *software;        /* EXIF Software, NULL = long exposure */
} Fp6SaveJob;

static gboolean
fp6m_save_jpeg (Fp6SaveJob * job, GError ** err)
{
  GstElement *pipe, *appsrc, *conv, *enc, *mux, *sink, *flip = NULL;
  GstCaps *caps;
  GstBuffer *buf;
  GstBus *bus;
  GstMessage *msg;
  GstTagList *tags;
  GstDateTime *now;
  GstFlowReturn fr = GST_FLOW_OK;
  gchar *tmp, *orient;
  gint num, den;
  gboolean ok = FALSE;

  appsrc = gst_element_factory_make ("appsrc", NULL);
  conv = gst_element_factory_make ("videoconvert", NULL);
  enc = gst_element_factory_make ("jpegenc", NULL);
  mux = gst_element_factory_make ("jifmux", NULL);
  sink = gst_element_factory_make ("filesink", NULL);
  if (!appsrc || !conv || !enc || !mux || !sink) {
    g_set_error (err, GST_CORE_ERROR, GST_CORE_ERROR_MISSING_PLUGIN,
        "appsrc/videoconvert/jpegenc/jifmux/filesink missing");
    g_clear_object (&appsrc);
    g_clear_object (&conv);
    g_clear_object (&enc);
    g_clear_object (&mux);
    g_clear_object (&sink);
    return FALSE;
  }
  pipe = gst_pipeline_new ("fp6-stack-save");
  tmp = g_strdup_printf ("%s.part", job->path);
  caps = gst_video_info_to_caps (&job->info);
  gst_caps_set_simple (caps, "framerate", GST_TYPE_FRACTION, 0, 1, NULL);
  g_object_set (appsrc, "caps", caps, "format", GST_FORMAT_TIME, NULL);
  gst_caps_unref (caps);
  g_object_set (enc, "quality", CLAMP (job->quality, 10, 100), NULL);
  g_object_set (sink, "location", tmp, NULL);
  gst_bin_add_many (GST_BIN (pipe), appsrc, conv, enc, mux, sink, NULL);
  if (job->flip && (flip = gst_element_factory_make ("videoflip", NULL))) {
    gst_util_set_object_arg (G_OBJECT (flip), "method", job->flip);
    gst_bin_add (GST_BIN (pipe), flip);
  }
  if (!(flip ? gst_element_link_many (appsrc, flip, conv, enc, mux, sink,
              NULL) : gst_element_link_many (appsrc, conv, enc, mux, sink,
              NULL))) {
    g_set_error (err, GST_CORE_ERROR, GST_CORE_ERROR_NEGOTIATION,
        "cannot link the JPEG writer");
    goto out;
  }

  /* the gst-tag library's extra tags (capturing-*, image-orientation) */
  gst_tag_register_musicbrainz_tags ();
  tags = gst_tag_list_new_empty ();
  orient = g_strdup_printf ("rotate-%d", ((job->orientation % 360) + 360) %
      360);
  gst_util_double_to_fraction (job->exposure_s, &num, &den);
  now = gst_date_time_new_now_local_time ();
  gst_tag_list_add (tags, GST_TAG_MERGE_REPLACE,
      GST_TAG_IMAGE_ORIENTATION, orient,
      GST_TAG_CAPTURING_SHUTTER_SPEED, num, den,
      GST_TAG_DATE_TIME, now,
      GST_TAG_DEVICE_MANUFACTURER, "Fairphone",
      GST_TAG_DEVICE_MODEL, "FP6",
      GST_TAG_APPLICATION_NAME, job->software ? job->software :
      "lomiri-camera-app (fp6 long exposure)", NULL);
  if (job->iso > 0)
    gst_tag_list_add (tags, GST_TAG_MERGE_REPLACE,
        GST_TAG_CAPTURING_ISO_SPEED, job->iso, NULL);
  gst_tag_setter_merge_tags (GST_TAG_SETTER (mux), tags, GST_TAG_MERGE_REPLACE);
  gst_tag_list_unref (tags);
  gst_date_time_unref (now);
  g_free (orient);

  if (gst_element_set_state (pipe, GST_STATE_PLAYING) ==
      GST_STATE_CHANGE_FAILURE) {
    g_set_error (err, GST_CORE_ERROR, GST_CORE_ERROR_STATE_CHANGE,
        "cannot start the JPEG writer");
    goto out;
  }
  buf = gst_buffer_new_wrapped_full (GST_MEMORY_FLAG_READONLY, job->image,
      job->info.size, 0, job->info.size, NULL, NULL);
  GST_BUFFER_PTS (buf) = 0;
  g_signal_emit_by_name (appsrc, "push-buffer", buf, &fr);
  gst_buffer_unref (buf);
  g_signal_emit_by_name (appsrc, "end-of-stream", &fr);
  bus = gst_element_get_bus (pipe);
  msg = gst_bus_timed_pop_filtered (bus, 60 * GST_SECOND,
      GST_MESSAGE_EOS | GST_MESSAGE_ERROR);
  if (msg && GST_MESSAGE_TYPE (msg) == GST_MESSAGE_EOS)
    ok = TRUE;
  else if (msg)
    gst_message_parse_error (msg, err, NULL);
  else
    g_set_error (err, GST_CORE_ERROR, GST_CORE_ERROR_FAILED,
        "JPEG writer timed out");
  if (msg)
    gst_message_unref (msg);
  gst_object_unref (bus);
out:
  gst_element_set_state (pipe, GST_STATE_NULL);
  gst_object_unref (pipe);
  if (ok && g_rename (tmp, job->path) != 0) {
    g_set_error (err, G_FILE_ERROR, g_file_error_from_errno (errno),
        "cannot rename %s: %s", tmp, g_strerror (errno));
    ok = FALSE;
  }
  if (!ok)
    g_unlink (tmp);
  g_free (tmp);
  return ok;
}

static gpointer
fp6m_save_thread (gpointer data)
{
  Fp6SaveJob *job = data;
  Fp6Manual *m = job->src->manual;
  GError *err = NULL;
  gboolean ok = fp6m_save_jpeg (job, &err);

  g_mutex_lock (&m->lock);
  if (ok) {
    g_free (m->st_result);
    m->st_result = g_strdup (job->path);
    m->st = STK_DONE;
    GST_INFO_OBJECT (job->src, "long exposure saved: %s", job->path);
  } else {
    g_free (m->st_error);
    m->st_error = g_strdup (err && err->message ? err->message : "save failed");
    m->st = STK_ERROR;
    GST_WARNING_OBJECT (job->src, "long exposure save failed: %s",
        m->st_error);
  }
  g_mutex_unlock (&m->lock);
  g_clear_error (&err);
  g_free (job->image);
  g_free (job->path);
  g_free (job->flip);
  gst_object_unref (job->src);
  g_free (job);
  return NULL;
}

/* Light (and dark) frames are done: render, restore the live settings and
 * write the JPEG in a separate thread. m->lock held. */
static void
fp6m_stack_finish_locked (Fp6Manual * m)
{
  Fp6SaveJob *job;
  gint n = MAX (m->st_frames, 1);
  gfloat scale = m->st_mode == 1 ? 1.0f / n : 1.0f;
  /* acc - n * mean(dark): the same for the sum and (divided by n) mean */
  gfloat dark_scale = m->dark && m->st_dark_frames > 0 ?
      (gfloat) n / m->st_dark_frames : 0.0f;

  job = g_new0 (Fp6SaveJob, 1);
  job->src = gst_object_ref (m->src);
  job->info = m->info;
  job->image = g_malloc (m->info.size);
  fp6m_render (m->acc, dark_scale > 0 ? m->dark : NULL, dark_scale, scale,
      &m->info, job->image);
  job->path = g_strdup (m->st_path);
  job->flip = m->flip && g_strcmp0 (m->flip, "none") ? g_strdup (m->flip)
      : NULL;
  job->orientation = m->st_orientation;
  job->quality = m->st_quality;
  job->iso = (gint) lround (m->st_gain * 100.0);
  job->exposure_s = (m->st_mode == 1 ? 1 : n) * (gdouble) m->st_frame_us / 1e6;
  GST_INFO_OBJECT (m->src, "stack done: %d frames of %" G_GINT64_FORMAT
      " us (%s), %d dark frames -> %s", n, m->st_frame_us,
      m->st_mode == 1 ? "mean" : "sum", m->st_dark_frames, job->path);

  m->st = STK_SAVING;
  fp6m_stack_free_locked (m);
  fp6m_apply_locked (m);        /* back to the live (user) settings */
  g_thread_unref (g_thread_new ("fp6-stack-save", fp6m_save_thread, job));
}

static gfloat *
fp6m_alloc_plane (Fp6Manual * m)
{
  gsize n = (gsize) GST_VIDEO_INFO_WIDTH (&m->info) *
      GST_VIDEO_INFO_HEIGHT (&m->info) * 3;
  return g_try_new0 (gfloat, n);
}

static void
fp6m_stack_error_locked (Fp6Manual * m, const gchar * msg)
{
  GST_WARNING_OBJECT (m->src, "stack: %s", msg);
  g_free (m->st_error);
  m->st_error = g_strdup (msg);
  m->st = STK_ERROR;
  fp6m_stack_free_locked (m);
  fp6m_apply_locked (m);
}

static void
fp6m_update_preview_locked (Fp6Manual * m, gint64 now)
{
  if (!m->acc || m->st_frames == m->preview_frames
      || now - m->preview_last < 250000)
    return;
  if (!m->preview)
    m->preview = g_try_malloc (m->info.size);
  if (!m->preview)
    return;
  fp6m_render (m->acc, NULL, 0, m->st_mode == 1 ?
      1.0f / MAX (m->st_frames, 1) : 1.0f, &m->info, m->preview);
  m->preview_last = now;
  m->preview_frames = m->st_frames;
}

/* One frame while a stack is active (streaming thread, m->lock held). */
static void
fp6m_stack_frame_locked (Fp6Manual * m, GstVideoFrame * f, gint64 now)
{
  if (m->st_cancel) {
    m->st_cancel = FALSE;
    GST_INFO_OBJECT (m->src, "stack cancelled");
    m->st = STK_IDLE;
    fp6m_stack_free_locked (m);
    fp6m_apply_locked (m);
    return;
  }
  if (GST_VIDEO_FRAME_WIDTH (f) != m->acc_w
      || GST_VIDEO_FRAME_HEIGHT (f) != m->acc_h) {
    if (m->acc) {
      fp6m_stack_error_locked (m, "stream size changed");
      return;
    }
    m->acc_w = GST_VIDEO_FRAME_WIDTH (f);
    m->acc_h = GST_VIDEO_FRAME_HEIGHT (f);
  }

  switch (m->st) {
    case STK_SETTLE:{
      gint64 e = fp6m_get_int (m->cam, "exposure-time");
      gdouble g = fp6m_get_float (m->cam, "analogue-gain");
      gboolean ok = e > 0
          && llabs (e - m->st_frame_us) <= MAX (m->st_frame_us / 33, 60)
          && fabs (g - m->st_gain) <= 0.05 * m->st_gain + 0.05;

      if (m->st_stop) {
        m->st_stop = FALSE;
        m->st = STK_IDLE;
        fp6m_apply_locked (m);
        return;
      }
      m->st_settle_ok = ok ? m->st_settle_ok + 1 : 0;
      if (m->st_settle_ok < 2 && now < m->st_settle_deadline)
        return;
      if (!ok)
        GST_WARNING_OBJECT (m->src, "stack: exposure %" G_GINT64_FORMAT
            " us / gain %.2f not confirmed (have %" G_GINT64_FORMAT
            " / %.2f), starting anyway", m->st_frame_us, m->st_gain, e, g);
      m->acc = fp6m_alloc_plane (m);
      if (!m->acc) {
        fp6m_stack_error_locked (m, "out of memory");
        return;
      }
      m->st = STK_LIGHT;
      m->st_frames = 0;
      m->preview_frames = 0;
      m->preview_last = 0;
      GST_INFO_OBJECT (m->src, "stack: light frames start (%d x %"
          G_GINT64_FORMAT " us, gain %.2f)", m->st_target, m->st_frame_us,
          m->st_gain);
      /* this frame was exposed with the stack settings already */
    }
      /* fall through */
    case STK_LIGHT:
      if (!m->st_stop || m->st_frames == 0) {
        fp6m_accumulate (m->acc, f);
        m->st_frames++;
      }
      fp6m_update_preview_locked (m, now);
      if (m->st_stop || (m->st_target > 0 && m->st_frames >= m->st_target)
          || m->st_frames >= FP6M_STACK_MAX_FRAMES) {
        m->st_stop = FALSE;
        if (m->st_want_dark) {
          m->st = STK_WAIT_DARK;
          GST_INFO_OBJECT (m->src, "stack: waiting for the dark frames");
        } else {
          fp6m_stack_finish_locked (m);
        }
      }
      break;
    case STK_WAIT_DARK:
      if (m->st_stop) {
        m->st_stop = FALSE;
        fp6m_stack_finish_locked (m);   /* skip the dark frames */
      } else if (m->st_continue) {
        m->st_continue = FALSE;
        m->dark = fp6m_alloc_plane (m);
        if (!m->dark) {
          fp6m_stack_finish_locked (m);
          break;
        }
        m->st_dark_target = CLAMP (m->st_frames, 1, FP6M_DARK_MAX_FRAMES);
        m->st_dark_frames = 0;
        m->st_skip = 2;         /* frames possibly exposed while covering */
        m->st = STK_DARK;
      }
      break;
    case STK_DARK:
      if (m->st_skip > 0) {
        m->st_skip--;
      } else {
        fp6m_accumulate (m->dark, f);
        m->st_dark_frames++;
      }
      if (m->st_stop || m->st_dark_frames >= m->st_dark_target) {
        m->st_stop = FALSE;
        fp6m_stack_finish_locked (m);
      }
      break;
    default:
      break;
  }
}

/* crop sink pad (streaming thread, before the zoom crop) */
static GstPadProbeReturn
fp6m_probe (GstPad * pad, GstPadProbeInfo * info, gpointer user)
{
  Fp6LibcameraSrc *self = user;
  Fp6Manual *m = self->manual;
  GstBuffer *buf = GST_PAD_PROBE_INFO_BUFFER (info);
  GstVideoFrame f;
  gint64 now = g_get_monotonic_time ();
  gboolean analyse, stack, show;
  GstCaps *caps;

  g_mutex_lock (&m->lock);
  /* A long exposure requested before the camera ran was clamped to 1/30 s
   * (no FrameDurationLimits yet): apply again once the limits are known. */
  if (!m->fdl_known && m->cam
      && fp6m_get_array (m->cam, "frame-duration-limits", 1, 0) > 0) {
    m->fdl_known = TRUE;
    if (m->exposure_us > 33333)
      fp6m_apply_locked (m);
  }
  stack = fp6m_stacking (m);
  analyse = m->analysis && !stack && now - m->analysis_last >= 200000;
  if (!analyse && !stack) {
    g_mutex_unlock (&m->lock);
    return GST_PAD_PROBE_OK;
  }
  caps = gst_pad_get_current_caps (pad);
  if (caps && caps != m->caps) {
    gst_caps_replace (&m->caps, caps);
    m->info_ok = gst_video_info_from_caps (&m->info, caps)
        && GST_VIDEO_INFO_N_PLANES (&m->info) == 1
        && GST_VIDEO_INFO_COMP_PSTRIDE (&m->info, 0) >= 3
        && GST_VIDEO_FORMAT_INFO_IS_RGB (m->info.finfo);
  }
  if (caps)
    gst_caps_unref (caps);
  if (!m->info_ok || !gst_video_frame_map (&f, &m->info, buf, GST_MAP_READ)) {
    if (stack && !m->info_ok)
      fp6m_stack_error_locked (m, "unsupported stream format");
    g_mutex_unlock (&m->lock);
    return GST_PAD_PROBE_OK;
  }
  if (analyse) {
    m->analysis_last = now;
    fp6m_analyse (m, &f);
  }
  if (stack)
    fp6m_stack_frame_locked (m, &f, now);
  gst_video_frame_unmap (&f);

  /* the viewfinder shows the running result */
  show = m->preview && m->st >= STK_LIGHT && m->st <= STK_DARK;
  if (show) {
    GstBuffer *nb = gst_buffer_new_allocate (NULL, m->info.size, NULL);
    if (nb) {
      gst_buffer_fill (nb, 0, m->preview, m->info.size);
      gst_buffer_copy_into (nb, buf, GST_BUFFER_COPY_FLAGS |
          GST_BUFFER_COPY_TIMESTAMPS, 0, -1);
      gst_buffer_unref (buf);
      GST_PAD_PROBE_INFO_DATA (info) = nb;
    }
  }
  g_mutex_unlock (&m->lock);
  return GST_PAD_PROBE_OK;
}

/* ------------------------------------------------------------------------ */
/* action signals                                                           */

static gboolean
fp6m_stack_start (Fp6LibcameraSrc * self, const gchar * path, gint64 total_us,
    gint mode, gboolean dark, gint orientation, gint quality, gpointer unused)
{
  Fp6Manual *m = self->manual;
  gint64 max, frame, cur_e;
  gdouble gain, cur_g;
  gint target;

  g_mutex_lock (&m->lock);
  if (!m->cam || !path || !*path || fp6m_forced (m) || m->st == STK_SAVING) {
    g_mutex_unlock (&m->lock);
    return FALSE;
  }
  fp6m_tables_init ();
  max = fp6m_max_exposure_locked (m);
  total_us = CLAMP (total_us, 0, FP6M_STACK_MAX_US);
  mode = mode == 1 ? 1 : 0;
  cur_e = fp6m_get_int (m->cam, "exposure-time");
  cur_g = fp6m_get_float (m->cam, "analogue-gain");

  if (mode == 0 && total_us > 0) {
    /* sum: total split into equal frames of at most the sensor maximum */
    target = (gint) ((total_us + max - 1) / max);
    frame = total_us / MAX (target, 1);
  } else {
    /* mean, or bulb: frames of the set exposure (at most the maximum) */
    frame = m->exposure_us > 0 ? MIN (m->exposure_us, max) : max;
    target = total_us > 0 ? (gint) ((total_us + frame - 1) / frame) : 0;
  }
  frame = MAX (frame, 100);
  target = MIN (target, FP6M_STACK_MAX_FRAMES);

  if (m->iso > 0)
    gain = m->iso / 100.0;
  else if (mode == 0 && total_us > 0 && cur_e > 0 && cur_g > 0)
    /* auto ISO for a sum: expose the TOTAL like the live view's AE did */
    gain = MAX (1.0, cur_g * cur_e / (gdouble) total_us);
  else if (mode == 1 && cur_g > 0)
    gain = cur_g;
  else
    gain = 1.0;

  g_free (m->st_path);
  m->st_path = g_strdup (path);
  g_clear_pointer (&m->st_result, g_free);
  g_clear_pointer (&m->st_error, g_free);
  m->st_total_us = total_us;
  m->st_mode = mode;
  m->st_want_dark = dark;
  m->st_orientation = orientation;
  m->st_quality = quality > 0 ? quality : 95;
  m->st_frame_us = frame;
  m->st_gain = gain;
  m->st_target = target;
  m->st_frames = m->st_dark_frames = 0;
  m->st_settle_ok = 0;
  m->st_stop = m->st_continue = m->st_cancel = FALSE;
  m->acc_w = m->acc_h = 0;
  m->st_settle_deadline = g_get_monotonic_time () + 3 * G_USEC_PER_SEC +
      4 * frame;
  m->st = STK_SETTLE;
  GST_INFO_OBJECT (self, "stack start: %s, total %" G_GINT64_FORMAT " us, %d"
      " frames of %" G_GINT64_FORMAT " us, gain %.2f, dark %d -> %s",
      mode ? "mean" : "sum", total_us, target, frame, gain, dark, path);
  fp6m_apply_locked (m);
  g_mutex_unlock (&m->lock);
  return TRUE;
}

static void
fp6m_stack_stop (Fp6LibcameraSrc * self, gpointer unused)
{
  Fp6Manual *m = self->manual;
  g_mutex_lock (&m->lock);
  if (fp6m_stacking (m))
    m->st_stop = TRUE;
  g_mutex_unlock (&m->lock);
}

static void
fp6m_stack_continue (Fp6LibcameraSrc * self, gpointer unused)
{
  Fp6Manual *m = self->manual;
  g_mutex_lock (&m->lock);
  if (m->st == STK_WAIT_DARK)
    m->st_continue = TRUE;
  g_mutex_unlock (&m->lock);
}

static void
fp6m_stack_cancel (Fp6LibcameraSrc * self, gpointer unused)
{
  Fp6Manual *m = self->manual;
  g_mutex_lock (&m->lock);
  if (fp6m_stacking (m))
    m->st_cancel = TRUE;
  else if (m->st == STK_DONE || m->st == STK_ERROR)
    m->st = STK_IDLE;
  g_mutex_unlock (&m->lock);
}

static void
fp6m_signal_autofocus (Fp6LibcameraSrc * self, gdouble x, gdouble y,
    gpointer unused)
{
  fp6m_trigger_autofocus_at (self, x, y);
}

/* ------------------------------------------------------------------------ */
/* element hooks                                                            */

static Fp6Manual *
fp6m_new (Fp6LibcameraSrc * self)
{
  Fp6Manual *m = g_new0 (Fp6Manual, 1);
  m->src = self;
  g_mutex_init (&m->lock);
  m->focus = -1.0f;
  m->zebra_level = 245;
  m->sent_af_mode = -1;
  return m;
}

static void
fp6m_free (Fp6LibcameraSrc * self)
{
  Fp6Manual *m = self->manual;
  if (!m)
    return;
  fp6m_stack_free_locked (m);
  gst_clear_object (&m->cam);
  g_clear_pointer (&m->hist, g_bytes_unref);
  g_clear_pointer (&m->mask, g_bytes_unref);
  gst_clear_caps (&m->caps);
  g_free (m->st_path);
  g_free (m->st_result);
  g_free (m->st_error);
  g_free (m->flip);
  g_mutex_clear (&m->lock);
  g_free (m);
  self->manual = NULL;
}

/* lsrc_build hook, after the chain was linked */
static void
fp6m_attach (Fp6LibcameraSrc * self, GstElement * cam, GstElement * crop,
    gint idx)
{
  Fp6Manual *m = self->manual;
  GstPad *pad = gst_element_get_static_pad (crop, "sink");

  gst_pad_add_probe (pad, GST_PAD_PROBE_TYPE_BUFFER, fp6m_probe, self, NULL);
  gst_object_unref (pad);

  g_mutex_lock (&m->lock);
  gst_object_replace ((GstObject **) & m->cam, GST_OBJECT (cam));
  m->sent_anything = FALSE;
  m->sent_af_mode = -1;
  m->fdl_known = FALSE;
  m->has_focus = idx == 0;      /* both back lenses have a VCM */
  gst_clear_caps (&m->caps);
  m->info_ok = FALSE;
  fp6m_apply_locked (m);
  g_mutex_unlock (&m->lock);

  g_weak_ref_set (&fp6m_active_ref, self);
}

/* lsrc_clear hook */
static void
fp6m_detach (Fp6LibcameraSrc * self)
{
  Fp6Manual *m = self->manual;

  g_mutex_lock (&m->lock);
  if (fp6m_stacking (m)) {
    g_free (m->st_error);
    m->st_error = g_strdup ("camera stopped");
    m->st = STK_ERROR;
  }
  if (m->st != STK_SAVING)
    fp6m_stack_free_locked (m);
  gst_clear_object (&m->cam);
  gst_clear_caps (&m->caps);
  m->info_ok = FALSE;
  g_clear_pointer (&m->hist, g_bytes_unref);
  g_clear_pointer (&m->mask, g_bytes_unref);
  g_mutex_unlock (&m->lock);
}

static gboolean
fp6m_set_property (Fp6LibcameraSrc * self, guint id, const GValue * value)
{
  Fp6Manual *m = self->manual;
  gboolean apply = TRUE;

  if (id < FP6M_PROP_BASE || id >= FP6M_PROP_LAST)
    return FALSE;
  g_mutex_lock (&m->lock);
  switch (id) {
    case FP6M_PROP_EXPOSURE_US:
      m->exposure_us = MAX (g_value_get_int64 (value), 0);
      break;
    case FP6M_PROP_ISO:
      m->iso = g_value_get_int (value);
      break;
    case FP6M_PROP_EV:
      m->ev = g_value_get_float (value);
      break;
    case FP6M_PROP_WB_KELVIN:
      m->wb_kelvin = g_value_get_int (value);
      break;
    case FP6M_PROP_AE_LOCK:
      m->ae_lock = g_value_get_boolean (value);
      break;
    case FP6M_PROP_AWB_LOCK:
      m->awb_lock = g_value_get_boolean (value);
      break;
    case FP6M_PROP_AF_LOCK:
      m->af_lock = g_value_get_boolean (value);
      break;
    case FP6M_PROP_FOCUS:
      m->focus = g_value_get_float (value);
      if (m->focus < 0)
        m->focus = -1.0f;
      break;
    case FP6M_PROP_ULTRAWIDE:
      m->ultrawide = g_value_get_boolean (value);
      apply = FALSE;            /* takes effect at the next NULL->READY */
      break;
    case FP6M_PROP_ANALYSIS:
      m->analysis = g_value_get_uint (value);
      apply = FALSE;
      break;
    case FP6M_PROP_ZEBRA_LEVEL:
      m->zebra_level = g_value_get_int (value);
      apply = FALSE;
      break;
    default:
      apply = FALSE;
      break;
  }
  /* while stacking the stack settings stay; they are restored after */
  if (apply && !fp6m_forced (m))
    fp6m_apply_locked (m);
  g_mutex_unlock (&m->lock);
  return TRUE;
}

static gboolean
fp6m_get_property (Fp6LibcameraSrc * self, guint id, GValue * value)
{
  Fp6Manual *m = self->manual;

  if (id < FP6M_PROP_BASE || id >= FP6M_PROP_LAST)
    return FALSE;
  g_mutex_lock (&m->lock);
  switch (id) {
    case FP6M_PROP_EXPOSURE_US:
      g_value_set_int64 (value, m->exposure_us);
      break;
    case FP6M_PROP_ISO:
      g_value_set_int (value, m->iso);
      break;
    case FP6M_PROP_EV:
      g_value_set_float (value, m->ev);
      break;
    case FP6M_PROP_WB_KELVIN:
      g_value_set_int (value, m->wb_kelvin);
      break;
    case FP6M_PROP_AE_LOCK:
      g_value_set_boolean (value, m->ae_lock);
      break;
    case FP6M_PROP_AWB_LOCK:
      g_value_set_boolean (value, m->awb_lock);
      break;
    case FP6M_PROP_AF_LOCK:
      g_value_set_boolean (value, m->af_lock);
      break;
    case FP6M_PROP_FOCUS:
      g_value_set_float (value, m->focus);
      break;
    case FP6M_PROP_ULTRAWIDE:
      g_value_set_boolean (value, m->ultrawide);
      break;
    case FP6M_PROP_ANALYSIS:
      g_value_set_uint (value, m->analysis);
      break;
    case FP6M_PROP_ZEBRA_LEVEL:
      g_value_set_int (value, m->zebra_level);
      break;
    case FP6M_PROP_CUR_EXPOSURE_US:
      g_value_set_int64 (value, fp6m_get_int (m->cam, "exposure-time"));
      break;
    case FP6M_PROP_CUR_ISO:
      g_value_set_int (value, (gint) lroundf (100.0f *
              fp6m_get_float (m->cam, "analogue-gain")));
      break;
    case FP6M_PROP_CUR_KELVIN:
      g_value_set_int (value, fp6m_get_int (m->cam, "colour-temperature"));
      break;
    case FP6M_PROP_CUR_FOCUS:
      g_value_set_float (value, m->has_focus ?
          fp6m_get_float (m->cam, "lens-position") / 100.0f : -1.0f);
      break;
    case FP6M_PROP_FRAME_DURATION_US:
      g_value_set_int64 (value, fp6m_get_int64 (m->cam, "frame-duration"));
      break;
    case FP6M_PROP_MAX_EXPOSURE_US:
      g_value_set_int64 (value, m->cam ? fp6m_max_exposure_locked (m) : 0);
      break;
    case FP6M_PROP_AF_STATE:
      g_value_set_int (value, m->has_focus ? fp6m_get_int (m->cam,
              "af-state") : -1);
      break;
    case FP6M_PROP_HAS_FOCUS:
      g_value_set_boolean (value, m->cam && m->has_focus);
      break;
    case FP6M_PROP_HISTOGRAM:
      g_value_set_boxed (value, m->hist);
      break;
    case FP6M_PROP_OVERLAY:
      g_value_set_boxed (value, m->mask);
      break;
    case FP6M_PROP_OVERLAY_WIDTH:
      g_value_set_int (value, m->mask ? m->mask_w : 0);
      break;
    case FP6M_PROP_OVERLAY_HEIGHT:
      g_value_set_int (value, m->mask ? m->mask_h : 0);
      break;
    case FP6M_PROP_ANALYSIS_SERIAL:
      g_value_set_uint (value, m->serial);
      break;
    case FP6M_PROP_STACK_STATE:
      g_value_set_int (value, m->st);
      break;
    case FP6M_PROP_STACK_FRAMES:
      g_value_set_int (value, m->st_frames);
      break;
    case FP6M_PROP_STACK_TARGET_FRAMES:
      g_value_set_int (value, m->st_target);
      break;
    case FP6M_PROP_STACK_FRAME_US:
      g_value_set_int64 (value, m->st_frame_us);
      break;
    case FP6M_PROP_STACK_DARK_FRAMES:
      g_value_set_int (value, m->st_dark_frames);
      break;
    case FP6M_PROP_STACK_RESULT:
      g_value_set_string (value, m->st_result);
      break;
    case FP6M_PROP_STACK_ERROR:
      g_value_set_string (value, m->st_error);
      break;
    case FP6M_PROP_ACTIVE_LENS:{
      gint idx = device_index (self->device);
      g_value_set_string (value, idx == 1 ? "front" :
          (m->ultrawide_built ? "ultrawide" : "main"));
      break;
    }
    default:
      break;
  }
  g_mutex_unlock (&m->lock);
  return TRUE;
}

static void
fp6m_class_init (GObjectClass * oc)
{
  const GParamFlags rw = G_PARAM_READWRITE | G_PARAM_STATIC_STRINGS;
  const GParamFlags ro = G_PARAM_READABLE | G_PARAM_STATIC_STRINGS;
  GType t = G_TYPE_FROM_CLASS (oc);

#define P(id, spec) g_object_class_install_property (oc, id, spec)
  P (FP6M_PROP_EXPOSURE_US, g_param_spec_int64 ("exposure-us", "Exposure",
          "Manual exposure time in us (0 = auto); above max-exposure-us the "
          "preview uses the maximum and captures stack", 0, G_MAXINT64, 0,
          rw));
  P (FP6M_PROP_ISO, g_param_spec_int ("iso", "ISO",
          "Manual ISO (100 = analogue gain 1.0; 0 = auto)", 0, 25600, 0, rw));
  P (FP6M_PROP_EV, g_param_spec_float ("ev", "EV",
          "Exposure compensation in stops (auto exposure)", -4, 4, 0, rw));
  P (FP6M_PROP_WB_KELVIN, g_param_spec_int ("wb-kelvin", "WB Kelvin",
          "Manual white balance colour temperature (0 = auto)", 0, 15000, 0,
          rw));
  P (FP6M_PROP_AE_LOCK, g_param_spec_boolean ("ae-lock", "AE lock",
          "Hold the current automatic exposure time and gain", FALSE, rw));
  P (FP6M_PROP_AWB_LOCK, g_param_spec_boolean ("awb-lock", "AWB lock",
          "Hold the current automatic white balance", FALSE, rw));
  P (FP6M_PROP_AF_LOCK, g_param_spec_boolean ("af-lock", "AF lock",
          "Hold the current lens position", FALSE, rw));
  P (FP6M_PROP_FOCUS, g_param_spec_float ("focus", "Focus",
          "Manual lens position 0..1 of the VCM range (< 0 = autofocus)",
          -1, 1, -1, rw));
  P (FP6M_PROP_ULTRAWIDE, g_param_spec_boolean ("ultrawide", "Ultrawide",
          "Use the [ultrawide] camera instead of primary (next start)", FALSE,
          rw));
  P (FP6M_PROP_ANALYSIS, g_param_spec_uint ("analysis", "Analysis",
          "1 = histogram, 2 = zebra, 4 = focus peaking", 0, 7, 0, rw));
  P (FP6M_PROP_ZEBRA_LEVEL, g_param_spec_int ("zebra-level", "Zebra level",
          "Luma at or above which zebra stripes show", 128, 255, 245, rw));
  P (FP6M_PROP_CUR_EXPOSURE_US, g_param_spec_int64 ("current-exposure-us",
          "Current exposure", "Exposure time of the last frame (us)", 0,
          G_MAXINT64, 0, ro));
  P (FP6M_PROP_CUR_ISO, g_param_spec_int ("current-iso", "Current ISO",
          "Analogue gain of the last frame x 100", 0, G_MAXINT, 0, ro));
  P (FP6M_PROP_CUR_KELVIN, g_param_spec_int ("current-kelvin",
          "Current colour temperature", "Colour temperature (K)", 0,
          G_MAXINT, 0, ro));
  P (FP6M_PROP_CUR_FOCUS, g_param_spec_float ("current-focus",
          "Current focus", "Lens position 0..1 (-1 = no lens)", -1, 1, -1,
          ro));
  P (FP6M_PROP_FRAME_DURATION_US, g_param_spec_int64 ("frame-duration-us",
          "Frame duration", "Frame duration of the last frame (us)", 0,
          G_MAXINT64, 0, ro));
  P (FP6M_PROP_MAX_EXPOSURE_US, g_param_spec_int64 ("max-exposure-us",
          "Max exposure", "Longest single-frame exposure (us)", 0,
          G_MAXINT64, 0, ro));
  P (FP6M_PROP_AF_STATE, g_param_spec_int ("af-state", "AF state",
          "libcamera AfState (0 idle, 1 scanning, 2 focused, 3 failed; "
          "-1 no lens)", -1, 3, -1, ro));
  P (FP6M_PROP_HAS_FOCUS, g_param_spec_boolean ("has-focus", "Has focus",
          "The running camera has a focus lens", FALSE, ro));
  P (FP6M_PROP_HISTOGRAM, g_param_spec_boxed ("histogram", "Histogram",
          "4 x 64 guint32 bins: luma, R, G, B", G_TYPE_BYTES, ro));
  P (FP6M_PROP_OVERLAY, g_param_spec_boxed ("overlay", "Overlay",
          "overlay-width x overlay-height bytes, stream orientation: bit0 "
          "zebra, bit1 focus peaking", G_TYPE_BYTES, ro));
  P (FP6M_PROP_OVERLAY_WIDTH, g_param_spec_int ("overlay-width",
          "Overlay width", "Overlay mask width", 0, G_MAXINT, 0, ro));
  P (FP6M_PROP_OVERLAY_HEIGHT, g_param_spec_int ("overlay-height",
          "Overlay height", "Overlay mask height", 0, G_MAXINT, 0, ro));
  P (FP6M_PROP_ANALYSIS_SERIAL, g_param_spec_uint ("analysis-serial",
          "Analysis serial", "Bumped on every analysis", 0, G_MAXUINT, 0,
          ro));
  P (FP6M_PROP_STACK_STATE, g_param_spec_int ("stack-state", "Stack state",
          "0 idle 1 settle 2 light 3 wait-dark 4 dark 5 saving 6 done "
          "7 error", 0, 7, 0, ro));
  P (FP6M_PROP_STACK_FRAMES, g_param_spec_int ("stack-frames",
          "Stack frames", "Light frames accumulated", 0, G_MAXINT, 0, ro));
  P (FP6M_PROP_STACK_TARGET_FRAMES, g_param_spec_int ("stack-target-frames",
          "Stack target", "Light frames to take (0 = bulb)", 0, G_MAXINT, 0,
          ro));
  P (FP6M_PROP_STACK_FRAME_US, g_param_spec_int64 ("stack-frame-us",
          "Stack frame exposure", "Exposure of each stacked frame (us)", 0,
          G_MAXINT64, 0, ro));
  P (FP6M_PROP_STACK_DARK_FRAMES, g_param_spec_int ("stack-dark-frames",
          "Dark frames", "Dark frames accumulated", 0, G_MAXINT, 0, ro));
  P (FP6M_PROP_STACK_RESULT, g_param_spec_string ("stack-result",
          "Stack result", "Path of the last saved long exposure", NULL, ro));
  P (FP6M_PROP_STACK_ERROR, g_param_spec_string ("stack-error",
          "Stack error", "Why the last long exposure failed", NULL, ro));
  P (FP6M_PROP_ACTIVE_LENS, g_param_spec_string ("active-lens",
          "Active lens", "main | ultrawide | front", NULL, ro));
#undef P

  g_signal_new_class_handler ("stack-start", t,
      G_SIGNAL_RUN_LAST | G_SIGNAL_ACTION, G_CALLBACK (fp6m_stack_start),
      NULL, NULL, NULL, G_TYPE_BOOLEAN, 6, G_TYPE_STRING, G_TYPE_INT64,
      G_TYPE_INT, G_TYPE_BOOLEAN, G_TYPE_INT, G_TYPE_INT);
  g_signal_new_class_handler ("stack-stop", t,
      G_SIGNAL_RUN_LAST | G_SIGNAL_ACTION, G_CALLBACK (fp6m_stack_stop),
      NULL, NULL, NULL, G_TYPE_NONE, 0);
  g_signal_new_class_handler ("stack-continue", t,
      G_SIGNAL_RUN_LAST | G_SIGNAL_ACTION, G_CALLBACK (fp6m_stack_continue),
      NULL, NULL, NULL, G_TYPE_NONE, 0);
  g_signal_new_class_handler ("stack-cancel", t,
      G_SIGNAL_RUN_LAST | G_SIGNAL_ACTION, G_CALLBACK (fp6m_stack_cancel),
      NULL, NULL, NULL, G_TYPE_NONE, 0);
  /* x, y normalised to the displayed stream; negative = whole frame */
  g_signal_new_class_handler ("trigger-autofocus", t,
      G_SIGNAL_RUN_LAST | G_SIGNAL_ACTION, G_CALLBACK (fp6m_signal_autofocus),
      NULL, NULL, NULL, G_TYPE_NONE, 2, G_TYPE_DOUBLE, G_TYPE_DOUBLE);

  /* how the application finds the running element */
  fp6m_active_quark = g_quark_from_static_string ("fp6-manual-active");
  g_weak_ref_init (&fp6m_active_ref, NULL);
  g_type_set_qdata (t, fp6m_active_quark, &fp6m_active_ref);
}
